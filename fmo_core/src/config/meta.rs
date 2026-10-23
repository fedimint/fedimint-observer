use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, ensure, Context};
use axum::extract::{Path, State};
use axum::Json;
use fedimint_connectors::ConnectorRegistry;
use fedimint_core::config::{FederationId, JsonClientConfig};
use fedimint_core::invite_code::InviteCode;
use fedimint_meta_client::api::MetaFederationApi;
use fedimint_meta_client::common::MetaKey;
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;
use tracing::warn;

use crate::meta::federation_meta;
use crate::util::{federation_api, make_room};
use crate::AppState;

pub type MetaFields = BTreeMap<String, serde_json::Value>;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Override URLs and consensus metas come from federation configs anyone can
/// make the observer download, so both caches are bounded
const MAX_CACHED_METAS: usize = 256;
/// Meta files are small JSON documents
const MAX_META_OVERRIDE_BYTES: usize = 1024 * 1024;
const CONSENSUS_META_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn fetch_federation_meta(
    Path(invite): Path<InviteCode>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<MetaFields>> {
    let config = state
        .federation_config_cache
        .fetch_config_cached(&invite, &state.federation_observer)
        .await?;

    federation_meta(&config, &state).await
}

#[derive(Debug, Clone)]
pub struct MetaOverrideCache {
    client: reqwest::Client,
    override_files: Arc<tokio::sync::RwLock<HashMap<String, (serde_json::Value, SystemTime)>>>,
}

impl Default for MetaOverrideCache {
    fn default() -> Self {
        Self {
            // The URL comes from a federation's config: bound how long a fetch
            // can hang and how far it can be redirected
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::limited(3))
                .build()
                .expect("valid HTTP client configuration"),
            override_files: Default::default(),
        }
    }
}

impl MetaOverrideCache {
    pub async fn fetch_meta_cached(
        &self,
        url: &str,
        federation_id: FederationId,
    ) -> anyhow::Result<MetaFields> {
        let current_meta_cache_entry = self.override_files.read().await.get(url).cloned();
        let meta = match current_meta_cache_entry {
            Some((meta, last_update))
                if SystemTime::now()
                    .duration_since(last_update)
                    .unwrap_or_default()
                    <= REFRESH_INTERVAL =>
            {
                meta
            }
            _ => {
                let meta = self.fetch_meta_inner(url).await?;
                let mut cache = self.override_files.write().await;
                if !cache.contains_key(url) {
                    make_room(&mut cache, MAX_CACHED_METAS);
                }
                cache.insert(url.to_owned(), (meta.clone(), SystemTime::now()));
                meta
            }
        };

        let federation_meta = parse_meta_lenient(serde_json::from_value::<MetaFields>(
            meta.get(federation_id.to_string())
                .ok_or_else(|| anyhow!("No entry for federation {federation_id} in {url}"))?
                .clone(),
        )?);
        Ok(federation_meta)
    }

    async fn fetch_meta_inner(&self, url: &str) -> anyhow::Result<serde_json::Value> {
        let mut response = self.client.get(url).send().await?.error_for_status()?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            body.extend_from_slice(&chunk);
            ensure!(
                body.len() <= MAX_META_OVERRIDE_BYTES,
                "Meta override file is larger than {MAX_META_OVERRIDE_BYTES} bytes"
            );
        }
        Ok(serde_json::from_slice(&body)?)
    }
}

type ConsensusMetaCacheInner = Arc<RwLock<HashMap<FederationId, (Option<MetaFields>, SystemTime)>>>;

#[derive(Default, Debug, Clone)]
pub struct ConsensusMetaCache {
    metas: ConsensusMetaCacheInner,
}

