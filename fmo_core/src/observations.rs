//! Persisted, last-known-good snapshots. A failed poll never replaces good
//! data.

use std::time::Duration;

use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use futures::{stream, StreamExt};
use serde_json::Value;

use crate::config::meta::{parse_meta_lenient, ConsensusMetaCache, MetaFields, MetaOverrideCache};
use crate::federation::observer::FederationObserver;
use crate::module::Services;

impl Services {
    pub async fn save_observation(
        &self,
        federation_id: FederationId,
        kind: &str,
        data: &Value,
    ) -> anyhow::Result<()> {
        self.connection().await?.execute(
            "INSERT INTO federation_observations(federation_id, kind, data) VALUES ($1, $2, $3)
             ON CONFLICT (federation_id, kind) DO UPDATE SET data = EXCLUDED.data, observed_at = now()",
            &[&federation_id.consensus_encode_to_vec(), &kind, data],
        ).await?;
        Ok(())
    }

    pub async fn observation(
        &self,
        federation_id: FederationId,
        kind: &str,
    ) -> anyhow::Result<Option<(Value, DateTime<Utc>)>> {
        Ok(self.connection().await?.query_opt(
            "SELECT data, observed_at FROM federation_observations WHERE federation_id = $1 AND kind = $2",
            &[&federation_id.consensus_encode_to_vec(), &kind],
        ).await?.map(|row| (row.get(0), row.get(1))))
    }
}

impl FederationObserver {
    pub(crate) async fn prune_health(self, days: u32) -> anyhow::Result<()> {
        loop {
            self.connection().await?.execute(
                "DELETE FROM guardian_health WHERE (federation_id, guardian_id, time) IN (
                    SELECT h.federation_id, h.guardian_id, h.time FROM guardian_health h
                    WHERE h.time < (now() AT TIME ZONE 'UTC') - make_interval(days => $1)
                      AND EXISTS (SELECT 1 FROM guardian_health newer
                          WHERE newer.federation_id = h.federation_id AND newer.guardian_id = h.guardian_id
                            AND newer.time > h.time)
                    ORDER BY h.time LIMIT 5000
                 )", &[&(days as i32)],
            ).await?;
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

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
                                ConsensusMetaCache::try_fetch_meta_inner(
                                    &config,
                                    observer.connectors(),
                                ),
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
