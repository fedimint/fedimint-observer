use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::bail;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use deadpool_postgres::Transaction;
use fedimint_api_client::api::{DynGlobalApi, FederationApiExt};
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::{ModuleInstanceId, ModuleKind};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::ApiRequestErased;
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;
use fedimint_ln_common::client::GatewayApi;
use fedimint_ln_common::federation_endpoint_constants::LIST_GATEWAYS_ENDPOINT;
use fedimint_ln_common::LightningGatewayAnnouncement;
use fedimint_lnv2_common::endpoint_constants::GATEWAYS_ENDPOINT;
use fedimint_lnv2_common::gateway_api::{
    GatewayConnection, PaymentFee, RealGatewayConnection, RoutingInfo,
};
use fmo_api_types::{
    GatewayActivityMetrics, GatewayFee, GatewayInfo, GatewayProtocol, GatewayUptimeMetrics,
    GatewayUptimeTrendPoint, Lnv2GatewayInfo, Lnv2RoutingStatus,
};
use futures::future::join_all;
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::federation::observer::FederationObserver;
use crate::util::query;

const GATEWAY_POLL_INTERVAL_MINUTES: u64 = 5;
const GATEWAY_SNAPSHOT_RETENTION_DAYS: i64 = 90;
const GATEWAY_PRUNE_INTERVAL_HOURS: i64 = 6;
const LNV2_PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const LNV2_PROBE_CONCURRENCY: usize = 8;

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

/// Instance ids of the Lightning modules a federation runs. Either, both or
/// neither may be present.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LnInstances {
    lnv1: Option<ModuleInstanceId>,
    lnv2: Option<ModuleInstanceId>,
}

impl LnInstances {
    fn from_config(config: &ClientConfig) -> Self {
        Self::from_kinds(
            config
                .modules
                .iter()
                .map(|(&instance_id, module)| (instance_id, &module.kind)),
        )
    }

    fn from_kinds<'a>(
        modules: impl IntoIterator<Item = (ModuleInstanceId, &'a ModuleKind)>,
    ) -> Self {
        let mut instances = Self::default();
        for (instance_id, kind) in modules {
            if *kind == fedimint_ln_common::KIND {
                instances.lnv1.get_or_insert(instance_id);
            } else if *kind == fedimint_lnv2_common::KIND {
                instances.lnv2.get_or_insert(instance_id);
            }
        }
        instances
    }

    fn is_empty(&self) -> bool {
        self.lnv1.is_none() && self.lnv2.is_none()
    }
}

/// An LNv2 registry entry together with what the gateway itself told us.
#[derive(Debug, Clone)]
struct Lnv2Gateway {
    /// Normalized API URL, see [`normalize_gateway_url`]
    gateway_id: String,
    /// API URL exactly as listed in the registry
    api_endpoint: SafeUrl,
    probe: Lnv2Probe,
}

#[derive(Debug, Clone)]
enum Lnv2Probe {
    Ok(Box<RoutingInfo>),
    NotServing,
    Unreachable,
}

impl Lnv2Probe {
    fn status(&self) -> Lnv2RoutingStatus {
        match self {
            Self::Ok(_) => Lnv2RoutingStatus::Ok,
            Self::NotServing => Lnv2RoutingStatus::NotServing,
            Self::Unreachable => Lnv2RoutingStatus::Unreachable,
        }
    }

    fn routing_info(&self) -> Option<&RoutingInfo> {
        match self {
            Self::Ok(info) => Some(info),
            Self::NotServing | Self::Unreachable => None,
        }
    }
}

/// Identity key for a gateway API URL.
///
/// Gateways serve their API both at the root and under `/v1`, and guardians
/// enter LNv2 registry URLs by hand, so a trailing slash or `/v1` must not make
/// the same gateway look like two. Scheme and host are already lowercased by
/// URL parsing.
pub(crate) fn normalize_gateway_url(url: &str) -> String {
    let url = url.trim_end_matches('/');
    url.strip_suffix("/v1")
        .unwrap_or(url)
        .trim_end_matches('/')
        .to_owned()
}

fn routing_status_str(status: Lnv2RoutingStatus) -> &'static str {
    match status {
        Lnv2RoutingStatus::Ok => "ok",
        Lnv2RoutingStatus::NotServing => "not_serving",
        Lnv2RoutingStatus::Unreachable => "unreachable",
    }
}

fn parse_routing_status(status: &str) -> Option<Lnv2RoutingStatus> {
    match status {
        "ok" => Some(Lnv2RoutingStatus::Ok),
        "not_serving" => Some(Lnv2RoutingStatus::NotServing),
        "unreachable" => Some(Lnv2RoutingStatus::Unreachable),
        _ => None,
    }
}

fn gateway_fee(fee: &PaymentFee) -> GatewayFee {
    GatewayFee {
        base_msat: fee.base.msats,
        parts_per_million: fee.parts_per_million,
    }
}

