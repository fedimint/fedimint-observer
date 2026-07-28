use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use axum::extract::{Path, State};
use axum::Json;
use fedimint_api_client::api::{DynGlobalApi, FederationApiExt, StatusResponse};
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::encoding::Encodable;
use fedimint_core::endpoint_constants::STATUS_ENDPOINT;
use fedimint_core::module::ApiRequestErased;
use fedimint_core::{NumPeers, PeerId};
use fedimint_wallet_common::endpoint_constants::BLOCK_COUNT_LOCAL_ENDPOINT;
use fmo_api_types::{FederationHealth, GuardianHealth, GuardianHealthLatest};
use futures::future::join_all;
use postgres_from_row::FromRow;

use crate::federation::observer::FederationObserver;
use crate::util::query;

impl FederationObserver {
    pub async fn monitor_health(
        &self,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        const REQUEST_INTERVAL: Duration = Duration::from_secs(60);

        let mut interval = tokio::time::interval(REQUEST_INTERVAL);
        let api = DynGlobalApi::from_endpoints(
            config
                .global
                .api_endpoints
                .iter()
                .map(|(&peer_id, peer_url)| (peer_id, peer_url.url.clone())),
            &None,
        )
        .await?;

        let wallet_module = config
            .modules
            .iter()
            .find_map(|(&module_instance_id, module)| {
                (module.kind.as_str() == "wallet").then_some(module_instance_id)
            })
            .context("Wallet module not found")?;

        loop {
            interval.tick().await;

            let peer_status_responses =
                join_all(config.global.api_endpoints.keys().map(|&peer_id| {
                    let api = api.clone();
                    async move {
                        // We don't time the first request, there might be a reconnect happening in
                        // the background
                        let status = api
                            .request_single_peer(
                                STATUS_ENDPOINT.to_owned(),
                                ApiRequestErased::default(),
                                peer_id,
                            )
                            .await
                            .ok()
                            .and_then(|json| serde_json::from_value::<StatusResponse>(json).ok());

                        // Second request is used to determine ping
                        // TODO: how much time does bitcoind take to answer if at all (caching?)?
                        let start_time = Instant::now();
                        let block_height = api
                            .with_module(wallet_module)
                            .request_single_peer(
                                BLOCK_COUNT_LOCAL_ENDPOINT.to_owned(),
                                ApiRequestErased::default(),
                                peer_id,
                            )
                            .await
                            .ok()
                            .and_then(|json| {
                                serde_json::from_value::<Option<u32>>(json).ok().flatten()
                            })
                            .map(|block_count| {
                                // Fedimint uses 1-based block heights, while bitcoind uses 0-based
                                // heights
                                block_count - 1
                            });
                        let api_latency = start_time.elapsed();

                        (peer_id, status, block_height, api_latency)
                    }
                }))
                .await;

            let mut conn = self.connection().await?;
            let dbtx = conn.transaction().await?;
            let timestamp = chrono::Utc::now().naive_utc();
            for (peer_id, status, block_height, api_latency) in peer_status_responses {
                dbtx.execute(
                    "INSERT INTO guardian_health VALUES ($1, $2, $3, $4, $5, $6)",
                    &[
                        &federation_id.consensus_encode_to_vec(),
                        &timestamp,
                        &(peer_id.to_usize() as i32),
                        &status.map(|s| serde_json::to_value(s).expect("Can be serialized")),
                        &block_height.map(|bh| bh as i32),
                        &(api_latency.as_millis() as i32),
                    ],
                )
                .await?;
            }
            dbtx.commit().await?;
        }
    }

