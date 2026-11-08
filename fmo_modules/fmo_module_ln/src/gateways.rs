//! Gateway monitoring: which gateways a federation's guardians announce,
//! how long each has been seen, and what it did in a window

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use fedimint_api_client::api::{DynGlobalApi, FederationApiExt};
use fedimint_core::config::{ClientConfig, FederationId, JsonClientConfig};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::encoding::Encodable;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::module::ApiRequestErased;
use fedimint_ln_common::federation_endpoint_constants::LIST_GATEWAYS_ENDPOINT;
use fedimint_ln_common::LightningGatewayAnnouncement;
use fmo_api_types::{
    GatewayActivityMetrics, GatewayInfo, GatewayOverview, GatewayOverviewSummary, GatewayStatus,
    GatewayUptimeBucket, GatewayUptimeMetrics, GatewayUptimeTrendPoint,
};
use fmo_core::module::{
    module_instance_id, query, ApiResult, ClientError, Services, INVITE_FETCH_TIMEOUT,
};
use futures::future::join_all;
use serde::Deserialize;
use tracing::{info, warn};

const GATEWAY_POLL_INTERVAL_MINUTES: u64 = 5;
/// How long one guardian may take to list its gateways. Matches the Lightning
/// v2 poller.
const PEER_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Backstop for a whole poll round, so it never delays the next one
const POLL_ROUND_TIMEOUT: Duration = Duration::from_secs(40);
const GATEWAY_SNAPSHOT_RETENTION_DAYS: i64 = 90;
const GATEWAY_PRUNE_INTERVAL_HOURS: i64 = 6;
// A gateway missing from the registry this long has left the federation: it
// gets no more "not seen" samples, and later ones don't count against uptime or
// the trend
const GATEWAY_RETIRED_AFTER_DAYS: i32 = 7;
/// Time slots per gateway availability strip
const GATEWAY_UPTIME_BUCKETS: usize = 30;
// Matches the poll interval, so short windows are never more than one poll
// behind; longer windows refresh every few ticks (see refresh_every_ticks)
const GATEWAY_OVERVIEW_TICK_MINUTES: u64 = 5;