/// Query one module endpoint on every peer and concatenate the answers. Each
/// guardian keeps its own gateway registry, so the union is what matters.
/// Returns `None` if no peer answered, i.e. registry contents are unknown.
async fn fetch_registry<T: DeserializeOwned>(
    api: &DynGlobalApi,
    instance_id: ModuleInstanceId,
    endpoint: &str,
    peer_ids: &[PeerId],
) -> Option<Vec<T>> {
    let peer_results = join_all(peer_ids.iter().copied().map(|peer_id| async move {
        let result: anyhow::Result<Vec<T>> = api
            .with_module(instance_id)
            .request_single_peer(endpoint.to_owned(), ApiRequestErased::default(), peer_id)
            .await
            .map_err(anyhow::Error::from);
        (peer_id, result)
    }))
    .await;

    let mut any_success = false;
    let mut entries = Vec::new();
    for (peer_id, result) in peer_results {
        match result {
            Ok(peer_entries) => {
                any_success = true;
                entries.extend(peer_entries);
            }
            Err(e) => {
                warn!("Failed to fetch {endpoint} from peer {peer_id}: {e:?}");
            }
        }
    }
    any_success.then_some(entries)
}

async fn fetch_lnv1_registry(
    api: &DynGlobalApi,
    instance_id: ModuleInstanceId,
    peer_ids: &[PeerId],
) -> Option<Vec<LightningGatewayAnnouncement>> {
    let announcements: Vec<LightningGatewayAnnouncement> =
        fetch_registry(api, instance_id, LIST_GATEWAYS_ENDPOINT, peer_ids).await?;

    let mut merged: HashMap<String, LightningGatewayAnnouncement> = HashMap::new();
    for gw in announcements {
        merged.entry(gw.info.gateway_id.to_string()).or_insert(gw);
    }
    Some(merged.into_values().collect())
}

async fn fetch_lnv2_registry(
    api: &DynGlobalApi,
    instance_id: ModuleInstanceId,
    peer_ids: &[PeerId],
) -> Option<Vec<SafeUrl>> {
    let urls: Vec<SafeUrl> = fetch_registry(api, instance_id, GATEWAYS_ENDPOINT, peer_ids).await?;

    let mut merged: BTreeMap<String, SafeUrl> = BTreeMap::new();
    for url in urls {
        merged
            .entry(normalize_gateway_url(&url.to_string()))
            .or_insert(url);
    }
    Some(merged.into_values().collect())
}

/// Ask each LNv2 gateway for its routing info for this federation. This is
/// the only way to learn an LNv2 gateway's keys, since the registry lists
/// URLs only.
async fn probe_lnv2_gateways(
    gateway_conn: &RealGatewayConnection,
    federation_id: FederationId,
    urls: Vec<SafeUrl>,
) -> Vec<Lnv2Gateway> {
    futures::stream::iter(urls)
        .map(|url| async move {
            let probe = match tokio::time::timeout(
                LNV2_PROBE_TIMEOUT,
                gateway_conn.routing_info(url.clone(), &federation_id),
            )
            .await
            {
                Ok(Ok(Some(routing_info))) => Lnv2Probe::Ok(Box::new(routing_info)),
                Ok(Ok(None)) => Lnv2Probe::NotServing,
                Ok(Err(e)) => {
                    debug!("LNv2 gateway {url} routing info request failed: {e:?}");
                    Lnv2Probe::Unreachable
                }
                Err(_) => {
                    debug!("LNv2 gateway {url} routing info request timed out");
                    Lnv2Probe::Unreachable
                }
            };
            Lnv2Gateway {
                gateway_id: normalize_gateway_url(&url.to_string()),
                api_endpoint: url,
                probe,
            }
        })
        .buffer_unordered(LNV2_PROBE_CONCURRENCY)
        .collect()
        .await
}

fn lnv1_gateway_info(gw: LightningGatewayAnnouncement) -> anyhow::Result<GatewayInfo> {
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
        protocols: None,
        lnv2: None,
    })
}

fn lnv2_raw(gw: &Lnv2Gateway) -> serde_json::Value {
    serde_json::json!({
        "api_endpoint": gw.api_endpoint.to_string(),
        "routing_info": gw.probe.routing_info(),
    })
}