    pub async fn get_guardian_health(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<BTreeMap<PeerId, GuardianHealth>> {
        let _federation = self
            .get_federation(federation_id)
            .await
            .context("Unknown federation")?;

        let health_rows = query::<GuardianHealthRow>(
            &self.connection().await?,
            // The `guardians` CTE emulates an index skip-scan over the
            // (federation_id, guardian_id, time) index to enumerate the
            // federation's guardian ids. Naively grouping by guardian_id to get
            // the latest row per guardian instead degenerates into a full scan
            // of the (large) guardian_health table, since PostgreSQL has no
            // native skip-scan. With the guardian ids known we can fetch each
            // latest row with a single index descent.
            // language=postgresql
            "WITH RECURSIVE guardians AS (
                 SELECT (SELECT h.guardian_id
                         FROM guardian_health h
                         WHERE h.federation_id = $1
                         ORDER BY h.guardian_id
                         LIMIT 1) AS guardian_id
               UNION ALL
                 SELECT (SELECT h.guardian_id
                         FROM guardian_health h
                         WHERE h.federation_id = $1
                           AND h.guardian_id > g.guardian_id
                         ORDER BY h.guardian_id
                         LIMIT 1)
                 FROM guardians g
                 WHERE g.guardian_id IS NOT NULL
             )
             SELECT
                g.guardian_id,
                latest.block_height,
                (latest.status -> 'federation' ->> 'session_count')::integer AS session_count,
                last30d.uptime,
                last30d.latency_ms
             FROM guardians g
             CROSS JOIN LATERAL (
                 SELECT h.block_height, h.status
                 FROM guardian_health h
                 WHERE h.federation_id = $1
                   AND h.guardian_id = g.guardian_id
                 ORDER BY h.time DESC
                 LIMIT 1
             ) latest
             INNER JOIN (
                 SELECT
                     guardian_id,
                     (COUNT(status)::decimal / COUNT(*)::decimal * 100)::real as uptime,
                     AVG(latency_ms)::real as latency_ms
                 FROM guardian_health
                 WHERE federation_id = $1
                   AND time > NOW() - INTERVAL '30 days'
                 GROUP BY guardian_id
             ) last30d ON g.guardian_id = last30d.guardian_id
             WHERE g.guardian_id IS NOT NULL",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        let our_block_height = self.get_block_height().await?;
        let max_session = health_rows
            .iter()
            .filter_map(|row| row.session_count)
            .max()
            .unwrap_or_default() as u32;

        Ok(health_rows
            .into_iter()
            .map(|row| {
                let latest = if row.session_count.is_some() && row.block_height.is_some() {
                    let block_height = row.block_height.expect("checked above") as u32;
                    let session_count = row.session_count.expect("checked above") as u32;
                    Some(GuardianHealthLatest {
                        block_height,
                        block_outdated: our_block_height.saturating_sub(block_height) > 6,
                        session_count,
                        session_outdated: max_session.saturating_sub(session_count) > 1,
                    })
                } else {
                    None
                };

                let health = GuardianHealth {
                    avg_uptime: row.uptime,
                    avg_latency: row.latency_ms,
                    latest,
                };

                (PeerId::new(row.guardian_id as u16), health)
            })
            .collect())
    }

    pub async fn get_guardian_health_summary(
        &self,
    ) -> anyhow::Result<BTreeMap<FederationId, FederationHealth>> {
        #[derive(FromRow)]
        struct FederationHealthRow {
            federation_id: Vec<u8>,
            guardians: i32,
            online_guardians: i32,
        }

        let federations = query::<FederationHealthRow>(
            &self.connection().await?,
            // See `get_guardian_health` for why the guardian ids are enumerated
            // with a recursive CTE instead of grouping over guardian_health:
            // it emulates the index skip-scan PostgreSQL lacks, turning a full
            // table scan into a handful of index descents.
            // language=postgresql
            "WITH RECURSIVE guardians AS (
                 SELECT f.federation_id,
                        (SELECT h.guardian_id
                         FROM guardian_health h
                         WHERE h.federation_id = f.federation_id
                         ORDER BY h.guardian_id
                         LIMIT 1) AS guardian_id
                 FROM federations f
               UNION ALL
                 SELECT g.federation_id,
                        (SELECT h.guardian_id
                         FROM guardian_health h
                         WHERE h.federation_id = g.federation_id
                           AND h.guardian_id > g.guardian_id
                         ORDER BY h.guardian_id
                         LIMIT 1)
                 FROM guardians g
                 WHERE g.guardian_id IS NOT NULL
             )
             SELECT
                g.federation_id,
                COUNT(*)::int as guardians,
                COUNT(*) FILTER (WHERE latest.status -> 'federation' ->> 'session_count' IS NOT NULL)::int as online_guardians
             FROM guardians g
             CROSS JOIN LATERAL (
                 SELECT h.status
                 FROM guardian_health h
                 WHERE h.federation_id = g.federation_id
                   AND h.guardian_id = g.guardian_id
                 ORDER BY h.time DESC
                 LIMIT 1
             ) latest
             WHERE g.guardian_id IS NOT NULL
             GROUP BY g.federation_id",
            &[],
        )
        .await?;

        federations
            .into_iter()
            .map(|federation| {
                let federation_id = FederationId(bitcoin::hashes::Hash::from_byte_array(
                    federation
                        .federation_id
                        .try_into()
                        .map_err(|_| anyhow!("Invalid federation id in DB"))?,
                ));

                // Special case single guardian federations to not show them as degraded
                if federation.guardians == 1 {
                    return Ok((federation_id, FederationHealth::Online));
                }

                let threshold = NumPeers::from(federation.guardians as usize).threshold();
                let online = federation.online_guardians as usize;

                #[allow(clippy::comparison_chain)]
                if online > threshold {
                    Ok((federation_id, FederationHealth::Online))
                } else if online == threshold {
                    Ok((federation_id, FederationHealth::Degraded))
                } else {
                    Ok((federation_id, FederationHealth::Offline))
                }
            })
            .collect()
    }
}

#[derive(FromRow)]
struct GuardianHealthRow {
    guardian_id: i32,
    block_height: Option<i32>,
    session_count: Option<i32>,
    uptime: f32,
    latency_ms: f32,
}

pub(super) async fn get_federation_health(
    Path(federation_id): Path<FederationId>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<BTreeMap<PeerId, GuardianHealth>>> {
    let guardian_health = state
        .federation_observer
        .get_guardian_health(federation_id)
        .await?;

    Ok(Json(guardian_health))
}
