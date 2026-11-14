//! Collection of federations' metadata: every 5 minutes, each federation's
//! consensus meta and its override document are fetched and stored as
//! observations, with the merged result the API serves (`meta`).

use std::time::Duration;

use futures::{stream, StreamExt};
use serde_json::Value;

use crate::config::meta::{parse_meta_lenient, ConsensusMetaCache, MetaFields, MetaOverrideCache};
use crate::federation::observer::FederationObserver;
use crate::module::Services;

impl FederationObserver {
    pub(crate) async fn refresh_metadata(self) -> anyhow::Result<()> {
        let services = Services::new(self.clone());
        let overrides = MetaOverrideCache::default();
        loop {
            stream::iter(self.list_federations().await?)
                .for_each_concurrent(4, |federation| {
                    let services = services.clone();
                    let observer = self.clone();
                    let overrides = overrides.clone();
                    async move {
                        let config = federation.config.to_json();
                        let configured = parse_meta_lenient(
                            config
                                .global
                                .meta
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone().into())),
                        );
                        let result = async {
                            let _permit = services.external_io_permit().await?;
                            if let Ok(Ok(meta)) = tokio::time::timeout(
                                Duration::from_secs(20),
                                ConsensusMetaCache::try_fetch_meta_inner(&config, &observer),
                            )
                            .await
                            {
                                services
                                    .save_observation(
                                        federation.federation_id,
                                        "meta-consensus",
                                        &serde_json::to_value(meta.unwrap_or_default())?,
                                    )
                                    .await?;
                            }
                            if let Some(url) = configured
                                .get("meta_override_url")
                                .or_else(|| configured.get("meta_external_url"))
                                .and_then(Value::as_str)
                            {
                                if let Ok(meta) = overrides
                                    .fetch_meta_cached(url, federation.federation_id)
                                    .await
                                {
                                    services
                                        .save_observation(
                                            federation.federation_id,
                                            "meta-override",
                                            &serde_json::to_value(meta)?,
                                        )
                                        .await?;
                                }
                            }
                            let mut sources = Vec::new();
                            for kind in ["meta-consensus", "meta-override"] {
                                sources.push(
                                    services
                                        .observation(federation.federation_id, kind)
                                        .await?
                                        .map(|(value, _)| {
                                            serde_json::from_value::<MetaFields>(value)
                                        })
                                        .transpose()?
                                        .unwrap_or_default(),
                                );
                            }
                            sources.push(configured);
                            services
                                .save_observation(
                                    federation.federation_id,
                                    "meta",
                                    &serde_json::to_value(crate::util::merge_metas(&sources))?,
                                )
                                .await
                        }
                        .await;
                        if let Err(error) = result {
                            tracing::warn!(%error, "Could not persist federation metadata");
                        }
                    }
                })
                .await;
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    }
}
