use std::collections::HashMap;
use std::time::Duration;

use anyhow::{bail, Context};
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use fedimint_api_client::api::{DynGlobalApi, FederationApiExt};
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::encoding::Encodable;
use fedimint_core::module::ApiRequestErased;
use fedimint_ln_common::federation_endpoint_constants::LIST_GATEWAYS_ENDPOINT;
use fedimint_ln_common::LightningGatewayAnnouncement;
use fmo_api_types::{GatewayActivityMetrics, GatewayInfo, GatewayUptimeMetrics};
use futures::future::join_all;
use serde::Deserialize;
use tracing::{info, warn};

use crate::federation::observer::FederationObserver;
use crate::util::query;

const GATEWAY_POLL_INTERVAL_MINUTES: u64 = 5;
const GATEWAY_SNAPSHOT_RETENTION_DAYS: i64 = 90;
const GATEWAY_PRUNE_INTERVAL_HOURS: i64 = 6;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum GatewayMetricsWindow {
    H1,
    H24,
    D7,
    D30,
    D90,
}

impl GatewayMetricsWindow {
    fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value.unwrap_or("7d") {
            "1h" => Ok(Self::H1),
            "24h" => Ok(Self::H24),
            "7d" => Ok(Self::D7),
            "30d" => Ok(Self::D30),
            "90d" => Ok(Self::D90),
            invalid => bail!(
                "Invalid gateways window '{invalid}'. Supported values: 1h, 24h, 7d, 30d, 90d"
            ),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::H1 => "1h",
            Self::H24 => "24h",
            Self::D7 => "7d",
            Self::D30 => "30d",
            Self::D90 => "90d",
        }
    }

    fn duration(self) -> chrono::Duration {
        match self {
            Self::H1 => chrono::Duration::hours(1),
            Self::H24 => chrono::Duration::hours(24),
            Self::D7 => chrono::Duration::days(7),
            Self::D30 => chrono::Duration::days(30),
            Self::D90 => chrono::Duration::days(90),
        }
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GetFederationGatewaysParams {
    window: Option<String>,
}

pub(crate) async fn fetch_gateways_for_config(
    config: &ClientConfig,
) -> anyhow::Result<Vec<GatewayInfo>> {
    let api = DynGlobalApi::from_endpoints(
        config
            .global
            .api_endpoints
            .iter()
            .map(|(&peer_id, peer_url)| (peer_id, peer_url.url.clone())),
        &None,
    )
    .await?;

    let ln_instance_id = config
        .modules
        .iter()
        .find_map(|(&instance_id, module)| (module.kind.as_str() == "ln").then_some(instance_id))
        .context("No LN module found in federation config")?;

    let peer_ids: Vec<fedimint_core::PeerId> =
        config.global.api_endpoints.keys().copied().collect();
    let mut merged: HashMap<String, LightningGatewayAnnouncement> = HashMap::new();
    let peer_results = join_all(peer_ids.iter().copied().map(|peer_id| {
        let api = api.clone();
        async move {
            let result: anyhow::Result<Vec<LightningGatewayAnnouncement>> = api
                .with_module(ln_instance_id)
                .request_single_peer(
                    LIST_GATEWAYS_ENDPOINT.to_owned(),
                    ApiRequestErased::default(),
                    peer_id,
                )
                .await
                .map_err(anyhow::Error::from)
                .and_then(|v| serde_json::from_value(v).map_err(anyhow::Error::from));
            (peer_id, result)
        }
    }))
    .await;

    for (peer_id, result) in peer_results {
        match result {
            Ok(gateways) => {
                for gw in gateways {
                    merged.entry(gw.info.gateway_id.to_string()).or_insert(gw);
                }
            }
            Err(e) => {
                warn!(
                    "Failed to fetch live gateways from peer {}: {:?}",
                    peer_id, e
                );
            }
        }
    }

    merged
        .into_values()
        .map(|gw| {
            let raw = serde_json::to_value(&gw)?;
            Ok(GatewayInfo {
                gateway_id: gw.info.gateway_id.to_string(),
                node_pub_key: gw.info.node_pub_key.to_string(),
                lightning_alias: gw.info.lightning_alias,
                api_endpoint: gw.info.api.to_string(),
                vetted: gw.vetted,
                raw: Some(raw),
                first_seen: None,
                last_seen: None,
                activity_7d: None,
                activity_window: None,
                uptime_window: None,
                metrics_window: None,
            })
        })
        .collect()
}

impl FederationObserver {
    /// Background task: poll the LN module on this federation for currently
    /// registered gateways and persist them. Runs in a loop until cancelled.
    pub async fn monitor_gateways(
        &self,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        const POLL_INTERVAL: Duration = Duration::from_secs(GATEWAY_POLL_INTERVAL_MINUTES * 60);

        let api = DynGlobalApi::from_endpoints(
            config
                .global
                .api_endpoints
                .iter()
                .map(|(&peer_id, peer_url)| (peer_id, peer_url.url.clone())),
            &None,
        )
        .await?;

        let ln_instance_id = config
            .modules
            .iter()
            .find_map(|(&instance_id, module)| {
                (module.kind.as_str() == "ln").then_some(instance_id)
            })
            .context("No LN module found in federation config")?;

        let peer_ids: Vec<fedimint_core::PeerId> =
            config.global.api_endpoints.keys().copied().collect();

        let mut interval = tokio::time::interval(POLL_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(e) =
                Self::fetch_and_store_gateways(self, federation_id, &api, ln_instance_id, &peer_ids)
                    .await
            {
                warn!(
                    "Failed to fetch gateways for federation {}: {:?}",
                    federation_id, e
                );
            }
        }
    }

    async fn fetch_and_store_gateways(
        &self,
        federation_id: FederationId,
        api: &DynGlobalApi,
        ln_instance_id: ModuleInstanceId,
        peer_ids: &[fedimint_core::PeerId],
    ) -> anyhow::Result<()> {
        // Query all peers and merge by gateway_id — each guardian has their own
        // registry so we take the union across all peers.
        let mut merged: HashMap<String, LightningGatewayAnnouncement> = HashMap::new();
        let mut successful_peer_queries: u32 = 0;
        let peer_results = join_all(peer_ids.iter().copied().map(|peer_id| async move {
            let result: anyhow::Result<Vec<LightningGatewayAnnouncement>> = api
                .with_module(ln_instance_id)
                .request_single_peer(
                    LIST_GATEWAYS_ENDPOINT.to_owned(),
                    ApiRequestErased::default(),
                    peer_id,
                )
                .await
                .map_err(anyhow::Error::from)
                .and_then(|v| serde_json::from_value(v).map_err(anyhow::Error::from));
            (peer_id, result)
        }))
        .await;

        for (peer_id, result) in peer_results {
            match result {
                Ok(gateways) => {
                    successful_peer_queries += 1;
                    for gw in gateways {
                        merged.entry(gw.info.gateway_id.to_string()).or_insert(gw);
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to fetch gateways from peer {} for {}: {:?}",
                        peer_id, federation_id, e
                    );
                }
            }
        }

        if successful_peer_queries == 0 {
            bail!(
                "No successful gateway registry responses from any federation peer for {}",
                federation_id
            );
        }

        let mut conn = self.connection().await?;
        let dbtx = conn.transaction().await?;
        let now = chrono::Utc::now();
        let federation_id_bytes = federation_id.consensus_encode_to_vec();

        let mut gateway_ids = Vec::with_capacity(merged.len());
        let mut node_pub_keys = Vec::with_capacity(merged.len());
        let mut api_endpoints = Vec::with_capacity(merged.len());
        let mut lightning_aliases = Vec::with_capacity(merged.len());
        let mut vetted_flags = Vec::with_capacity(merged.len());
        let mut raw_announcements = Vec::with_capacity(merged.len());

        for (gateway_id, gw) in &merged {
            gateway_ids.push(gateway_id.clone());
            node_pub_keys.push(gw.info.node_pub_key.to_string());
            api_endpoints.push(gw.info.api.to_string());
            lightning_aliases.push(gw.info.lightning_alias.clone());
            vetted_flags.push(gw.vetted);
            raw_announcements.push(serde_json::to_string(gw)?);
        }

        if !gateway_ids.is_empty() {
            dbtx.execute(
                "INSERT INTO gateways
                     (federation_id, gateway_id, node_pub_key, api_endpoint,
                      lightning_alias, vetted, raw, first_seen, last_seen)
                 SELECT
                     $1,
                     gw.gateway_id,
                     gw.node_pub_key,
                     gw.api_endpoint,
                     gw.lightning_alias,
                     gw.vetted,
                     gw.raw_json::jsonb,
                     $2,
                     $2
                 FROM UNNEST(
                     $3::text[],
                     $4::text[],
                     $5::text[],
                     $6::text[],
                     $7::boolean[],
                     $8::text[]
                 ) AS gw(gateway_id, node_pub_key, api_endpoint, lightning_alias, vetted, raw_json)
                 ON CONFLICT (federation_id, gateway_id) DO UPDATE
                     SET node_pub_key    = EXCLUDED.node_pub_key,
                         api_endpoint    = EXCLUDED.api_endpoint,
                         lightning_alias = EXCLUDED.lightning_alias,
                         vetted          = EXCLUDED.vetted,
                         raw             = EXCLUDED.raw,
                         last_seen       = EXCLUDED.last_seen",
                &[
                    &federation_id_bytes,
                    &now,
                    &gateway_ids,
                    &node_pub_keys,
                    &api_endpoints,
                    &lightning_aliases,
                    &vetted_flags,
                    &raw_announcements,
                ],
            )
            .await?;
        }

        dbtx.execute(
            "WITH current_gateway_ids AS (
                 SELECT UNNEST($3::text[]) AS gateway_id
             ),
             all_gateway_ids AS (
                 SELECT gateway_id, TRUE AS is_seen
                 FROM current_gateway_ids
                 UNION
                 SELECT g.gateway_id, FALSE AS is_seen
                 FROM gateways g
                 WHERE g.federation_id = $1
                   AND NOT EXISTS (
                       SELECT 1
                       FROM current_gateway_ids c
                       WHERE c.gateway_id = g.gateway_id
                   )
             )
             INSERT INTO gateway_poll_snapshots
                 (federation_id, gateway_id, poll_time, is_seen)
             SELECT
                 $1,
                 a.gateway_id,
                 $2,
                 a.is_seen
             FROM all_gateway_ids a
             ON CONFLICT DO NOTHING",
            &[&federation_id_bytes, &now, &gateway_ids],
        )
        .await?;

        let prune_interval_secs = GATEWAY_PRUNE_INTERVAL_HOURS * 60 * 60;
        let should_prune = now.timestamp().rem_euclid(prune_interval_secs)
            < (GATEWAY_POLL_INTERVAL_MINUTES as i64 * 60);
        let deleted_snapshots = if should_prune {
            let retention_cutoff = now - chrono::Duration::days(GATEWAY_SNAPSHOT_RETENTION_DAYS);
            dbtx.execute(
                "DELETE FROM gateway_poll_snapshots
                 WHERE federation_id = $1
                   AND poll_time < $2",
                &[&federation_id_bytes, &retention_cutoff],
            )
            .await?
        } else {
            0
        };
        dbtx.commit().await?;

        info!(
            "Stored {} seen gateway(s), persisted poll snapshots for federation {}, deleted {} old snapshots",
            merged.len(), federation_id, deleted_snapshots
        );
        Ok(())
    }

    async fn list_federation_gateways(
        &self,
        federation_id: FederationId,
        window: GatewayMetricsWindow,
    ) -> anyhow::Result<Vec<GatewayInfo>> {
        #[derive(postgres_from_row::FromRow)]
        struct GatewayRow {
            gateway_id: String,
            node_pub_key: String,
            lightning_alias: String,
            api_endpoint: String,
            vetted: bool,
            raw: serde_json::Value,
            first_seen: chrono::DateTime<chrono::Utc>,
            last_seen: chrono::DateTime<chrono::Utc>,
        }

        #[derive(postgres_from_row::FromRow)]
        struct GatewayActivityRow {
            gateway_key: String,
            fund_count: i64,
            settle_count: i64,
            cancel_count: i64,
            total_volume_msat: i64,
        }

        #[derive(postgres_from_row::FromRow)]
        struct GatewayUptimeRow {
            gateway_id: String,
            seen_samples: i64,
            total_samples: i64,
        }

        let conn = self.connection().await?;
        let federation_id_bytes = federation_id.consensus_encode_to_vec();
        let window_start_utc: DateTime<Utc> = Utc::now() - window.duration();
        let window_start_naive = window_start_utc.naive_utc();
        let metrics_window = window.label().to_owned();

        let rows = query::<GatewayRow>(
            &conn,
            "SELECT gateway_id, node_pub_key, lightning_alias, api_endpoint, vetted, raw, first_seen, last_seen
             FROM gateways
             WHERE federation_id = $1
             ORDER BY last_seen DESC",
            &[&federation_id_bytes],
        )
        .await?;

        let activity_rows = query::<GatewayActivityRow>(
            &conn,
            "WITH tx_window AS (
                     SELECT t.federation_id, t.txid
                     FROM transactions t
                     JOIN session_times st
                         ON st.federation_id = t.federation_id
                        AND st.session_index = t.session_index
                     WHERE t.federation_id = $1
                       AND st.estimated_session_timestamp >= $2
                 ),
                 window_ln_outputs AS (
                     SELECT
                         o.federation_id,
                         o.txid,
                         o.out_index,
                         o.ln_contract_id,
                         o.ln_contract_interaction_kind,
                         COALESCE(o.amount_msat, 0)::bigint AS amount_msat
                     FROM transaction_outputs o
                     JOIN tx_window tw
                         ON tw.federation_id = o.federation_id
                        AND tw.txid = o.txid
                     WHERE o.federation_id = $1
                       AND o.kind = 'ln'
                       AND o.ln_contract_id IS NOT NULL
                 ),
                 window_ln_inputs AS (
                     SELECT
                         i.federation_id,
                         i.txid,
                         i.ln_contract_id
                     FROM transaction_inputs i
                     JOIN tx_window tw
                         ON tw.federation_id = i.federation_id
                        AND tw.txid = i.txid
                     WHERE i.federation_id = $1
                       AND i.kind = 'ln'
                       AND i.ln_contract_id IS NOT NULL
                 ),
                 contract_map AS (
                     SELECT DISTINCT ON (wlo.federation_id, wlo.ln_contract_id)
                         wlo.federation_id,
                         wlo.ln_contract_id,
                         COALESCE(
                             d.details #>> '{V0,Contract,contract,Outgoing,gateway_key}',
                             d.details #>> '{V0,Contract,contract,Incoming,gateway_key}'
                         ) AS gateway_key
                     FROM window_ln_outputs wlo
                     JOIN transaction_output_details d
                         ON d.federation_id = wlo.federation_id
                        AND d.txid = wlo.txid
                        AND d.out_index = wlo.out_index
                     WHERE wlo.ln_contract_interaction_kind = 'fund'
                     ORDER BY wlo.federation_id, wlo.ln_contract_id, wlo.txid, wlo.out_index
                 ),
                 events AS (
                     SELECT
                         cm.gateway_key,
                         1::bigint AS fund_count,
                         0::bigint AS settle_count,
                         0::bigint AS cancel_count,
                         wlo.amount_msat AS volume_msat
                     FROM window_ln_outputs wlo
                     JOIN contract_map cm
                         ON cm.federation_id = wlo.federation_id
                        AND cm.ln_contract_id = wlo.ln_contract_id
                     WHERE wlo.ln_contract_interaction_kind = 'fund'
                     UNION ALL
                     SELECT
                         cm.gateway_key,
                         0::bigint,
                         1::bigint,
                         0::bigint,
                         0::bigint
                     FROM window_ln_inputs wli
                     JOIN contract_map cm
                         ON cm.federation_id = wli.federation_id
                        AND cm.ln_contract_id = wli.ln_contract_id
                     UNION ALL
                     SELECT
                         cm.gateway_key,
                         0::bigint,
                         0::bigint,
                         1::bigint,
                         0::bigint
                     FROM window_ln_outputs wlo
                     JOIN contract_map cm
                         ON cm.federation_id = wlo.federation_id
                        AND cm.ln_contract_id = wlo.ln_contract_id
                     WHERE wlo.ln_contract_interaction_kind = 'cancel'
                 )
                 SELECT
                     gateway_key,
                     SUM(fund_count)::bigint AS fund_count,
                     SUM(settle_count)::bigint AS settle_count,
                     SUM(cancel_count)::bigint AS cancel_count,
                     SUM(volume_msat)::bigint AS total_volume_msat
                 FROM events
                 WHERE gateway_key IS NOT NULL
                 GROUP BY gateway_key",
            &[&federation_id_bytes, &window_start_naive],
        )
        .await?;

        let activity_by_gateway_key: HashMap<String, GatewayActivityMetrics> = activity_rows
            .into_iter()
            .map(|row| {
                (
                    row.gateway_key,
                    GatewayActivityMetrics {
                        fund_count: row.fund_count.max(0) as u64,
                        settle_count: row.settle_count.max(0) as u64,
                        cancel_count: row.cancel_count.max(0) as u64,
                        total_volume_msat: row.total_volume_msat.max(0) as u64,
                    },
                )
            })
            .collect();

        let uptime_rows = query::<GatewayUptimeRow>(
            &conn,
            "SELECT
                 gateway_id,
                 COUNT(*) FILTER (WHERE is_seen)::bigint AS seen_samples,
                 COUNT(*)::bigint AS total_samples
             FROM gateway_poll_snapshots
             WHERE federation_id = $1
               AND poll_time >= $2
             GROUP BY gateway_id",
            &[&federation_id_bytes, &window_start_utc],
        )
        .await?;

        let uptime_by_gateway_id: HashMap<String, GatewayUptimeMetrics> = uptime_rows
            .into_iter()
            .map(|row| {
                let seen_samples = row.seen_samples.max(0) as u64;
                let total_samples = row.total_samples.max(0) as u64;
                let online_minutes = seen_samples.saturating_mul(GATEWAY_POLL_INTERVAL_MINUTES);
                let offline_minutes = total_samples
                    .saturating_sub(seen_samples)
                    .saturating_mul(GATEWAY_POLL_INTERVAL_MINUTES);
                let uptime_pct = if total_samples > 0 {
                    (seen_samples as f64 / total_samples as f64) * 100.0
                } else {
                    0.0
                };
                (
                    row.gateway_id,
                    GatewayUptimeMetrics {
                        sample_count: total_samples,
                        seen_samples,
                        online_minutes,
                        offline_minutes,
                        uptime_pct,
                    },
                )
            })
            .collect();

        Ok(rows
            .into_iter()
            .map(|r| {
                let activity_window = r
                    .raw
                    .pointer("/info/gateway_redeem_key")
                    .and_then(|v| v.as_str())
                    .and_then(|gateway_key| activity_by_gateway_key.get(gateway_key).cloned());
                let uptime_window = uptime_by_gateway_id.get(&r.gateway_id).cloned();

                GatewayInfo {
                    activity_7d: if window == GatewayMetricsWindow::D7 {
                        activity_window.clone()
                    } else {
                        None
                    },
                    activity_window,
                    uptime_window,
                    metrics_window: Some(metrics_window.clone()),
                    gateway_id: r.gateway_id,
                    node_pub_key: r.node_pub_key,
                    lightning_alias: r.lightning_alias,
                    api_endpoint: r.api_endpoint,
                    vetted: r.vetted,
                    raw: Some(r.raw),
                    first_seen: Some(r.first_seen),
                    last_seen: Some(r.last_seen),
                }
            })
            .collect())
    }
}

pub(super) async fn get_federation_gateways(
    Path(federation_id): Path<FederationId>,
    Query(params): Query<GetFederationGatewaysParams>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<Vec<GatewayInfo>>> {
    let window = GatewayMetricsWindow::parse(params.window.as_deref())?;
    Ok(state
        .federation_observer
        .list_federation_gateways(federation_id, window)
        .await?
        .into())
}