/// Builds the API view of an LNv2-only registration. `gateway_id` is the
/// normalized URL because LNv2 has no other stable identifier; LNv2 has no
/// notion of vetting, so `vetted` is always false.
#[allow(clippy::too_many_arguments)]
fn lnv2_gateway_info(
    gateway_id: String,
    api_endpoint: String,
    routing_status: Lnv2RoutingStatus,
    routing_checked_at: Option<DateTime<Utc>>,
    lightning_public_key: Option<String>,
    lightning_alias: Option<String>,
    module_public_key: Option<String>,
    routing_info: Option<&RoutingInfo>,
    raw: serde_json::Value,
) -> GatewayInfo {
    GatewayInfo {
        gateway_id,
        node_pub_key: lightning_public_key.clone().unwrap_or_default(),
        lightning_alias: lightning_alias.unwrap_or_default(),
        api_endpoint: api_endpoint.clone(),
        vetted: false,
        raw: Some(raw),
        first_seen: None,
        last_seen: None,
        activity_7d: None,
        activity_window: None,
        uptime_window: None,
        metrics_window: None,
        protocols: Some(vec![GatewayProtocol::Lnv2]),
        lnv2: Some(Lnv2GatewayInfo {
            api_endpoint,
            routing_status,
            routing_checked_at,
            lightning_public_key,
            module_public_key,
            send_fee_minimum: routing_info.map(|info| gateway_fee(&info.send_fee_minimum)),
            send_fee_default: routing_info.map(|info| gateway_fee(&info.send_fee_default)),
            receive_fee: routing_info.map(|info| gateway_fee(&info.receive_fee)),
            uptime_window: None,
        }),
    }
}

fn lnv2_gateway_info_from_probe(gw: &Lnv2Gateway, checked_at: DateTime<Utc>) -> GatewayInfo {
    let routing_info = gw.probe.routing_info();
    lnv2_gateway_info(
        gw.gateway_id.clone(),
        gw.api_endpoint.to_string(),
        gw.probe.status(),
        Some(checked_at),
        routing_info.map(|info| info.lightning_public_key.to_string()),
        routing_info.and_then(|info| info.lightning_alias.clone()),
        routing_info.map(|info| info.module_public_key.to_string()),
        routing_info,
        lnv2_raw(gw),
    )
}

/// Merges LNv2 registrations into LNv1 gateways where identity is verified:
/// the LNv1 node key must equal the LNv2 lightning key **and** the normalized
/// API URLs must match. Anything else, including an ambiguous match against
/// more than one LNv1 gateway, stays a separate row. LNv1 rows must already
/// carry `protocols: Some([Lnv1])`.
pub(crate) fn merge_protocols(
    mut lnv1: Vec<GatewayInfo>,
    lnv2: Vec<GatewayInfo>,
) -> Vec<GatewayInfo> {
    let mut unmerged = Vec::new();

    for v2 in lnv2 {
        let candidates: Vec<usize> = match &v2.lnv2 {
            Some(Lnv2GatewayInfo {
                lightning_public_key: Some(lightning_key),
                api_endpoint,
                ..
            }) => {
                let v2_url = normalize_gateway_url(api_endpoint);
                lnv1.iter()
                    .enumerate()
                    .filter(|(_, v1)| {
                        v1.lnv2.is_none()
                            && v1.node_pub_key == *lightning_key
                            && normalize_gateway_url(&v1.api_endpoint) == v2_url
                    })
                    .map(|(idx, _)| idx)
                    .collect()
            }
            _ => vec![],
        };

        if let [idx] = candidates[..] {
            let v1 = &mut lnv1[idx];
            v1.protocols = Some(vec![GatewayProtocol::Lnv1, GatewayProtocol::Lnv2]);
            v1.lnv2 = v2.lnv2;
        } else {
            unmerged.push(v2);
        }
    }

    lnv1.extend(unmerged);
    lnv1
}

/// Live registry lookup used by the stable `/config/:invite/gateways` API.
///
/// With `include_lnv2 == false` the output is exactly what this API returned
/// before LNv2 support: LNv1 gateways only, without the `protocols`/`lnv2`
/// fields. A federation without a Lightning module yields an empty list.
pub(crate) async fn fetch_gateways_for_config(
    config: &ClientConfig,
    include_lnv2: bool,
) -> anyhow::Result<Vec<GatewayInfo>> {
    let instances = LnInstances::from_config(config);
    if instances.is_empty() {
        return Ok(vec![]);
    }

    let connectors = fedimint_connectors::ConnectorRegistry::build_from_client_env()?
        .bind()
        .await?;
    let peers = config
        .global
        .api_endpoints
        .iter()
        .map(|(&peer_id, peer_url)| (peer_id, peer_url.url.clone()))
        .collect();
    let api = DynGlobalApi::new(connectors.clone(), peers, None)?;
    let peer_ids: Vec<PeerId> = config.global.api_endpoints.keys().copied().collect();

    let mut lnv1 = match instances.lnv1 {
        Some(instance_id) => fetch_lnv1_registry(&api, instance_id, &peer_ids)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(lnv1_gateway_info)
            .collect::<anyhow::Result<Vec<_>>>()?,
        None => vec![],
    };

    let Some(lnv2_instance_id) = instances.lnv2.filter(|_| include_lnv2) else {
        return Ok(lnv1);
    };

    for gw in &mut lnv1 {
        gw.protocols = Some(vec![GatewayProtocol::Lnv1]);
    }

    let lnv2_urls = fetch_lnv2_registry(&api, lnv2_instance_id, &peer_ids)
        .await
        .unwrap_or_default();
    let gateway_conn = RealGatewayConnection {
        api: GatewayApi::new(None, connectors),
    };
    let now = Utc::now();
    let lnv2 = probe_lnv2_gateways(
        &gateway_conn,
        config.global.calculate_federation_id(),
        lnv2_urls,
    )
    .await
    .iter()
    .map(|gw| lnv2_gateway_info_from_probe(gw, now))
    .collect();

    Ok(merge_protocols(lnv1, lnv2))
}