impl ConsensusMetaCache {
    pub async fn fetch_meta_cached(
        &self,
        config: &JsonClientConfig,
        connectors: &ConnectorRegistry,
    ) -> Option<MetaFields> {
        let federation_id = config.global.calculate_federation_id();
        let current_meta_cache_entry = {
            let metas = self.metas.read().await;
            metas.get(&federation_id).cloned()
        };

        match current_meta_cache_entry {
            Some((meta, last_update_started)) => {
                let now = SystemTime::now();
                if now.duration_since(last_update_started).unwrap_or_default() > REFRESH_INTERVAL {
                    {
                        let mut metas = self.metas.write().await;

                        // Another request may have started a refresh since we
                        // read the entry
                        if metas.get(&federation_id).is_some_and(|(_, started)| {
                            now.duration_since(*started).unwrap_or_default() <= REFRESH_INTERVAL
                        }) {
                            return meta;
                        }

                        // Since this process is about to start a background refresh, we update the
                        // timestamp. No crash tolerance needed since it's an in-memory cache that
                        // gets reset on crash anyway.
                        metas
                            .entry(federation_id)
                            .and_modify(|(_val, timestamp)| *timestamp = SystemTime::now());
                    }

                    let self_inner = self.metas.clone();
                    let config_inner = config.clone();
                    let connectors = connectors.clone();
                    tokio::task::spawn(async move {
                        Self::update_meta_cache(&self_inner, &config_inner, &connectors).await;
                    });
                }
                meta
            }
            None => {
                // TODO: deduplicate efforts by making the content of the map subscribable
                Self::update_meta_cache(&self.metas, config, connectors).await
            }
        }
    }

    pub async fn update_meta_cache(
        inner: &ConsensusMetaCacheInner,
        config: &JsonClientConfig,
        connectors: &ConnectorRegistry,
    ) -> Option<MetaFields> {
        let federation_id = config.global.calculate_federation_id();
        let meta = tokio::time::timeout(
            CONSENSUS_META_TIMEOUT,
            Self::try_fetch_meta_inner(config, connectors),
        )
        .await
        .unwrap_or_else(|_| Err(anyhow!("timed out")))
        .map_err(|e| {
            warn!("Failed to fetch consensus meta for federation {federation_id}: {e}");
        })
        .ok()
        .flatten();
        let mut metas = inner.write().await;
        if !metas.contains_key(&federation_id) {
            make_room(&mut metas, MAX_CACHED_METAS);
        }
        metas.insert(federation_id, (meta.clone(), SystemTime::now()));
        meta
    }

    async fn try_fetch_meta_inner(
        config: &JsonClientConfig,
        connectors: &ConnectorRegistry,
    ) -> anyhow::Result<Option<MetaFields>> {
        let Some((meta_instance_id, _)) = config
            .modules
            .iter()
            .find(|(_, module)| module.kind().as_str() == "meta")
        else {
            bail!("No meta module found in federation");
        };

        // Reuse the observer's registry: one built per refresh keeps its
        // guardian connections open
        let api_client = federation_api(connectors, &config.global)?;

        let module_api = api_client.with_module(*meta_instance_id);

        let Some(raw_consensus_meta) =
            MetaFederationApi::get_consensus(&*module_api, MetaKey(0)).await?
        else {
            return Ok(None);
        };

        let consensus_meta_object = raw_consensus_meta.value.to_json_lossy()?;
        let consensus_meta_map = consensus_meta_object
            .as_object()
            .context("Failed to parse consensus meta as JSON object")?;

        Ok(Some(parse_meta_lenient(consensus_meta_map.clone())))
    }
}

pub fn parse_meta_lenient(
    meta: impl IntoIterator<Item = (String, serde_json::Value)>,
) -> MetaFields {
    meta.into_iter()
        .filter_map(|(key, value)| {
            let value_string = value.as_str()?.to_owned();
            let value = serde_json::from_str(&value_string).unwrap_or_else(|_| value_string.into());
            Some((key, value))
        })
        .collect()
}

pub trait MetaFieldsExt {
    fn get_as<T: DeserializeOwned>(&self, key: &str) -> Option<T>;
}

impl MetaFieldsExt for MetaFields {
    fn get_as<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        serde_json::from_value(self.get(key).cloned()?)
            .inspect_err(|error| warn!(?error, "Could not decode meta field {key}"))
            .ok()
    }
}