/// Per gateway key, the contracts funded, settled and cancelled in the window
/// (`$1` federation, `$2` window start), and the volume funded.
///
/// A settle is the input that pays a contract out on success. For an outgoing
/// contract that is the gateway's claim, the only input carrying a preimage
/// witness: the user's refund after the timeout or after `CancelOutgoing` has
/// none. For an incoming contract it is the user's claim of the decrypted
/// preimage. Inputs there carry no witness, so the gateway's reclaim after an
/// invalid preimage (rare: it needs a bad offer) still counts as a settle.
///
/// NOT MATERIALIZED lets the planner see real row counts through the CTEs;
/// materialized, it estimated one row each and nested-looped ~10k x 10k rows
/// (90D: 55 s instead of 4 s)
const GATEWAY_ACTIVITY_SQL: &str = "WITH tx_window AS NOT MATERIALIZED (
         SELECT t.federation_id, t.txid
         FROM transactions t
         JOIN session_times st
             ON st.federation_id = t.federation_id
            AND st.session_index = t.session_index
         WHERE t.federation_id = $1
           AND st.estimated_session_timestamp >= $2
     ),
     window_ln_outputs AS NOT MATERIALIZED (
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
     window_ln_inputs AS NOT MATERIALIZED (
         SELECT
             i.federation_id,
             i.txid,
             i.in_index,
             i.ln_contract_id
         FROM transaction_inputs i
         JOIN tx_window tw
             ON tw.federation_id = i.federation_id
            AND tw.txid = i.txid
         WHERE i.federation_id = $1
           AND i.kind = 'ln'
           AND i.ln_contract_id IS NOT NULL
     ),
     contract_map AS NOT MATERIALIZED (
         SELECT DISTINCT ON (wlo.federation_id, wlo.ln_contract_id)
             wlo.federation_id,
             wlo.ln_contract_id,
             COALESCE(
                 d.details #>> '{V0,Contract,contract,Outgoing,gateway_key}',
                 d.details #>> '{V0,Contract,contract,Incoming,gateway_key}'
             ) AS gateway_key,
             d.details #> '{V0,Contract,contract,Outgoing}' IS NOT NULL AS outgoing
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
         LEFT JOIN transaction_input_details di
             ON di.federation_id = wli.federation_id
            AND di.txid = wli.txid
            AND di.in_index = wli.in_index
         WHERE NOT cm.outgoing
            OR di.details #>> '{V0,witness}' IS NOT NULL
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
     GROUP BY gateway_key";

type OverviewKey = (FederationId, GatewayMetricsWindow);
/// Overviews for every federation and window, rebuilt in the background so
/// requests never wait for the queries (90D takes seconds)
pub(crate) type GatewayOverviewCache = Arc<OverviewCache>;

#[derive(Default)]
pub(crate) struct OverviewCache {
    overviews: RwLock<HashMap<OverviewKey, Arc<GatewayOverview>>>,
    /// Held while an overview is computed. Until the cache is filled (after a
    /// restart, or for a federation added since the last refresh), concurrent
    /// requests for the same overview wait for the one computation instead of
    /// each running the heavy queries.
    computing: KeyLocks<OverviewKey>,
}

impl OverviewCache {
    fn get(&self, key: &OverviewKey) -> Option<Arc<GatewayOverview>> {
        self.overviews
            .read()
            .expect("gateway overview lock poisoned")
            .get(key)
            .cloned()
    }

    fn insert(&self, key: OverviewKey, overview: Arc<GatewayOverview>) {
        self.overviews
            .write()
            .expect("gateway overview lock poisoned")
            .insert(key, overview);
    }
}

/// One async lock per key, so concurrent misses of a cache compute a value
/// once: the first computes while holding the key's lock, the others wait for
/// it and then find the value in the cache. Keys are observed federations and
/// windows, so the map stays small.
struct KeyLocks<K>(Mutex<HashMap<K, Arc<tokio::sync::Mutex<()>>>>);

impl<K> Default for KeyLocks<K> {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

impl<K: Eq + std::hash::Hash + Clone> KeyLocks<K> {
    fn get(&self, key: &K) -> Arc<tokio::sync::Mutex<()>> {
        self.0
            .lock()
            .expect("cache key lock poisoned")
            .entry(key.clone())
            .or_default()
            .clone()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub(crate) enum GatewayMetricsWindow {
    H24,
    D7,
    D30,
    D90,
}

impl GatewayMetricsWindow {
    // The page's default window first, so it is ready soonest after a restart
    const ALL: [Self; 4] = [Self::D7, Self::H24, Self::D30, Self::D90];

    fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value.unwrap_or("7d") {
            "24h" => Ok(Self::H24),
            "7d" => Ok(Self::D7),
            "30d" => Ok(Self::D30),
            "90d" => Ok(Self::D90),
            invalid => bail!(ClientError::bad_request(format!(
                "Invalid gateways window '{invalid}'. Supported values: 24h, 7d, 30d, 90d"
            ))),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::H24 => "24h",
            Self::D7 => "7d",
            Self::D30 => "30d",
            Self::D90 => "90d",
        }
    }

    fn duration(self) -> chrono::Duration {
        match self {
            Self::H24 => chrono::Duration::hours(24),
            Self::D7 => chrono::Duration::days(7),
            Self::D30 => chrono::Duration::days(30),
            Self::D90 => chrono::Duration::days(90),
        }
    }

    /// How many overview ticks pass between refreshes. Five more minutes
    /// barely move a 30- or 90-day figure, and those windows run the heaviest
    /// queries.
    fn refresh_every_ticks(self) -> u64 {
        match self {
            Self::H24 | Self::D7 => 1,
            Self::D30 => 6,
            Self::D90 => 12,
        }
    }
}

/// The module's view of core, and its cache of overviews
pub(crate) struct Gateways<'a> {
    pub(crate) services: &'a Services,
    pub(crate) overviews: &'a GatewayOverviewCache,
}

/// `GET /federations/:federation_id/gateways/overview`
pub(crate) fn federation_routes(overviews: GatewayOverviewCache) -> Router<Services> {
    Router::new().route(
        "/gateways/overview",
        get(move |path, query, state| {
            get_federation_gateway_overview(overviews.clone(), path, query, state)
        }),
    )
}

/// `GET /config/:invite/gateways`
pub(crate) fn config_routes() -> Router<Services> {
    Router::new().route("/gateways", get(fetch_federation_gateways))
}

/// The gateways the guardians behind an invite code announce right now, for
/// federations that aren't observed
async fn fetch_federation_gateways(
    Path(invite): Path<InviteCode>,
    State(services): State<Services>,
) -> ApiResult<Json<Vec<GatewayInfo>>> {
    let config = services.config_for_invite(&invite).await?;
    let gateways = tokio::time::timeout(
        INVITE_FETCH_TIMEOUT,
        fetch_gateways_for_config(&services, &config, invite.api_secret()),
    )
    .await
    .map_err(|_| ClientError::gateway_timeout("Timed out fetching the federation's gateways"))??;
    Ok(gateways.into())
}

#[derive(Debug, Deserialize)]
pub(crate) struct GetFederationGatewaysParams {
    window: Option<String>,
    /// Comma-separated parts to return: `gateways`, `trend`. Both by default.
    include: Option<String>,
}

/// Registered gateways a federation's guardians currently announce. Each
/// guardian keeps its own registry, so this is the union across all of them.
///
/// A federation without a Lightning (v1) module is a 404 rather than an empty
/// list: it has no such registry, while an empty list means guardians answered
/// that no gateway is registered.
pub(crate) async fn fetch_gateways_for_config(
    services: &Services,
    config: &JsonClientConfig,
    api_secret: Option<String>,
) -> anyhow::Result<Vec<GatewayInfo>> {
    let ln_instance_id = config
        .modules
        .iter()
        .find_map(|(&instance_id, module)| (module.kind().as_str() == "ln").then_some(instance_id))
        .ok_or_else(|| {
            ClientError::not_found(
                "This federation has no Lightning (v1) module, so it has no Lightning v1 \
                 gateway registry",
            )
        })?;
    // The federation may not be observed: a lookup client, with the invite's secret
    let api = services.lookup_api(&config.global, api_secret)?;
    let peer_ids = config
        .global
        .api_endpoints
        .keys()
        .copied()
        .collect::<Vec<_>>();

    let announcements = fetch_gateway_announcements(&api, ln_instance_id, &peer_ids).await;
    if announcements.answered == 0 {
        let error = if announcements.all_timed_out() {
            ClientError::gateway_timeout("No guardian answered in time")
        } else {
            ClientError::bad_gateway("No guardian returned its gateway registry")
        };
        return Err(anyhow::Error::new(error).context(format!(
            "Listing gateways failed: {}",
            announcements.describe_failures()
        )));
    }
    announcements
        .merged
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
                status: None,
                coverage_pct: None,
                estimated_uptime_pct: None,
                estimated_online_minutes: None,
                estimated_offline_minutes: None,
                estimated_unknown_minutes: None,
            })
        })
        .collect()
}