impl FederationObserver {
    /// Background task: poll the federation's LNv1 and LNv2 gateway registries
    /// and persist what they contain. Runs in a loop until cancelled.
    pub async fn monitor_gateways(
        &self,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        const POLL_INTERVAL: Duration = Duration::from_secs(GATEWAY_POLL_INTERVAL_MINUTES * 60);

        let instances = LnInstances::from_config(&config);
        if instances.is_empty() {
            info!("Federation {federation_id} has no Lightning module, not monitoring gateways");
            // Returning would make the supervisor restart this task every 30s
            return futures::future::pending().await;
        }

        let peers = config
            .global
            .api_endpoints
            .iter()
            .map(|(&peer_id, peer_url)| (peer_id, peer_url.url.clone()))
            .collect();
        let api = DynGlobalApi::new(self.connectors().clone(), peers, None)?;
        // Kept across polls so connections to gateways are reused
        let gateway_conn = RealGatewayConnection {
            api: GatewayApi::new(None, self.connectors().clone()),
        };

        let peer_ids: Vec<PeerId> = config.global.api_endpoints.keys().copied().collect();

        let mut interval = tokio::time::interval(POLL_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(e) = self
                .fetch_and_store_gateways(federation_id, &api, &gateway_conn, instances, &peer_ids)
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
        gateway_conn: &RealGatewayConnection,
        instances: LnInstances,
        peer_ids: &[PeerId],
    ) -> anyhow::Result<()> {
        // `None` for a protocol means its registry state is unknown this poll (module
        // absent or no peer answered), so no snapshot is written for it.
        let lnv1 = match instances.lnv1 {
            Some(instance_id) => {
                let registry = fetch_lnv1_registry(api, instance_id, peer_ids).await;
                if registry.is_none() {
                    warn!("No peer answered the LNv1 gateway registry query for {federation_id}");
                }
                registry
            }
            None => None,
        };
        let lnv2 = match instances.lnv2 {
            Some(instance_id) => match fetch_lnv2_registry(api, instance_id, peer_ids).await {
                Some(urls) => Some(probe_lnv2_gateways(gateway_conn, federation_id, urls).await),
                None => {
                    warn!("No peer answered the LNv2 gateway registry query for {federation_id}");
                    None
                }
            },
            None => None,
        };

        if lnv1.is_none() && lnv2.is_none() {
            bail!(
                "No successful gateway registry responses from any federation peer for {}",
                federation_id
            );
        }

        let mut conn = self.connection().await?;
        let dbtx = conn.transaction().await?;
        let now = chrono::Utc::now();
        let federation_id_bytes = federation_id.consensus_encode_to_vec();

        if let Some(lnv1) = &lnv1 {
            store_lnv1_gateways(&dbtx, &federation_id_bytes, now, lnv1).await?;
        }
        if let Some(lnv2) = &lnv2 {
            store_lnv2_gateways(&dbtx, &federation_id_bytes, now, lnv2).await?;
        }

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
            "Stored {} LNv1 and {} LNv2 gateway(s), persisted poll snapshots for federation {}, deleted {} old snapshots",
            lnv1.as_ref().map_or(0, Vec::len),
            lnv2.as_ref().map_or(0, Vec::len),
            federation_id,
            deleted_snapshots
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
            protocol: String,
            gateway_id: String,
            node_pub_key: Option<String>,
            lightning_alias: Option<String>,
            api_endpoint: String,
            vetted: bool,
            raw: serde_json::Value,
            first_seen: chrono::DateTime<chrono::Utc>,
            last_seen: chrono::DateTime<chrono::Utc>,
            module_public_key: Option<String>,
            routing_status: Option<String>,
            routing_checked_at: Option<chrono::DateTime<chrono::Utc>>,
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
            protocol: String,
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
            "SELECT protocol, gateway_id, node_pub_key, lightning_alias, api_endpoint, vetted, raw,
                    first_seen, last_seen, module_public_key, routing_status, routing_checked_at
             FROM gateways
             WHERE federation_id = $1",
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
                 protocol,
                 gateway_id,
                 COUNT(*) FILTER (WHERE is_seen)::bigint AS seen_samples,
                 COUNT(*)::bigint AS total_samples
             FROM gateway_poll_snapshots
             WHERE federation_id = $1
               AND poll_time >= $2
             GROUP BY protocol, gateway_id",
            &[&federation_id_bytes, &window_start_utc],
        )
        .await?;

        let uptime_by_gateway_id: HashMap<(String, String), GatewayUptimeMetrics> = uptime_rows
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
                    (row.protocol, row.gateway_id),
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

        let mut lnv1 = Vec::new();
        let mut lnv2 = Vec::new();
        for r in rows {
            let uptime_window = uptime_by_gateway_id
                .get(&(r.protocol.clone(), r.gateway_id.clone()))
                .cloned();

            match parse_protocol(&r.protocol) {
                Some(GatewayProtocol::Lnv1) => {
                    let activity_window = r
                        .raw
                        .pointer("/info/gateway_redeem_key")
                        .and_then(|v| v.as_str())
                        .and_then(|gateway_key| activity_by_gateway_key.get(gateway_key).cloned());

                    lnv1.push(GatewayInfo {
                        activity_7d: if window == GatewayMetricsWindow::D7 {
                            activity_window.clone()
                        } else {
                            None
                        },
                        activity_window,
                        uptime_window,
                        metrics_window: Some(metrics_window.clone()),
                        gateway_id: r.gateway_id,
                        node_pub_key: r.node_pub_key.unwrap_or_default(),
                        lightning_alias: r.lightning_alias.unwrap_or_default(),
                        api_endpoint: r.api_endpoint,
                        vetted: r.vetted,
                        raw: Some(r.raw),
                        first_seen: Some(r.first_seen),
                        last_seen: Some(r.last_seen),
                        protocols: Some(vec![GatewayProtocol::Lnv1]),
                        lnv2: None,
                    });
                }
                Some(GatewayProtocol::Lnv2) => {
                    // Fees come from the last successful probe, which is what `raw` holds
                    let routing_info: Option<RoutingInfo> = r
                        .raw
                        .get("routing_info")
                        .and_then(|info| serde_json::from_value(info.clone()).ok());
                    let routing_status = r
                        .routing_status
                        .as_deref()
                        .and_then(parse_routing_status)
                        .unwrap_or(Lnv2RoutingStatus::Unreachable);

                    let mut info = lnv2_gateway_info(
                        r.gateway_id,
                        r.api_endpoint,
                        routing_status,
                        r.routing_checked_at,
                        r.node_pub_key,
                        r.lightning_alias,
                        r.module_public_key,
                        routing_info.as_ref(),
                        r.raw,
                    );
                    // LNv2 activity attribution is not implemented yet
                    info.metrics_window = Some(metrics_window.clone());
                    info.first_seen = Some(r.first_seen);
                    info.last_seen = Some(r.last_seen);
                    info.uptime_window = uptime_window.clone();
                    if let Some(details) = &mut info.lnv2 {
                        details.uptime_window = uptime_window;
                    }
                    lnv2.push(info);
                }
                None => {
                    warn!(
                        "Ignoring gateway {} with unknown protocol {}",
                        r.gateway_id, r.protocol
                    );
                }
            }
        }

        let mut gateways = merge_protocols(lnv1, lnv2);
        gateways.sort_by_key(|gateway| std::cmp::Reverse(gateway.last_seen));
        Ok(gateways)
    }

