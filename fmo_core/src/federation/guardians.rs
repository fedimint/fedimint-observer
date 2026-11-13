use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::Json;
use chrono::{NaiveDateTime, TimeDelta};
use fedimint_api_client::api::{FederationApiExt, StatusResponse};
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::encoding::Encodable;
use fedimint_core::endpoint_constants::STATUS_ENDPOINT;
use fedimint_core::module::ApiRequestErased;
use fedimint_core::{NumPeers, PeerId};
use fedimint_wallet_common::endpoint_constants::BLOCK_COUNT_LOCAL_ENDPOINT;
use fmo_api_types::{FederationHealth, GuardianHealth, GuardianHealthLatest};
use futures::future::join_all;
use postgres_from_row::FromRow;
use tracing::info;

use crate::federation::observer::FederationObserver;
use crate::util::{module_instance_id, query};

mod history;
pub(super) use history::{get_guardian_health_history, get_guardian_live_health};

impl FederationObserver {
    pub async fn monitor_health(
        &self,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        let api = self.federation_api(&config.global)?;
        let block_source = BlockHeightSource::of(&config);

        let poll_guardians = async || {
            join_all(config.global.api_endpoints.keys().map(|&peer_id| {
                let api = api.clone();
                async move {
                    // We don't time the first request, there might be a reconnect happening in
                    // the background
                    let status = tokio::time::timeout(
                        Duration::from_secs(10),
                        api.request_single_peer(
                            STATUS_ENDPOINT.to_owned(),
                            ApiRequestErased::default(),
                            peer_id,
                        ),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .and_then(|json| serde_json::from_value::<StatusResponse>(json).ok());

                    // Second request is used to determine ping
                    // TODO: how much time does bitcoind take to answer if at all (caching?)?
                    let start_time = Instant::now();
                    let timed_status = tokio::time::timeout(
                        Duration::from_secs(10),
                        api.request_single_peer::<StatusResponse>(
                            STATUS_ENDPOINT.to_owned(),
                            ApiRequestErased::default(),
                            peer_id,
                        ),
                    )
                    .await;
                    let api_latency = match timed_status {
                        Ok(Ok(_)) => Some(start_time.elapsed().as_millis() as i32),
                        _ => None,
                    };
                    let block_height = match block_source {
                        Some(source) => tokio::time::timeout(
                            Duration::from_secs(10),
                            api.with_module(source.instance()).request_single_peer(
                                source.endpoint().to_owned(),
                                ApiRequestErased::default(),
                                peer_id,
                            ),
                        )
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .and_then(|json| source.block_height(json)),
                        None => None,
                    };

                    let software_version = tokio::time::timeout(
                        Duration::from_secs(10),
                        api.fedimintd_version(peer_id),
                    )
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .map(|version| version.trim().to_owned())
                    .filter(|version| !version.is_empty());

                    (peer_id, status, software_version, block_height, api_latency)
                }
            }))
            .await
        };

        // Since when no guardian has answered, None while any does
        let mut offline_since = self.health_offline_since(federation_id).await?;
        let mut poll_interval =
            health_poll_interval(offline_since.map(|since| chrono::Utc::now().naive_utc() - since));
        if poll_interval != ONLINE_POLL_INTERVAL {
            log_poll_interval(poll_interval, offline_since);
        }

        loop {
            let poll_started = tokio::time::Instant::now();

            let mut peer_status_responses = poll_guardians().await;
            // While backed off, a miss can be a connection that is still being
            // re-established after the long pause: try once more before
            // waiting again, and store only that answer
            if poll_interval != ONLINE_POLL_INTERVAL
                && !peer_status_responses
                    .iter()
                    .any(|(_, status, ..)| status.is_some())
            {
                tokio::time::sleep(BACKED_OFF_RETRY_DELAY).await;
                peer_status_responses = poll_guardians().await;
            }
            let any_answered = peer_status_responses
                .iter()
                .any(|(_, status, ..)| status.is_some());

            let timestamp = chrono::Utc::now().naive_utc();
            offline_since = if any_answered {
                None
            } else {
                offline_since.or(Some(timestamp))
            };
            let next_interval = health_poll_interval(offline_since.map(|since| timestamp - since));

            let mut conn = self.connection().await?;
            let dbtx = conn.transaction().await?;
            for (peer_id, status, software_version, block_height, api_latency) in
                peer_status_responses
            {
                dbtx.execute(
                    "INSERT INTO guardian_health (
                        federation_id,
                        time,
                        guardian_id,
                        status,
                        block_height,
                        latency_ms,
                        software_version,
                        poll_interval_s
                    ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                    &[
                        &federation_id.consensus_encode_to_vec(),
                        &timestamp,
                        &(peer_id.to_usize() as i32),
                        &status.map(|s| serde_json::to_value(s).expect("Can be serialized")),
                        // Heights beyond the column's range are nonsense, not wrapped
                        &block_height.and_then(|height| i32::try_from(height).ok()),
                        &api_latency,
                        &software_version,
                        // The sample stands for the time until the next poll
                        &(next_interval.as_secs() as i32),
                    ],
                )
                .await?;
            }
            dbtx.commit().await?;
            // The next poll can be a minute or more away. Return the connection
            // before sleeping so other federations can record their health.
            drop(conn);

            if next_interval != poll_interval {
                log_poll_interval(next_interval, offline_since);
                poll_interval = next_interval;
            }
            tokio::time::sleep_until(poll_started + poll_interval).await;
        }
    }

    /// When a guardian of the federation last answered, as the end of the
    /// last hour with an answer, or the first hour on record if none did. A
    /// restart then keeps the back-off of a federation that has been offline
    /// for months.
    async fn health_offline_since(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Option<NaiveDateTime>> {
        Ok(self
            .connection()
            .await?
            .query_one(
                "SELECT COALESCE(
                     MAX(hour) FILTER (WHERE online_samples > 0) + INTERVAL '1 hour',
                     MIN(hour)
                 )
                 FROM guardian_health_hourly
                 WHERE federation_id = $1",
                &[&federation_id.consensus_encode_to_vec()],
            )
            .await?
            .get(0))
    }

    pub async fn get_guardian_health(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<BTreeMap<PeerId, GuardianHealth>> {
        self.ensure_observed(federation_id).await?;

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
            "WITH RECURSIVE window_bounds AS (
                 -- Samples are stored in UTC
                 SELECT NOW() AT TIME ZONE 'UTC' - INTERVAL '30 days' AS since,
                        date_trunc('hour', NOW() AT TIME ZONE 'UTC' - INTERVAL '30 days') + INTERVAL '1 hour' AS rollup_from
             ),
             guardians AS (
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
                -- Reported by the guardian: clamped, so an absurd value
                -- can't fail the whole federation's health. NULL (no answer
                -- in the window) must stay NULL, and GREATEST/LEAST skip NULLs:
                -- unguarded, an offline guardian would read as online at session 0.
                CASE WHEN latest.status IS NOT NULL THEN
                    LEAST(GREATEST((latest.status -> 'federation' ->> 'session_count')::numeric, 0),
                          4294967295)::bigint
                END AS session_count,
                latest.software_version,
                last30d.uptime,
                last30d.latency_ms
             FROM guardians g
             -- The latest answer within the window, the same rule as
             -- `get_guardian_health_summary`
             LEFT JOIN LATERAL (
                 SELECT h.block_height, h.status, h.software_version
                 FROM guardian_health h
                 WHERE h.federation_id = $1
                   AND h.guardian_id = g.guardian_id
                   AND h.time >= (now() AT TIME ZONE 'UTC') - INTERVAL '5 minutes'
                   AND h.status -> 'federation' ->> 'session_count' IS NOT NULL
                 ORDER BY h.time DESC
                 LIMIT 1
             ) latest ON true
             -- Whole hours of the window come from the hourly rollup, the
             -- partial first hour from the raw samples. Same result as
             -- aggregating all raw samples of the last 30 days. Uptime weights
             -- each sample by its poll interval, so the sparser samples of a
             -- federation that was offline (see `health_poll_interval`) don't
             -- raise it.
             INNER JOIN (
                 SELECT
                     guardian_id,
                     (SUM(online_s)::decimal / SUM(polled_s)::decimal * 100)::real AS uptime,
                     COALESCE((SUM(latency_sum_ms)::decimal / NULLIF(SUM(latency_samples), 0))::real, 0) AS latency_ms
                 FROM (
                     SELECT h.guardian_id, h.polled_s, h.online_s, h.latency_samples, h.latency_sum_ms
                     FROM guardian_health_hourly h, window_bounds b
                     WHERE h.federation_id = $1 AND h.hour >= b.rollup_from
                   UNION ALL
                     SELECT raw.guardian_id,
                            COALESCE(raw.poll_interval_s, 60),
                            CASE WHEN raw.status IS NOT NULL THEN COALESCE(raw.poll_interval_s, 60) ELSE 0 END,
                            (raw.latency_ms IS NOT NULL)::int,
                            COALESCE(raw.latency_ms, 0)
                     FROM guardian_health raw, window_bounds b
                     WHERE raw.federation_id = $1 AND raw.time > b.since AND raw.time < b.rollup_from
                 ) samples
                 GROUP BY guardian_id
             ) last30d ON g.guardian_id = last30d.guardian_id
             WHERE g.guardian_id IS NOT NULL",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        let our_block_height = self.last_fetched_block_height().await?;
        let max_session = health_rows
            .iter()
            .filter_map(|row| row.session_count)
            .max()
            .map_or(0, clamp_to_u32);

        Ok(health_rows
            .into_iter()
            .map(|row| {
                let latest = if let Some(session_count) = row.session_count {
                    let block_height = row
                        .block_height
                        .and_then(|height| u32::try_from(height).ok());
                    let session_count = clamp_to_u32(session_count);
                    Some(GuardianHealthLatest {
                        block_height,
                        block_outdated: block_height
                            .zip(our_block_height)
                            .is_some_and(|(height, ours)| ours.saturating_sub(height) > 6),
                        session_count,
                        session_outdated: max_session.saturating_sub(session_count) > 1,
                    })
                } else {
                    None
                };

                let health = GuardianHealth {
                    avg_uptime: row.uptime,
                    avg_latency: row.latency_ms,
                    software_version: row.software_version,
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
                COUNT(answered.time)::int as online_guardians
             FROM guardians g
             -- Online means answered within the window, not on the latest poll:
             -- one missed poll must not flip a federation at its threshold
             LEFT JOIN LATERAL (
                 SELECT h.time
                 FROM guardian_health h
                 WHERE h.federation_id = g.federation_id
                   AND h.guardian_id = g.guardian_id
                   AND h.time >= (now() AT TIME ZONE 'UTC') - INTERVAL '5 minutes'
                   AND h.status -> 'federation' ->> 'session_count' IS NOT NULL
                 ORDER BY h.time DESC
                 LIMIT 1
             ) answered ON true
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

                if federation.guardians == 1 {
                    return Ok((
                        federation_id,
                        if federation.online_guardians == 1 {
                            FederationHealth::Online
                        } else {
                            FederationHealth::Offline
                        },
                    ));
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

fn clamp_to_u32(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// `fedimint_walletv2_common::endpoint_constants::CONSENSUS_BLOCK_COUNT_ENDPOINT`
const WALLETV2_CONSENSUS_BLOCK_COUNT_ENDPOINT: &str = "consensus_block_count";
/// `fedimint_walletv2_server::CONFIRMATION_FINALITY_DELAY`: walletv2 guardians
/// vote for their bitcoind's block count minus this many blocks
const WALLETV2_CONFIRMATION_FINALITY_DELAY: u64 = 6;

/// Where a guardian's block height comes from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockHeightSource {
    /// The v1 wallet module's block count of the guardian's own bitcoind
    WalletLocal(ModuleInstanceId),
    /// Wallet v2 has no endpoint for the guardian's own bitcoind, only for
    /// the block count consensus agreed on as the guardian has processed it,
    /// which trails the chain tip by the finality delay. Added back, it is
    /// comparable to the chain tip; a guardian behind on consensus or
    /// federation whose bitcoinds lag reports a lower height.
    WalletV2Consensus(ModuleInstanceId),
}

impl BlockHeightSource {
    fn of(config: &ClientConfig) -> Option<Self> {
        module_instance_id(config, "wallet")
            .map(Self::WalletLocal)
            .or_else(|| module_instance_id(config, "walletv2").map(Self::WalletV2Consensus))
    }

    fn instance(self) -> ModuleInstanceId {
        match self {
            Self::WalletLocal(instance) | Self::WalletV2Consensus(instance) => instance,
        }
    }

    fn endpoint(self) -> &'static str {
        match self {
            Self::WalletLocal(_) => BLOCK_COUNT_LOCAL_ENDPOINT,
            Self::WalletV2Consensus(_) => WALLETV2_CONSENSUS_BLOCK_COUNT_ENDPOINT,
        }
    }

    /// The 0-based height of the chain tip in the guardian's answer. Fedimint
    /// counts blocks, so the tip is one less; a guardian without blocks
    /// reports 0.
    fn block_height(self, response: serde_json::Value) -> Option<u32> {
        let block_count = match self {
            Self::WalletLocal(_) => {
                u64::from(serde_json::from_value::<Option<u32>>(response).ok()??)
            }
            Self::WalletV2Consensus(_) => {
                let count = serde_json::from_value::<u64>(response).ok()?;
                // Consensus hasn't agreed on any block yet
                if count == 0 {
                    return None;
                }
                count.saturating_add(WALLETV2_CONFIRMATION_FINALITY_DELAY)
            }
        };
        u32::try_from(block_count.saturating_sub(1)).ok()
    }
}

/// Polls of a federation that is online, or partly so
const ONLINE_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Retry of a poll that got no answer while backed off
const BACKED_OFF_RETRY_DELAY: Duration = Duration::from_secs(30);

/// How often to poll the health of a federation none of whose guardians has
/// answered for `offline_for`. A federation that has been offline for days is
/// still polled every 30 minutes, so a returning one is noticed within that.
fn health_poll_interval(offline_for: Option<TimeDelta>) -> Duration {
    match offline_for {
        Some(offline_for) if offline_for >= TimeDelta::days(7) => Duration::from_secs(30 * 60),
        Some(offline_for) if offline_for >= TimeDelta::days(1) => Duration::from_secs(10 * 60),
        _ => ONLINE_POLL_INTERVAL,
    }
}

fn log_poll_interval(interval: Duration, offline_since: Option<NaiveDateTime>) {
    match offline_since {
        Some(since) if interval != ONLINE_POLL_INTERVAL => info!(
            "No guardian has answered since {since} or earlier, polling health every {} minutes",
            interval.as_secs() / 60
        ),
        _ => info!("A guardian answered again, polling health every minute"),
    }
}

#[derive(FromRow)]
struct GuardianHealthRow {
    guardian_id: i32,
    block_height: Option<i32>,
    session_count: Option<i64>,
    software_version: Option<String>,
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

#[cfg(test)]
mod tests;
