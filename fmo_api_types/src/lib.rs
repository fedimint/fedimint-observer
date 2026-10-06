use bitcoin::address::NetworkUnchecked;
use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::Amount;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FedimintTotals {
    pub federations: u64,
    pub tx_volume: Amount,
    pub tx_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationSummary {
    pub id: FederationId,
    pub name: Option<String>,
    pub last_7d_activity: Vec<FederationActivity>,
    pub deposits: Amount,
    pub invite: String,
    pub nostr_votes: FederationRating,
    pub health: FederationHealth,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FederationRating {
    pub count: u64,
    pub avg: Option<f64>,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct FederationActivity {
    pub num_transactions: u64,
    pub amount_transferred: Amount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationUtxo {
    pub address: bitcoin::Address<NetworkUnchecked>,
    pub out_point: bitcoin::OutPoint,
    pub amount: Amount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealth {
    pub avg_uptime: f32,
    pub avg_latency: f32,
    pub software_version: Option<String>,
    pub latest: Option<GuardianHealthLatest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealthLatest {
    pub block_height: u32,
    pub block_outdated: bool,
    pub session_count: u32,
    pub session_outdated: bool,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FederationHealth {
    Online,
    Degraded,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoncesRequest {
    pub nonces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceSpendInfo {
    pub session_index: u64,
    pub estimated_timestamp: Option<chrono::DateTime<chrono::Utc>>,
}

/// Subset of a gateway's registration info suitable for public API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayInfo {
    /// Gateway's public key (hex-encoded)
    pub gateway_id: String,
    /// LN node public key (hex-encoded)
    pub node_pub_key: String,
    pub lightning_alias: String,
    /// URL of the gateway's public API
    pub api_endpoint: String,
    /// Whether the federation has vetted this gateway
    pub vetted: bool,
    /// Full raw announcement, useful for forwards-compatible client usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    /// First time this gateway was seen by the observer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<DateTime<Utc>>,
    /// Most recent time this gateway was seen by the observer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
    /// Real LN activity metrics over the last 7 days
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_7d: Option<GatewayActivityMetrics>,
    /// Real LN activity metrics over the requested API window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_window: Option<GatewayActivityMetrics>,
    /// Uptime metrics computed from periodic gateway snapshots over the
    /// requested window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_window: Option<GatewayUptimeMetrics>,
    /// The window label used for `activity_window` and `uptime_window`, e.g.
    /// `7d`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_window: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayActivityMetrics {
    pub fund_count: u64,
    pub settle_count: u64,
    pub cancel_count: u64,
    pub total_volume_msat: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayUptimeMetrics {
    pub sample_count: u64,
    pub seen_samples: u64,
    pub online_minutes: u64,
    pub offline_minutes: u64,
    pub uptime_pct: f64,
}

/// Federation-wide gateway availability aggregated into daily buckets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayUptimeTrendPoint {
    pub day: DateTime<Utc>,
    pub seen_samples: u64,
    pub total_samples: u64,
    pub uptime_pct: f64,
}

/// Result category of a single Lightning probe towards a gateway's LN node.
///
/// A probe is a payment with a random payment hash, so it can never settle.
/// The outcome is derived from where and why the HTLC failed.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeOutcome {
    /// The gateway node itself rejected the unknown payment hash: the route
    /// works and the gateway can receive at least the probed amount.
    Success,
    /// The channel into the gateway could not forward the probed amount.
    InsufficientLiquidity,
    /// The gateway node was offline or its channels were disabled.
    Unreachable,
    /// An intermediate hop failed the HTLC.
    RouteFailure,
    /// No route to the gateway node could be found.
    NoRoute,
    /// The prober's own channel failed the HTLC, says nothing about the
    /// gateway.
    LocalFailure,
    /// The HTLC was not resolved in time.
    Timeout,
    /// Prober side error (e.g. LN node API failure).
    Error,
}

impl ProbeOutcome {
    pub const ALL: [ProbeOutcome; 8] = [
        ProbeOutcome::Success,
        ProbeOutcome::InsufficientLiquidity,
        ProbeOutcome::Unreachable,
        ProbeOutcome::RouteFailure,
        ProbeOutcome::NoRoute,
        ProbeOutcome::LocalFailure,
        ProbeOutcome::Timeout,
        ProbeOutcome::Error,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ProbeOutcome::Success => "success",
            ProbeOutcome::InsufficientLiquidity => "insufficient_liquidity",
            ProbeOutcome::Unreachable => "unreachable",
            ProbeOutcome::RouteFailure => "route_failure",
            ProbeOutcome::NoRoute => "no_route",
            ProbeOutcome::LocalFailure => "local_failure",
            ProbeOutcome::Timeout => "timeout",
            ProbeOutcome::Error => "error",
        }
    }

    /// Whether the outcome tells us something about the gateway. Prober side
    /// failures are excluded from gateway success rates.
    pub fn is_conclusive(self) -> bool {
        !matches!(
            self,
            ProbeOutcome::LocalFailure | ProbeOutcome::Timeout | ProbeOutcome::Error
        )
    }
}

impl std::str::FromStr for ProbeOutcome {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProbeOutcome::ALL
            .into_iter()
            .find(|outcome| outcome.as_str() == s)
            .ok_or_else(|| format!("Unknown probe outcome '{s}'"))
    }
}

/// A single probe measurement as reported by a prober.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProbeResult {
    /// LN node public key of the probed gateway (hex-encoded)
    pub node_pub_key: String,
    pub probe_time: DateTime<Utc>,
    pub amount_msat: u64,
    pub outcome: ProbeOutcome,
    /// Time until the HTLC was resolved
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// LN failure code as reported by the prober's node
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    /// Index of the hop that reported the failure (0 = prober's node)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_source_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_hops: Option<u32>,
    /// Fees the route would have cost if the payment had settled
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_fee_msat: Option<u64>,
}

/// Batch of probe results pushed by a prober to the observer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProbeSubmission {
    /// Identifies the probing node/location the results originate from
    pub prober_id: String,
    pub results: Vec<GatewayProbeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProbeSummary {
    pub window: String,
    /// Conclusive (see [`ProbeOutcome::is_conclusive`]) probes at
    /// `base_amount_msat`. Larger amounts are expected to fail eventually
    /// since probers increase the amount until one does, so they only count
    /// towards the liquidity estimate.
    pub conclusive_probes: u64,
    /// Successful probes at `base_amount_msat`
    pub successes: u64,
    /// Routing success rate at `base_amount_msat`
    pub success_rate_pct: Option<f64>,
    /// Probes of any amount that failed for prober side reasons
    pub inconclusive_probes: u64,
    /// Outcomes of probes of any amount
    pub outcome_counts: std::collections::BTreeMap<ProbeOutcome, u64>,
    pub last_probe_time: Option<DateTime<Utc>>,
    /// Smallest amount probed in the window, used as the reachability probe
    pub base_amount_msat: Option<u64>,
    /// Outcome of the most recent conclusive probe at `base_amount_msat`.
    /// Larger amounts failing only indicate limited liquidity, so this is the
    /// best indicator for whether the gateway is reachable at all.
    pub base_amount_last_outcome: Option<ProbeOutcome>,
    pub latency_p50_ms: Option<f64>,
    pub latency_p95_ms: Option<f64>,
    /// Largest successfully probed amount over the last 24h, a lower bound for
    /// the gateway's inbound liquidity
    pub max_successful_amount_msat: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProbeTrendPoint {
    pub bucket: DateTime<Utc>,
    /// Like [`GatewayProbeSummary`], counts only probes at the base amount
    pub conclusive_probes: u64,
    pub successes: u64,
    pub success_rate_pct: Option<f64>,
    pub latency_p50_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayProbeReport {
    pub node_pub_key: String,
    pub summary: GatewayProbeSummary,
    pub trend: Vec<GatewayProbeTrendPoint>,
    /// Most recent probes, newest first
    pub recent: Vec<GatewayProbeResult>,
}