    async fn federation_gateway_uptime_trend(
        &self,
        federation_id: FederationId,
        window: GatewayMetricsWindow,
    ) -> anyhow::Result<Vec<GatewayUptimeTrendPoint>> {
        #[derive(postgres_from_row::FromRow)]
        struct TrendRow {
            day: DateTime<Utc>,
            seen_samples: i64,
            total_samples: i64,
        }

        let conn = self.connection().await?;
        let federation_id_bytes = federation_id.consensus_encode_to_vec();
        let window_start = Utc::now() - window.duration();
        let rows = query::<TrendRow>(
            &conn,
            "SELECT
                 date_trunc('day', poll_time) AS day,
                 COUNT(*) FILTER (WHERE is_seen)::bigint AS seen_samples,
                 COUNT(*)::bigint AS total_samples
             FROM gateway_poll_snapshots
             WHERE federation_id = $1
               AND poll_time >= $2
             GROUP BY date_trunc('day', poll_time)
             ORDER BY day ASC",
            &[&federation_id_bytes, &window_start],
        )
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let seen_samples = row.seen_samples.max(0) as u64;
                let total_samples = row.total_samples.max(0) as u64;
                let uptime_pct = if total_samples == 0 {
                    0.0
                } else {
                    (seen_samples as f64 / total_samples as f64) * 100.0
                };
                GatewayUptimeTrendPoint {
                    day: row.day,
                    seen_samples,
                    total_samples,
                    uptime_pct,
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

pub(super) async fn get_federation_gateway_uptime_trend(
    Path(federation_id): Path<FederationId>,
    Query(params): Query<GetFederationGatewaysParams>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<Vec<GatewayUptimeTrendPoint>>> {
    let window = GatewayMetricsWindow::parse(params.window.as_deref())?;
    Ok(state
        .federation_observer
        .federation_gateway_uptime_trend(federation_id, window)
        .await?
        .into())
}

async fn store_lnv1_gateways(
    dbtx: &Transaction<'_>,
    federation_id_bytes: &[u8],
    now: DateTime<Utc>,
    gateways: &[LightningGatewayAnnouncement],
) -> anyhow::Result<()> {
    let mut gateway_ids = Vec::with_capacity(gateways.len());
    let mut node_pub_keys = Vec::with_capacity(gateways.len());
    let mut api_endpoints = Vec::with_capacity(gateways.len());
    let mut lightning_aliases = Vec::with_capacity(gateways.len());
    let mut vetted_flags = Vec::with_capacity(gateways.len());
    let mut raw_announcements = Vec::with_capacity(gateways.len());

    for gw in gateways {
        gateway_ids.push(gw.info.gateway_id.to_string());
        node_pub_keys.push(gw.info.node_pub_key.to_string());
        api_endpoints.push(gw.info.api.to_string());
        lightning_aliases.push(gw.info.lightning_alias.clone());
        vetted_flags.push(gw.vetted);
        raw_announcements.push(serde_json::to_string(gw)?);
    }

    if !gateway_ids.is_empty() {
        dbtx.execute(
            "INSERT INTO gateways
                 (federation_id, protocol, gateway_id, node_pub_key, api_endpoint,
                  lightning_alias, vetted, raw, first_seen, last_seen)
             SELECT
                 $1,
                 'lnv1',
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
             ON CONFLICT (federation_id, protocol, gateway_id) DO UPDATE
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

    let routing_ok = vec![None; gateway_ids.len()];
    store_poll_snapshots(
        dbtx,
        federation_id_bytes,
        now,
        GatewayProtocol::Lnv1,
        &gateway_ids,
        &routing_ok,
    )
    .await
}

async fn store_lnv2_gateways(
    dbtx: &Transaction<'_>,
    federation_id_bytes: &[u8],
    now: DateTime<Utc>,
    gateways: &[Lnv2Gateway],
) -> anyhow::Result<()> {
    let mut gateway_ids = Vec::with_capacity(gateways.len());
    let mut lightning_public_keys = Vec::with_capacity(gateways.len());
    let mut api_endpoints = Vec::with_capacity(gateways.len());
    let mut lightning_aliases = Vec::with_capacity(gateways.len());
    let mut raw_entries = Vec::with_capacity(gateways.len());
    let mut module_public_keys = Vec::with_capacity(gateways.len());
    let mut routing_statuses = Vec::with_capacity(gateways.len());
    let mut routing_ok = Vec::with_capacity(gateways.len());

    for gw in gateways {
        let routing_info = gw.probe.routing_info();
        gateway_ids.push(gw.gateway_id.clone());
        lightning_public_keys.push(routing_info.map(|info| info.lightning_public_key.to_string()));
        api_endpoints.push(gw.api_endpoint.to_string());
        lightning_aliases.push(routing_info.and_then(|info| info.lightning_alias.clone()));
        raw_entries.push(serde_json::to_string(&lnv2_raw(gw))?);
        module_public_keys.push(routing_info.map(|info| info.module_public_key.to_string()));
        routing_statuses.push(routing_status_str(gw.probe.status()));
        routing_ok.push(Some(routing_info.is_some()));
    }

    if !gateway_ids.is_empty() {
        // A failed or negative probe must not erase the last identity we verified, so
        // keys, alias and raw routing info are only overwritten by a successful probe.
        dbtx.execute(
            "INSERT INTO gateways
                 (federation_id, protocol, gateway_id, node_pub_key, api_endpoint,
                  lightning_alias, vetted, raw, first_seen, last_seen,
                  module_public_key, routing_status, routing_checked_at)
             SELECT
                 $1,
                 'lnv2',
                 gw.gateway_id,
                 gw.node_pub_key,
                 gw.api_endpoint,
                 gw.lightning_alias,
                 FALSE,
                 gw.raw_json::jsonb,
                 $2,
                 $2,
                 gw.module_public_key,
                 gw.routing_status,
                 $2
             FROM UNNEST(
                 $3::text[],
                 $4::text[],
                 $5::text[],
                 $6::text[],
                 $7::text[],
                 $8::text[],
                 $9::text[]
             ) AS gw(gateway_id, node_pub_key, api_endpoint, lightning_alias, raw_json,
                     module_public_key, routing_status)
             ON CONFLICT (federation_id, protocol, gateway_id) DO UPDATE
                 SET node_pub_key       = COALESCE(EXCLUDED.node_pub_key, gateways.node_pub_key),
                     api_endpoint       = EXCLUDED.api_endpoint,
                     lightning_alias    = COALESCE(EXCLUDED.lightning_alias, gateways.lightning_alias),
                     raw                = CASE WHEN EXCLUDED.routing_status = 'ok'
                                               THEN EXCLUDED.raw
                                               ELSE gateways.raw
                                          END,
                     module_public_key  = COALESCE(EXCLUDED.module_public_key, gateways.module_public_key),
                     routing_status     = EXCLUDED.routing_status,
                     routing_checked_at = EXCLUDED.routing_checked_at,
                     last_seen          = EXCLUDED.last_seen",
            &[
                &federation_id_bytes,
                &now,
                &gateway_ids,
                &lightning_public_keys,
                &api_endpoints,
                &lightning_aliases,
                &raw_entries,
                &module_public_keys,
                &routing_statuses,
            ],
        )
        .await?;

        dbtx.execute(
            "INSERT INTO lnv2_gateway_keys
                 (federation_id, gateway_id, module_public_key, lightning_public_key,
                  first_seen, last_seen)
             SELECT $1, k.gateway_id, k.module_public_key, k.lightning_public_key, $2, $2
             FROM UNNEST($3::text[], $4::text[], $5::text[])
                 AS k(gateway_id, module_public_key, lightning_public_key)
             WHERE k.module_public_key IS NOT NULL
             ON CONFLICT (federation_id, gateway_id, module_public_key) DO UPDATE
                 SET lightning_public_key = EXCLUDED.lightning_public_key,
                     last_seen            = EXCLUDED.last_seen",
            &[
                &federation_id_bytes,
                &now,
                &gateway_ids,
                &module_public_keys,
                &lightning_public_keys,
            ],
        )
        .await?;
    }

    store_poll_snapshots(
        dbtx,
        federation_id_bytes,
        now,
        GatewayProtocol::Lnv2,
        &gateway_ids,
        &routing_ok,
    )
    .await
}

/// Records one poll: every gateway currently in the protocol's registry as
/// seen, and every previously known gateway of that protocol as not seen.
async fn store_poll_snapshots(
    dbtx: &Transaction<'_>,
    federation_id_bytes: &[u8],
    now: DateTime<Utc>,
    protocol: GatewayProtocol,
    gateway_ids: &[String],
    routing_ok: &[Option<bool>],
) -> anyhow::Result<()> {
    dbtx.execute(
        "WITH current_gateways AS (
             SELECT *
             FROM UNNEST($4::text[], $5::boolean[]) AS c(gateway_id, routing_ok)
         ),
         all_gateways AS (
             SELECT gateway_id, TRUE AS is_seen, routing_ok
             FROM current_gateways
             UNION ALL
             SELECT g.gateway_id, FALSE AS is_seen, NULL::boolean AS routing_ok
             FROM gateways g
             WHERE g.federation_id = $1
               AND g.protocol = $3
               AND NOT EXISTS (
                   SELECT 1
                   FROM current_gateways c
                   WHERE c.gateway_id = g.gateway_id
               )
         )
         INSERT INTO gateway_poll_snapshots
             (federation_id, protocol, gateway_id, poll_time, is_seen, routing_ok)
         SELECT
             $1,
             $3,
             a.gateway_id,
             $2,
             a.is_seen,
             a.routing_ok
         FROM all_gateways a
         ON CONFLICT DO NOTHING",
        &[
            &federation_id_bytes,
            &now,
            &protocol_str(protocol),
            &gateway_ids,
            &routing_ok,
        ],
    )
    .await?;
    Ok(())
}

fn protocol_str(protocol: GatewayProtocol) -> &'static str {
    match protocol {
        GatewayProtocol::Lnv1 => "lnv1",
        GatewayProtocol::Lnv2 => "lnv2",
    }
}

fn parse_protocol(protocol: &str) -> Option<GatewayProtocol> {
    match protocol {
        "lnv1" => Some(GatewayProtocol::Lnv1),
        "lnv2" => Some(GatewayProtocol::Lnv2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use fedimint_core::core::{ModuleInstanceId, ModuleKind};
    use fmo_api_types::{GatewayInfo, GatewayProtocol, Lnv2RoutingStatus};

    use super::{lnv2_gateway_info, merge_protocols, normalize_gateway_url, LnInstances};

    const NODE_A: &str = "02aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NODE_B: &str = "03bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn lnv1(gateway_id: &str, node_pub_key: &str, api_endpoint: &str) -> GatewayInfo {
        GatewayInfo {
            gateway_id: gateway_id.to_owned(),
            node_pub_key: node_pub_key.to_owned(),
            lightning_alias: "alias".to_owned(),
            api_endpoint: api_endpoint.to_owned(),
            vetted: true,
            raw: None,
            first_seen: None,
            last_seen: None,
            activity_7d: None,
            activity_window: None,
            uptime_window: None,
            metrics_window: None,
            protocols: Some(vec![GatewayProtocol::Lnv1]),
            lnv2: None,
        }
    }

    fn lnv2(api_endpoint: &str, lightning_public_key: Option<&str>) -> GatewayInfo {
        lnv2_gateway_info(
            normalize_gateway_url(api_endpoint),
            api_endpoint.to_owned(),
            if lightning_public_key.is_some() {
                Lnv2RoutingStatus::Ok
            } else {
                Lnv2RoutingStatus::Unreachable
            },
            None,
            lightning_public_key.map(str::to_owned),
            None,
            None,
            None,
            serde_json::Value::Null,
        )
    }

    fn protocols(gateway: &GatewayInfo) -> Vec<GatewayProtocol> {
        gateway.protocols.clone().unwrap_or_default()
    }

    #[test]
    fn normalize_gateway_url_ignores_trailing_slash_and_v1() {
        let expected = "https://gw.example.com";
        for url in [
            "https://gw.example.com",
            "https://gw.example.com/",
            "https://gw.example.com/v1",
            "https://gw.example.com/v1/",
        ] {
            assert_eq!(normalize_gateway_url(url), expected, "{url}");
        }
        assert_eq!(
            normalize_gateway_url("https://gw.example.com/v12"),
            "https://gw.example.com/v12"
        );
    }

    #[test]
    fn ln_instances_detects_either_both_or_neither() {
        let ln = ModuleKind::from_static_str("ln");
        let lnv2 = ModuleKind::from_static_str("lnv2");
        let mint = ModuleKind::from_static_str("mint");
        fn kinds(modules: &[(ModuleInstanceId, &ModuleKind)]) -> LnInstances {
            LnInstances::from_kinds(modules.iter().copied())
        }

        let none = kinds(&[(0, &mint)]);
        assert!(none.is_empty());

        let v1_only = kinds(&[(0, &mint), (1, &ln)]);
        assert_eq!((v1_only.lnv1, v1_only.lnv2), (Some(1), None));

        let v2_only = kinds(&[(0, &mint), (3, &lnv2)]);
        assert_eq!((v2_only.lnv1, v2_only.lnv2), (None, Some(3)));

        let both = kinds(&[(1, &ln), (0, &mint), (2, &lnv2)]);
        assert_eq!((both.lnv1, both.lnv2), (Some(1), Some(2)));
    }

    #[test]
    fn merge_single_protocol_inputs_unchanged() {
        let merged = merge_protocols(vec![lnv1("g1", NODE_A, "https://a.example.com/v1")], vec![]);
        assert_eq!(merged.len(), 1);
        assert_eq!(protocols(&merged[0]), vec![GatewayProtocol::Lnv1]);

        let merged = merge_protocols(vec![], vec![lnv2("https://a.example.com", Some(NODE_A))]);
        assert_eq!(merged.len(), 1);
        assert_eq!(protocols(&merged[0]), vec![GatewayProtocol::Lnv2]);
    }

    #[test]
    fn merge_verified_dual_protocol_gateway() {
        let merged = merge_protocols(
            vec![lnv1("g1", NODE_A, "https://a.example.com/v1")],
            vec![lnv2("https://a.example.com/", Some(NODE_A))],
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].gateway_id, "g1");
        assert_eq!(
            protocols(&merged[0]),
            vec![GatewayProtocol::Lnv1, GatewayProtocol::Lnv2]
        );
        assert_eq!(
            merged[0]
                .lnv2
                .as_ref()
                .map(|details| details.routing_status),
            Some(Lnv2RoutingStatus::Ok)
        );
    }

    #[test]
    fn no_merge_without_both_key_and_url_match() {
        // Same node, different API
        let merged = merge_protocols(
            vec![lnv1("g1", NODE_A, "https://a.example.com")],
            vec![lnv2("https://other.example.com", Some(NODE_A))],
        );
        assert_eq!(merged.len(), 2);

        // Same API, different node
        let merged = merge_protocols(
            vec![lnv1("g1", NODE_A, "https://a.example.com")],
            vec![lnv2("https://a.example.com", Some(NODE_B))],
        );
        assert_eq!(merged.len(), 2);

        // Identity unknown because the probe failed
        let merged = merge_protocols(
            vec![lnv1("g1", NODE_A, "https://a.example.com")],
            vec![lnv2("https://a.example.com", None)],
        );
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn no_merge_when_match_is_ambiguous() {
        let merged = merge_protocols(
            vec![
                lnv1("g1", NODE_A, "https://a.example.com"),
                lnv1("g2", NODE_A, "https://a.example.com/v1"),
            ],
            vec![lnv2("https://a.example.com", Some(NODE_A))],
        );
        assert_eq!(merged.len(), 3);
        assert!(merged.iter().all(|gateway| protocols(gateway).len() == 1));
    }
}
