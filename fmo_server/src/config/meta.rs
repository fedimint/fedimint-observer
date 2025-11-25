use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, bail, Context};
use axum::extract::{Path, State};
use axum::Json;
use fedimint_api_client::api::DynGlobalApi;
use fedimint_core::config::{FederationId, JsonClientConfig};
use fedimint_core::invite_code::InviteCode;
use fedimint_meta_client::api::MetaFederationApi;
use fedimint_meta_client::common::MetaKey;
use serde::de::DeserializeOwned;
use tokio::sync::RwLock;
use tracing::warn;

use crate::meta::federation_meta;
use crate::AppState;

pub type MetaFields = BTreeMap<String, serde_json::Value>;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60 * 60);

pub async fn fetch_federation_meta(
    Path(invite): Path<InviteCode>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<MetaFields>> {
    let config = state
        .federation_config_cache
        .fetch_config_cached(&invite)
        .await?;

    federation_meta(&config, &state).await
}

#[derive(Default, Debug, Clone)]
pub struct MetaOverrideCache {
    client: reqwest::Client,
    override_files: Arc<tokio::sync::RwLock<HashMap<String, (serde_json::Value, SystemTime)>>>,
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
        Ok(self
            .client
            .get(url)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?)
    }
}

type ConsensusMetaCacheInner = Arc<RwLock<HashMap<FederationId, (Option<MetaFields>, SystemTime)>>>;

#[derive(Default, Debug, Clone)]
pub struct ConsensusMetaCache {
    metas: ConsensusMetaCacheInner,
}

impl ConsensusMetaCache {
    pub async fn fetch_meta_cached(&self, config: &JsonClientConfig) -> Option<MetaFields> {
        let federation_id = config.global.calculate_federation_id();
        let current_meta_cache_entry = {
            let metas = self.metas.read().await;
            metas.get(&federation_id).cloned()
        };

        let mut maybe_meta = match current_meta_cache_entry {
            Some((meta, last_update_started)) => {
                let now = SystemTime::now();
                if now.duration_since(last_update_started).unwrap_or_default() > REFRESH_INTERVAL {
                    {
                        let mut metas = self.metas.write().await;

                        // Check if another process has already started a background refresh
                        if now.duration_since(last_update_started).unwrap_or_default()
                            <= REFRESH_INTERVAL
                        {
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
                    tokio::task::spawn(async move {
                        Self::update_meta_cache(&self_inner, &config_inner).await;
                    });
                }
                meta
            }
            None => {
                // TODO: deduplicate efforts by making the content of the map subscribable
                Self::update_meta_cache(&self.metas, config).await
            }
        };

        if config.global.calculate_federation_id()
            == FederationId::from_str(
                "1bcb64e68ef0b3de3ad96cb98b43a2fd972a9ffa0fb6f0e26aaee69d1d463b97",
            )
            .expect("valid")
        {
            let mut meta_fields = maybe_meta.unwrap_or_default();
            meta_fields.insert(
                "federation_name".into(),
                serde_json::Value::String("Global Bitcoin Federation".into()),
            );
            maybe_meta = Some(meta_fields);
        }

        maybe_meta
    }

    pub async fn update_meta_cache(
        inner: &ConsensusMetaCacheInner,
        config: &JsonClientConfig,
    ) -> Option<MetaFields> {
        let federation_id = config.global.calculate_federation_id();
        let meta = Self::try_fetch_meta_inner(config)
            .await
            .map_err(|e| {
                warn!("Failed to fetch consensus meta for federation {federation_id}: {e}");
            })
            .ok()
            .flatten();
        let mut metas = inner.write().await;
        metas.insert(federation_id, (meta.clone(), SystemTime::now()));
        meta
    }

    async fn try_fetch_meta_inner(config: &JsonClientConfig) -> anyhow::Result<Option<MetaFields>> {
        let Some((meta_instance_id, _)) = config
            .modules
            .iter()
            .find(|(_, module)| module.kind().as_str() == "meta")
        else {
            bail!("No meta module found in federation");
        };

        let api_client = DynGlobalApi::from_endpoints(
            config
                .global
                .api_endpoints
                .iter()
                .map(|(peer_id, peer)| (*peer_id, peer.url.clone())),
            &None,
        )
        .await?;

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