/// Why a guardian's gateway list is missing from a poll
#[derive(Debug)]
enum PeerFailure {
    TimedOut,
    Failed(String),
}

/// Gateway announcements of every guardian that answered, merged by gateway
/// id, and which guardians didn't answer
struct Announcements {
    merged: HashMap<String, LightningGatewayAnnouncement>,
    answered: u32,
    failed: Vec<(fedimint_core::PeerId, PeerFailure)>,
}

impl Announcements {
    fn all_timed_out(&self) -> bool {
        !self.failed.is_empty()
            && self
                .failed
                .iter()
                .all(|(_, failure)| matches!(failure, PeerFailure::TimedOut))
    }

    /// The failed guardians and why, for logs
    fn describe_failures(&self) -> String {
        if self.failed.is_empty() {
            return "no guardians configured".to_owned();
        }
        self.failed
            .iter()
            .map(|(peer_id, failure)| match failure {
                PeerFailure::TimedOut => format!(
                    "peer {peer_id}: no answer within {}s",
                    PEER_REQUEST_TIMEOUT.as_secs()
                ),
                PeerFailure::Failed(error) => format!("peer {peer_id}: {error}"),
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Asks every guardian for the gateways it has registered, each within
/// `PEER_REQUEST_TIMEOUT`
async fn fetch_gateway_announcements(
    api: &DynGlobalApi,
    ln_instance_id: ModuleInstanceId,
    peer_ids: &[fedimint_core::PeerId],
) -> Announcements {
    let peer_results = join_all(peer_ids.iter().copied().map(|peer_id| async move {
        let module_api = api.with_module(ln_instance_id);
        let request = module_api.request_single_peer(
            LIST_GATEWAYS_ENDPOINT.to_owned(),
            ApiRequestErased::default(),
            peer_id,
        );
        let result = match tokio::time::timeout(PEER_REQUEST_TIMEOUT, request).await {
            Err(_) => Err(PeerFailure::TimedOut),
            Ok(Err(e)) => Err(PeerFailure::Failed(format!("{e:#}"))),
            Ok(Ok(value)) => serde_json::from_value::<Vec<LightningGatewayAnnouncement>>(value)
                .map_err(|e| PeerFailure::Failed(format!("invalid gateway list: {e}"))),
        };
        (peer_id, result)
    }))
    .await;

    let mut announcements = Announcements {
        merged: HashMap::new(),
        answered: 0,
        failed: Vec::new(),
    };
    for (peer_id, result) in peer_results {
        match result {
            Ok(gateways) => {
                announcements.answered += 1;
                for gw in gateways {
                    announcements
                        .merged
                        .entry(gw.info.gateway_id.to_string())
                        .or_insert(gw);
                }
            }
            Err(failure) => announcements.failed.push((peer_id, failure)),
        }
    }
    announcements
}

impl Gateways<'_> {
    /// Background task: poll the LN module on this federation for currently
    /// registered gateways and persist them. Runs in a loop until cancelled.
    pub(crate) async fn monitor_gateways(
        &self,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        const POLL_INTERVAL: Duration = Duration::from_secs(GATEWAY_POLL_INTERVAL_MINUTES * 60);

        let ln_instance_id =
            module_instance_id(&config, "ln").context("No LN module found in federation config")?;
        let api = self.services.federation_api(&config.global)?;

        let peer_ids: Vec<fedimint_core::PeerId> =
            config.global.api_endpoints.keys().copied().collect();

        let mut interval = tokio::time::interval(POLL_INTERVAL);
        // A round that overran waits a full interval, rather than the next
        // starting right away
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            // Nothing is recorded for a failed round, so it shows as "no polls"
            // (not observed) rather than as every gateway being offline
            if let Err(e) = self
                .fetch_and_store_gateways(federation_id, &api, ln_instance_id, &peer_ids)
                .await
            {
                warn!(
                    %federation_id,
                    "Gateway poll round failed, no snapshot recorded: {e:#}"
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
        let announcements = tokio::time::timeout(
            POLL_ROUND_TIMEOUT,
            fetch_gateway_announcements(api, ln_instance_id, peer_ids),
        )
        .await
        .map_err(|_| {
            anyhow!(
                "guardians did not answer within {}s",
                POLL_ROUND_TIMEOUT.as_secs()
            )
        })?;
        if announcements.answered == 0 {
            bail!(
                "no guardian answered ({})",
                announcements.describe_failures()
            );
        }
        if !announcements.failed.is_empty() {
            // Each guardian keeps its own registry, so a gateway registered
            // only with these is recorded as not seen this round
            warn!(
                %federation_id,
                answered = announcements.answered,
                total = peer_ids.len(),
                "Gateway poll round incomplete: {}",
                announcements.describe_failures()
            );
        }
        let merged = announcements.merged;

        let mut conn = self.services.connection().await?;
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
                "INSERT INTO fmo_ln.gateways
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

        // Every gateway in the registry gets a sample for this round: seen if
        // a guardian announced it, not seen otherwise (unless retired)
        dbtx.execute(
            "WITH current_gateway_ids AS (
                 SELECT UNNEST($3::text[]) AS gateway_id
             ),
             all_gateway_ids AS (
                 SELECT gateway_id, TRUE AS is_seen
                 FROM current_gateway_ids
                 UNION
                 SELECT g.gateway_id, FALSE AS is_seen
                 FROM fmo_ln.gateways g
                 WHERE g.federation_id = $1
                   AND g.last_seen >= $2::timestamptz - make_interval(days => $4)
                   AND NOT EXISTS (
                       SELECT 1
                       FROM current_gateway_ids c
                       WHERE c.gateway_id = g.gateway_id
                   )
             )
             INSERT INTO fmo_ln.gateway_poll_snapshots
                 (federation_id, gateway_id, poll_time, is_seen)
             SELECT
                 $1,
                 a.gateway_id,
                 $2,
                 a.is_seen
             FROM all_gateway_ids a
             ON CONFLICT DO NOTHING",
            &[
                &federation_id_bytes,
                &now,
                &gateway_ids,
                &GATEWAY_RETIRED_AFTER_DAYS,
            ],
        )
        .await?;

        let prune_interval_secs = GATEWAY_PRUNE_INTERVAL_HOURS * 60 * 60;
        let should_prune = now.timestamp().rem_euclid(prune_interval_secs)
            < (GATEWAY_POLL_INTERVAL_MINUTES as i64 * 60);
        let deleted_snapshots = if should_prune {
            let retention_cutoff = now - chrono::Duration::days(GATEWAY_SNAPSHOT_RETENTION_DAYS);
            dbtx.execute(
                "DELETE FROM fmo_ln.gateway_poll_snapshots
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

    /// Background task: rebuild every federation's gateway overviews. Runs in
    /// a loop until cancelled.
    pub(crate) async fn refresh_gateway_overviews(self) {
        let mut interval =
            tokio::time::interval(Duration::from_secs(GATEWAY_OVERVIEW_TICK_MINUTES * 60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Tick 0 builds every window, so all are ready right after a restart
        let mut tick: u64 = 0;
        loop {
            interval.tick().await;
            let start = Instant::now();
            let windows: Vec<GatewayMetricsWindow> = GatewayMetricsWindow::ALL
                .into_iter()
                .filter(|window| tick.is_multiple_of(window.refresh_every_ticks()))
                .collect();
            tick += 1;
            let federations = match self.services.federations().await {
                Ok(federations) => federations,
                Err(e) => {
                    warn!("Failed to list federations for gateway overviews: {e:?}");
                    continue;
                }
            };
            // One query at a time keeps the load on the database steady
            for &window in &windows {
                for federation in &federations {
                    let federation_id = federation.federation_id;
                    let key = (federation_id, window);
                    // Requests that miss the cache meanwhile wait for this
                    let computing = self.overviews.computing.get(&key);
                    let _computing = computing.lock().await;
                    match self.compute_gateway_overview(federation_id, window).await {
                        Ok(overview) => self.overviews.insert(key, Arc::new(overview)),
                        Err(e) => warn!(
                            "Failed to compute {} gateway overview for {}: {e:?}",
                            window.label(),
                            federation_id
                        ),
                    }
                }
            }
            info!(
                "Gateway overviews refreshed in {:.2}s ({})",
                start.elapsed().as_secs_f64(),
                windows
                    .iter()
                    .map(|window| window.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    async fn gateway_overview(
        &self,
        federation_id: FederationId,
        window: GatewayMetricsWindow,
    ) -> anyhow::Result<Arc<GatewayOverview>> {
        let key = (federation_id, window);
        if let Some(overview) = self.overviews.get(&key) {
            return Ok(overview);
        }
        // Only right after a restart, or for a federation added since the last
        // refresh. One computation per key at a time: the others wait for it
        // and take its result from the cache. The caller has checked the
        // federation is observed, so unknown IDs can't grow the cache.
        let computing = self.overviews.computing.get(&key);
        let _computing = computing.lock().await;
        if let Some(overview) = self.overviews.get(&key) {
            return Ok(overview);
        }
        let overview = Arc::new(self.compute_gateway_overview(federation_id, window).await?);
        self.overviews.insert(key, overview.clone());
        Ok(overview)
    }

    async fn compute_gateway_overview(
        &self,
        federation_id: FederationId,
        window: GatewayMetricsWindow,
    ) -> anyhow::Result<GatewayOverview> {
        let computed_at = Utc::now();
        let window_start = computed_at - window.duration();
        let gateways = self
            .list_federation_gateways(federation_id, window, computed_at)
            .await?;
        Ok(GatewayOverview {
            window: window.label().to_owned(),
            computed_at,
            summary: summarize(&gateways),
            gateways: Some(gateways),
            uptime_trend: Some(
                self.federation_gateway_uptime_trend(federation_id, window_start)
                    .await?,
            ),
        })
    }

    async fn list_federation_gateways(
        &self,
        federation_id: FederationId,
        window: GatewayMetricsWindow,
        computed_at: DateTime<Utc>,
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
            bucket: i32,
            seen_samples: i64,
            total_samples: i64,
        }

        let conn = self.services.connection().await?;
        let federation_id_bytes = federation_id.consensus_encode_to_vec();
        let window_start_utc = computed_at - window.duration();
        let window_start_naive = window_start_utc.naive_utc();
        let window_minutes = window.duration().num_minutes() as f64;
        let metrics_window = window.label().to_owned();

        let rows = query::<GatewayRow>(
            &conn,
            "SELECT gateway_id, node_pub_key, lightning_alias, api_endpoint, vetted, raw, first_seen, last_seen
             FROM fmo_ln.gateways
             WHERE federation_id = $1
             ORDER BY last_seen DESC",
            &[&federation_id_bytes],
        )
        .await?;

        let activity_rows = query::<GatewayActivityRow>(
            &conn,
            GATEWAY_ACTIVITY_SQL,
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
                 s.gateway_id,
                 LEAST($4::int - 1, GREATEST(0,
                     floor(EXTRACT(EPOCH FROM s.poll_time - $2)::float8 / $5::float8)::int)) AS bucket,
                 COUNT(*) FILTER (WHERE s.is_seen)::bigint AS seen_samples,
                 COUNT(*)::bigint AS total_samples
             FROM fmo_ln.gateway_poll_snapshots s
             JOIN fmo_ln.gateways g ON g.federation_id = s.federation_id AND g.gateway_id = s.gateway_id
             WHERE s.federation_id = $1
               AND s.poll_time >= $2
               AND s.poll_time <= g.last_seen + make_interval(days => $3)
             GROUP BY 1, 2",
            &[
                &federation_id_bytes,
                &window_start_utc,
                &GATEWAY_RETIRED_AFTER_DAYS,
                &(GATEWAY_UPTIME_BUCKETS as i32),
                &(window.duration().num_seconds() as f64 / GATEWAY_UPTIME_BUCKETS as f64),
            ],
        )
        .await?;

        let mut buckets_by_gateway_id: HashMap<String, Vec<GatewayUptimeBucket>> = HashMap::new();
        for row in uptime_rows {
            let buckets = buckets_by_gateway_id
                .entry(row.gateway_id)
                .or_insert_with(|| vec![GatewayUptimeBucket::default(); GATEWAY_UPTIME_BUCKETS]);
            // The query clamps the index into range
            let bucket = &mut buckets[usize::try_from(row.bucket).unwrap_or(0)];
            bucket.seen_samples += row.seen_samples.max(0) as u64;
            bucket.total_samples += row.total_samples.max(0) as u64;
        }
        let uptime_by_gateway_id: HashMap<String, GatewayUptimeMetrics> = buckets_by_gateway_id
            .into_iter()
            .map(|(gateway_id, buckets)| {
                let seen_samples: u64 = buckets.iter().map(|b| b.seen_samples).sum();
                let total_samples: u64 = buckets.iter().map(|b| b.total_samples).sum();
                let online_minutes = seen_samples.saturating_mul(GATEWAY_POLL_INTERVAL_MINUTES);
                let offline_minutes = total_samples
                    .saturating_sub(seen_samples)
                    .saturating_mul(GATEWAY_POLL_INTERVAL_MINUTES);
                let uptime_pct = uptime_pct(seen_samples, total_samples);
                (
                    gateway_id,
                    GatewayUptimeMetrics {
                        sample_count: total_samples,
                        seen_samples,
                        online_minutes,
                        offline_minutes,
                        uptime_pct,
                        buckets,
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
                let minutes_since_last_seen = minutes_between(r.last_seen, computed_at);
                let status = gateway_status(Some(minutes_since_last_seen));
                let estimate = WindowEstimate::new(
                    uptime_window.as_ref(),
                    status,
                    Some(minutes_since_last_seen),
                    window_minutes,
                );

                GatewayInfo {
                    status: Some(status),
                    coverage_pct: Some(estimate.coverage_pct),
                    estimated_uptime_pct: Some(estimate.uptime_pct),
                    estimated_online_minutes: Some(estimate.online_minutes),
                    estimated_offline_minutes: Some(estimate.offline_minutes),
                    estimated_unknown_minutes: Some(estimate.unknown_minutes),
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

    /// Uptime by UTC day, whatever the database session's time zone
    async fn federation_gateway_uptime_trend(
        &self,
        federation_id: FederationId,
        window_start: DateTime<Utc>,
    ) -> anyhow::Result<Vec<GatewayUptimeTrendPoint>> {
        #[derive(postgres_from_row::FromRow)]
        struct TrendRow {
            day: DateTime<Utc>,
            seen_samples: i64,
            total_samples: i64,
        }

        let conn = self.services.connection().await?;
        let federation_id_bytes = federation_id.consensus_encode_to_vec();
        let rows = query::<TrendRow>(
            &conn,
            "SELECT
                 date_trunc('day', s.poll_time AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' AS day,
                 COUNT(*) FILTER (WHERE s.is_seen)::bigint AS seen_samples,
                 COUNT(*)::bigint AS total_samples
             FROM fmo_ln.gateway_poll_snapshots s
             JOIN fmo_ln.gateways g ON g.federation_id = s.federation_id AND g.gateway_id = s.gateway_id
             WHERE s.federation_id = $1
               AND s.poll_time >= $2
               AND s.poll_time <= g.last_seen + make_interval(days => $3)
             GROUP BY 1
             ORDER BY 1",
            &[
                &federation_id_bytes,
                &window_start,
                &GATEWAY_RETIRED_AFTER_DAYS,
            ],
        )
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                let seen_samples = row.seen_samples.max(0) as u64;
                let total_samples = row.total_samples.max(0) as u64;
                let uptime_pct = uptime_pct(seen_samples, total_samples);
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

/// Minutes from `from` to `to`, fractional
fn minutes_between(from: DateTime<Utc>, to: DateTime<Utc>) -> f64 {
    (to - from).num_milliseconds() as f64 / 60_000.0
}

/// A gateway's status by the minutes since guardians last announced it
fn gateway_status(minutes_since_last_seen: Option<f64>) -> GatewayStatus {
    let Some(minutes) = minutes_since_last_seen else {
        return GatewayStatus::Unknown;
    };
    // Gateways are polled and overviews rebuilt every 5 minutes, so a healthy
    // gateway stays online for up to three polls
    if minutes <= 15.0 {
        GatewayStatus::Online
    } else if minutes <= 30.0 {
        GatewayStatus::Degraded
    } else if minutes > f64::from(GATEWAY_RETIRED_AFTER_DAYS) * 24.0 * 60.0 {
        GatewayStatus::Retired
    } else {
        GatewayStatus::Offline
    }
}

/// How a gateway's window divides into online, offline and unobserved
/// minutes
#[derive(Debug, Clone, Copy, PartialEq)]
struct WindowEstimate {
    coverage_pct: f64,
    uptime_pct: f64,
    online_minutes: f64,
    offline_minutes: f64,
    unknown_minutes: f64,
}

impl WindowEstimate {
    fn new(
        uptime: Option<&GatewayUptimeMetrics>,
        status: GatewayStatus,
        minutes_since_last_seen: Option<f64>,
        window_minutes: f64,
    ) -> Self {
        let (raw_online, raw_offline) = match uptime {
            Some(uptime) if uptime.sample_count > 0 => {
                (uptime.online_minutes as f64, uptime.offline_minutes as f64)
            }
            _ => (0.0, 0.0),
        };
        // Samples times the poll interval can add up to a little more than
        // the window
        let raw_total = raw_online + raw_offline;
        let clamp_scale = if raw_total > window_minutes {
            window_minutes / raw_total
        } else {
            1.0
        };
        let online_minutes = raw_online * clamp_scale;
        let observed_offline = raw_offline * clamp_scale;
        let sampled = online_minutes + observed_offline;
        let base_unknown = (window_minutes - sampled).max(0.0);
        // An offline gateway was offline since it was last seen, polled or not
        let inferred_offline_from_recency = match (status, minutes_since_last_seen) {
            (GatewayStatus::Offline, Some(minutes)) => minutes.min(base_unknown).max(0.0),
            _ => 0.0,
        };
        Self {
            coverage_pct: if window_minutes > 0.0 {
                sampled / window_minutes * 100.0
            } else {
                0.0
            },
            uptime_pct: if sampled > 0.0 {
                online_minutes / sampled * 100.0
            } else {
                0.0
            },
            online_minutes,
            offline_minutes: observed_offline + inferred_offline_from_recency,
            unknown_minutes: (base_unknown - inferred_offline_from_recency).max(0.0),
        }
    }
}

/// The overview's summary of its gateways, by their status and estimates.
/// Retired gateways count only as retired.
fn summarize(gateways: &[GatewayInfo]) -> GatewayOverviewSummary {
    let mut summary = GatewayOverviewSummary::default();
    let (mut coverage_sum, mut uptime_sum, mut observed) = (0.0, 0.0, 0u64);
    for gateway in gateways {
        match gateway.status.unwrap_or(GatewayStatus::Unknown) {
            GatewayStatus::Retired => {
                summary.retired += 1;
                continue;
            }
            GatewayStatus::Online => summary.online += 1,
            GatewayStatus::Degraded => summary.degraded += 1,
            GatewayStatus::Offline => summary.offline += 1,
            GatewayStatus::Unknown => summary.unknown += 1,
        }
        summary.total += 1;
        summary.vetted += u64::from(gateway.vetted);
        let coverage = gateway.coverage_pct.unwrap_or(0.0);
        coverage_sum += coverage;
        if coverage > 0.0 {
            observed += 1;
            uptime_sum += gateway.estimated_uptime_pct.unwrap_or(0.0);
        }
    }
    summary.avg_coverage_pct = (summary.total > 0).then(|| coverage_sum / summary.total as f64);
    summary.avg_uptime_pct = (observed > 0).then(|| uptime_sum / observed as f64);
    summary
}

fn uptime_pct(seen_samples: u64, total_samples: u64) -> f64 {
    if total_samples == 0 {
        0.0
    } else {
        (seen_samples as f64 / total_samples as f64) * 100.0
    }
}

/// Which parts of the overview to return, as (gateways, trend)
fn parse_overview_include(include: Option<&str>) -> anyhow::Result<(bool, bool)> {
    let Some(include) = include else {
        return Ok((true, true));
    };
    let (mut gateways, mut trend) = (false, false);
    for part in include.split(',').map(str::trim) {
        match part {
            "gateways" => gateways = true,
            "trend" => trend = true,
            invalid => bail!(ClientError::bad_request(format!(
                "Invalid include '{invalid}'. Supported values: gateways, trend"
            ))),
        }
    }
    Ok((gateways, trend))
}

pub(crate) async fn get_federation_gateway_overview(
    overviews: GatewayOverviewCache,
    Path(federation_id): Path<FederationId>,
    Query(params): Query<GetFederationGatewaysParams>,
    State(services): State<Services>,
) -> ApiResult<Json<GatewayOverview>> {
    let window = GatewayMetricsWindow::parse(params.window.as_deref())?;
    let (include_gateways, include_trend) = parse_overview_include(params.include.as_deref())?;
    services.ensure_observed(federation_id).await?;
    let overview = Gateways {
        services: &services,
        overviews: &overviews,
    }
    .gateway_overview(federation_id, window)
    .await?;
    // Only the requested parts are copied out of the shared cached overview
    Ok(Json(GatewayOverview {
        window: overview.window.clone(),
        computed_at: overview.computed_at,
        gateways: include_gateways
            .then(|| overview.gateways.clone())
            .flatten(),
        uptime_trend: include_trend
            .then(|| overview.uptime_trend.clone())
            .flatten(),
        summary: overview.summary.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use fedimint_core::{Amount, BitcoinHash};
    use fedimint_ln_common::contracts::{ContractId, Preimage};
    use fedimint_ln_common::{LightningInput, LightningInputV0};

    use super::*;

    fn status_of(error: anyhow::Error) -> StatusCode {
        error
            .downcast_ref::<ClientError>()
            .expect("a client error")
            .status()
    }

    #[test]
    fn invalid_overview_params_are_bad_requests() {
        assert_eq!(
            GatewayMetricsWindow::parse(None).unwrap(),
            GatewayMetricsWindow::D7
        );
        assert_eq!(
            GatewayMetricsWindow::parse(Some("90d")).unwrap(),
            GatewayMetricsWindow::D90
        );
        assert_eq!(
            status_of(GatewayMetricsWindow::parse(Some("bogus")).unwrap_err()),
            StatusCode::BAD_REQUEST
        );

        assert_eq!(parse_overview_include(None).unwrap(), (true, true));
        assert_eq!(
            parse_overview_include(Some("trend")).unwrap(),
            (false, true)
        );
        assert_eq!(
            parse_overview_include(Some("gateways, trend")).unwrap(),
            (true, true)
        );
        assert_eq!(
            status_of(parse_overview_include(Some("gateways,foo")).unwrap_err()),
            StatusCode::BAD_REQUEST
        );
    }

    /// The activity query tells settles from refunds by the witness in the
    /// stored input details, at `{V0,witness}`
    #[test]
    fn settles_are_inputs_with_a_preimage_witness() {
        let input = |witness| {
            LightningInput::V0(LightningInputV0 {
                contract_id: ContractId::from_byte_array([1; 32]),
                amount: Amount::from_msats(1000),
                witness,
            })
        };
        let claim = serde_json::to_value(input(Some(Preimage([7; 32])))).unwrap();
        let refund = serde_json::to_value(input(None)).unwrap();
        assert!(!claim.pointer("/V0/witness").unwrap().is_null());
        assert!(refund.pointer("/V0/witness").unwrap().is_null());
        assert!(GATEWAY_ACTIVITY_SQL.contains("di.details #>> '{V0,witness}' IS NOT NULL"));
    }

    #[test]
    fn gateway_status_by_recency() {
        assert_eq!(gateway_status(None), GatewayStatus::Unknown);
        assert_eq!(gateway_status(Some(15.0)), GatewayStatus::Online);
        assert_eq!(gateway_status(Some(15.1)), GatewayStatus::Degraded);
        assert_eq!(gateway_status(Some(30.0)), GatewayStatus::Degraded);
        assert_eq!(gateway_status(Some(31.0)), GatewayStatus::Offline);
        assert_eq!(
            gateway_status(Some(7.0 * 24.0 * 60.0)),
            GatewayStatus::Offline
        );
        assert_eq!(
            gateway_status(Some(7.0 * 24.0 * 60.0 + 1.0)),
            GatewayStatus::Retired
        );
    }

    fn uptime(online_minutes: u64, offline_minutes: u64) -> GatewayUptimeMetrics {
        GatewayUptimeMetrics {
            sample_count: (online_minutes + offline_minutes) / 5,
            seen_samples: online_minutes / 5,
            online_minutes,
            offline_minutes,
            uptime_pct: 0.0,
            buckets: Vec::new(),
        }
    }

    #[test]
    fn window_estimates_match_the_gateways_page() {
        // Half the 24h window polled, 3/4 of that online
        let estimate = WindowEstimate::new(
            Some(&uptime(540, 180)),
            GatewayStatus::Online,
            Some(1.0),
            1440.0,
        );
        assert_eq!(
            estimate,
            WindowEstimate {
                coverage_pct: 50.0,
                uptime_pct: 75.0,
                online_minutes: 540.0,
                offline_minutes: 180.0,
                unknown_minutes: 720.0,
            }
        );

        // More polled minutes than the window are scaled down to it
        let estimate = WindowEstimate::new(
            Some(&uptime(1500, 300)),
            GatewayStatus::Online,
            Some(1.0),
            1440.0,
        );
        assert_eq!(estimate.online_minutes, 1200.0);
        assert_eq!(estimate.offline_minutes, 240.0);
        assert_eq!(estimate.unknown_minutes, 0.0);
        assert_eq!(estimate.coverage_pct, 100.0);

        // An offline gateway's unpolled minutes since it was last seen count
        // as offline, up to the unobserved part of the window
        let estimate = WindowEstimate::new(
            Some(&uptime(600, 0)),
            GatewayStatus::Offline,
            Some(100.0),
            1440.0,
        );
        assert_eq!(estimate.offline_minutes, 100.0);
        assert_eq!(estimate.unknown_minutes, 740.0);
        assert_eq!(estimate.uptime_pct, 100.0);
        let estimate = WindowEstimate::new(
            Some(&uptime(600, 0)),
            GatewayStatus::Offline,
            Some(5000.0),
            1440.0,
        );
        assert_eq!(estimate.offline_minutes, 840.0);
        assert_eq!(estimate.unknown_minutes, 0.0);

        // Without polls nothing is known
        let estimate = WindowEstimate::new(None, GatewayStatus::Unknown, None, 1440.0);
        assert_eq!(
            estimate,
            WindowEstimate {
                coverage_pct: 0.0,
                uptime_pct: 0.0,
                online_minutes: 0.0,
                offline_minutes: 0.0,
                unknown_minutes: 1440.0,
            }
        );
    }

    #[test]
    fn summary_leaves_out_retired_gateways() {
        let gateway = |status, vetted, coverage_pct: f64, uptime_pct: f64| GatewayInfo {
            gateway_id: String::new(),
            node_pub_key: String::new(),
            lightning_alias: String::new(),
            api_endpoint: String::new(),
            vetted,
            raw: None,
            first_seen: None,
            last_seen: None,
            activity_7d: None,
            activity_window: None,
            uptime_window: None,
            metrics_window: None,
            status: Some(status),
            coverage_pct: Some(coverage_pct),
            estimated_uptime_pct: Some(uptime_pct),
            estimated_online_minutes: None,
            estimated_offline_minutes: None,
            estimated_unknown_minutes: None,
        };
        let summary = summarize(&[
            gateway(GatewayStatus::Online, true, 60.0, 80.0),
            gateway(GatewayStatus::Degraded, false, 30.0, 40.0),
            gateway(GatewayStatus::Offline, false, 0.0, 0.0),
            gateway(GatewayStatus::Unknown, false, 0.0, 0.0),
            gateway(GatewayStatus::Retired, true, 100.0, 100.0),
        ]);
        assert_eq!(
            (
                summary.total,
                summary.online,
                summary.degraded,
                summary.offline,
                summary.unknown,
                summary.retired,
                summary.vetted
            ),
            (4, 1, 1, 1, 1, 1, 1)
        );
        // Coverage over every active gateway, uptime over the polled ones
        assert_eq!(summary.avg_coverage_pct, Some(22.5));
        assert_eq!(summary.avg_uptime_pct, Some(60.0));

        let empty = summarize(&[gateway(GatewayStatus::Retired, false, 50.0, 50.0)]);
        assert_eq!((empty.total, empty.retired), (0, 1));
        assert_eq!(empty.avg_coverage_pct, None);
        assert_eq!(empty.avg_uptime_pct, None);
    }

    #[test]
    fn failed_peers_are_described() {
        let announcements = Announcements {
            merged: HashMap::new(),
            answered: 0,
            failed: vec![
                (fedimint_core::PeerId::from(0), PeerFailure::TimedOut),
                (
                    fedimint_core::PeerId::from(2),
                    PeerFailure::Failed("connection refused".to_owned()),
                ),
            ],
        };
        assert!(!announcements.all_timed_out());
        assert_eq!(
            announcements.describe_failures(),
            "peer 0: no answer within 10s; peer 2: connection refused"
        );
    }
}
