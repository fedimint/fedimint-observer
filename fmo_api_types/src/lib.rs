use std::collections::BTreeMap;

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

/// `GET /federations/:federation_id`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationOverview {
    pub session_count: u64,
    pub total_assets_msat: Option<Amount>,
    /// Sessions each module has processed, by module kind: behind
    /// `session_count` while the module catches up with the stored sessions
    pub module_sessions: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletV2OnChainTransaction {
    pub index: u64,
    pub txid: String,
    pub input_msat: u64,
    pub output_msat: u64,
    pub fee_msat: u64,
    pub vbytes: u64,
    pub created: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletV2History {
    pub module_instance_id: u16,
    pub transactions: Vec<WalletV2OnChainTransaction>,
    pub next_before: Option<u64>,
    pub observed_at: Option<DateTime<Utc>>,
    pub stale: bool,
    pub balance_msat: Option<u64>,
    pub balance_observed_at: Option<DateTime<Utc>>,
    pub balance_stale: bool,
    pub accounting_pending: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationSummary {
    pub id: FederationId,
    pub name: Option<String>,
    pub last_7d_activity: Vec<FederationActivity>,
    pub deposits: Option<Amount>,
    pub invite: String,
    pub nostr_votes: FederationRating,
    pub health: FederationHealth,
    pub expiry_timestamp: Option<u64>,
    /// `federation_successor` meta field as published, normally an invite code
    pub successor: Option<String>,
    /// Federation the successor invite code points to, when it parses as one
    pub successor_id: Option<FederationId>,
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

/// `GET /federations/:id/transactions/histogram`: funded transactions per UTC
/// day, from the first day with any through today
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionHistogram {
    /// Every UTC calendar day of the range, days without activity included
    pub days: Vec<TransactionHistogramDay>,
    /// Of all days, outliers left out if they were excluded
    pub total_count: u64,
    pub total_volume_msat: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionHistogramDay {
    /// `YYYY-MM-DD`, UTC
    pub date: chrono::NaiveDate,
    /// Funded transactions
    pub count: u64,
    pub volume_msat: u64,
    /// Trailing moving averages over the requested number of days, `None`
    /// unless averages were requested
    pub avg_count: Option<f64>,
    pub avg_volume_msat: Option<f64>,
    /// Volume of at least 10 times the 95th percentile of days with activity;
    /// only flagged if outliers were excluded
    pub outlier: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationUtxo {
    pub address: bitcoin::Address<NetworkUnchecked>,
    pub out_point: bitcoin::OutPoint,
    pub amount: Amount,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationUtxosResponse {
    /// How many guardians must agree before an output counts as held or not
    /// held: `n - f`, the same threshold the federation itself acts on
    pub threshold: usize,
    pub guardians: Vec<GuardianUtxoReport>,
    /// The requested page of the rows matching `view` and `search`, one row
    /// per output, most severe status first, then largest amount first
    pub utxos: Vec<UtxoComparisonRow>,
    /// Rows per status over every output, before `view` and `search`
    pub counts: UtxoStatusCounts,
    /// Rows matching `view` and `search`, across all pages
    pub total_rows: usize,
    /// The 1-based page served: the requested one, clamped to the last page
    pub page: usize,
    /// Rows per page
    pub limit: usize,
    /// Pages of rows matching `view` and `search`, at least 1
    pub page_count: usize,
}

/// Which rows `GET /federations/:id/utxos?view=` lists
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtxoView {
    #[default]
    All,
    Verified,
    Pending,
    Mismatch,
}

/// How many outputs have each status
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtxoStatusCounts {
    pub all: usize,
    pub verified: usize,
    pub pending: usize,
    pub mismatch: usize,
}

/// How a guardian took part in the comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianUtxoReport {
    pub guardian_id: u16,
    pub status: GuardianUtxoClaimStatus,
    /// Session count the guardian reported alongside its wallet summary
    pub session_count: Option<u64>,
    pub error: Option<String>,
    /// Outputs a threshold of guardians agrees the federation holds, but this
    /// guardian does not list
    pub missing_outputs: u32,
    /// Outputs this guardian lists, but a threshold of guardians agrees the
    /// federation does not hold
    pub extra_outputs: u32,
    /// Outputs this guardian lists with another amount than a threshold of
    /// guardians agrees on
    pub wrong_amounts: u32,
}

/// An output as seen by the observer, the guardians and, where needed, the
/// blockchain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoComparisonRow {
    pub out_point: bitcoin::OutPoint,
    pub amount: Amount,
    /// Only known for outputs in the observer's history
    pub address: Option<bitcoin::Address<NetworkUnchecked>>,
    /// State each guardian that took part in the comparison lists the output in
    pub guardian_states: BTreeMap<u16, GuardianClaimedUtxoState>,
    pub status: UtxoComparisonStatus,
    pub disagreement: Option<GuardianUtxoDisagreementKind>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtxoComparisonStatus {
    Mismatch,
    /// Not confirmed yet: in flight, observer lag, or no guardian responded
    Pending,
    Verified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianUtxoClaim {
    pub guardian_id: u16,
    pub status: GuardianUtxoClaimStatus,
    /// Session count the guardian reported alongside its wallet summary
    pub session_count: Option<u64>,
    pub utxos: Vec<GuardianClaimedUtxo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianUtxoClaimStatus {
    Unavailable,
    Ok,
    /// Answered, but behind the federation's session count, so its wallet
    /// summary is stale and left out of the comparison
    Lagging,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianClaimedUtxo {
    pub out_point: bitcoin::OutPoint,
    pub amount: Amount,
    pub state: GuardianClaimedUtxoState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianClaimedUtxoState {
    Spendable,
    UnsignedPegOut,
    UnsignedChange,
    UnconfirmedPegOut,
    UnconfirmedChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianUtxoDisagreementKind {
    /// The observer's amount differs from the one a threshold of guardians
    /// agrees on, or guardians cannot agree on an amount
    EvidenceMismatch,
    /// Guardians cannot reach a threshold on whether the federation holds the
    /// output, or only a minority lists an output the rest agree is not held
    InventoryDifference,
    /// A threshold of guardians agrees, but the observer's history differs,
    /// usually because it lags behind consensus or a peg-out is still pending
    ObserverDifference,
    /// The blockchain contradicts what the observer or guardians report: the
    /// output is already spent, does not exist, or holds a different amount
    OnChainConflict,
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
    pub block_height: Option<u32>,
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
    /// The fields below are set in the overview only, measured at its
    /// `computed_at` over its window.
    /// How recently the gateway was announced: online within 15 minutes,
    /// degraded within 30, retired after 7 days, offline otherwise; unknown if
    /// never seen
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<GatewayStatus>,
    /// Share of the window covered by polls, 0-100
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_pct: Option<f64>,
    /// Online share of the polled minutes, 0-100 (0 without polls)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_uptime_pct: Option<f64>,
    /// Polled minutes the gateway was seen, scaled to fit the window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_online_minutes: Option<f64>,
    /// Polled minutes it was not seen, plus, for an offline gateway, the
    /// unpolled minutes since it was last seen
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_offline_minutes: Option<f64>,
    /// The rest of the window
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_unknown_minutes: Option<f64>,
}

/// A Lightning v1 gateway's status by how recently guardians announced it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayStatus {
    Online,
    Degraded,
    Offline,
    Unknown,
    Retired,
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
    /// The window split into equal time slots, oldest first, each with the
    /// polls recorded in it. A slot without polls was not observed.
    #[serde(default)]
    pub buckets: Vec<GatewayUptimeBucket>,
}

/// Polls of one gateway recorded in one slot of the window
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct GatewayUptimeBucket {
    pub seen_samples: u64,
    pub total_samples: u64,
}

/// Federation-wide gateway availability aggregated into daily buckets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayUptimeTrendPoint {
    /// The UTC day, as its midnight (`2026-11-08T00:00:00Z`)
    pub day: DateTime<Utc>,
    pub seen_samples: u64,
    pub total_samples: u64,
    pub uptime_pct: f64,
}

/// A federation's gateways and their availability trend for one window,
/// computed at the same moment so both always agree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayOverview {
    /// The window label, e.g. `7d`
    pub window: String,
    pub computed_at: DateTime<Utc>,
    /// Left out when `include` doesn't ask for `gateways`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateways: Option<Vec<GatewayInfo>>,
    /// Left out when `include` doesn't ask for `trend`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_trend: Option<Vec<GatewayUptimeTrendPoint>>,
    /// Over all the federation's gateways, whatever `include` asks for
    pub summary: GatewayOverviewSummary,
}

/// A federation's gateways summed up for the overview's window. Retired
/// gateways count only in `retired`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayOverviewSummary {
    /// Gateways that aren't retired
    pub total: u64,
    pub online: u64,
    pub degraded: u64,
    pub offline: u64,
    pub unknown: u64,
    pub retired: u64,
    /// Of the gateways that aren't retired
    pub vetted: u64,
    /// Mean `estimated_uptime_pct` of the gateways that aren't retired and
    /// were polled in the window; `null` without any
    pub avg_uptime_pct: Option<f64>,
    /// Mean `coverage_pct` of the gateways that aren't retired; `null`
    /// without any
    pub avg_coverage_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayV2Info {
    pub status: String,
    pub api_endpoint: String,
    pub announcing_guardians: Vec<u16>,
    pub registered: bool,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub routing_info: Option<serde_json::Value>,
    pub routing_observed_at: Option<DateTime<Utc>>,
    pub probed_at: Option<DateTime<Utc>>,
    pub reachable: Option<bool>,
    pub federation_supported: Option<bool>,
    pub latency_ms: Option<i32>,
    pub samples: u64,
    pub online_samples: u64,
    pub avg_latency_ms: Option<f64>,
    pub availability: Vec<GatewayV2AvailabilityBucket>,
    pub activity: GatewayV2Activity,
    /// How `activity` is attributed. Gateways self-report their module key
    /// and the observer cannot verify ownership: `self_reported` credits
    /// contracts under keys this endpoint alone claims, `contested` means some
    /// of its keys are also claimed by another registered endpoint and credit
    /// neither, `none` means no key has been reported yet.
    pub attribution: String,
    /// Keys this endpoint claims that another registered endpoint also claims
    pub contested_keys: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayV2Registry {
    pub activity_caught_up: bool,
    pub window: String,
    pub module_instance_id: u16,
    pub observed_at: Option<DateTime<Utc>>,
    pub stale: bool,
    pub successful_guardians: i32,
    pub total_guardians: i32,
    /// Over every gateway of the module instance, not just this page
    pub totals: GatewayV2Totals,
    /// Probes of every gateway of the module instance (registered or
    /// removed), not just this page, in the same UTC buckets as each
    /// gateway's `availability`
    pub availability_trend: Vec<GatewayV2AvailabilityBucket>,
    pub gateways: Vec<GatewayV2Info>,
    pub next_after: Option<String>,
}

/// A module instance's gateways summed over all pages, for the window
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayV2Totals {
    /// Gateways guardians currently announce
    pub registered: u64,
    /// Gateways per `GatewayV2Info::status`
    pub online: u64,
    pub degraded: u64,
    pub offline: u64,
    pub unknown: u64,
    pub removed: u64,
    /// Probes in the window, and the successful ones
    pub samples: u64,
    pub online_samples: u64,
    /// Median of online gateways' latest successful probe latency
    pub median_latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayV2AvailabilityBucket {
    pub start: DateTime<Utc>,
    pub samples: u64,
    pub online_samples: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayV2Activity {
    pub incoming_contracts: u64,
    pub outgoing_contracts: u64,
    pub incoming_msat: u64,
    pub outgoing_msat: u64,
    pub outgoing_claimed: u64,
    pub outgoing_refunded: u64,
    pub outgoing_cancelled: u64,
}

/// A federation's current Lightning v1 and v2 gateway registrations, across
/// all module instances, grouped by the Lightning node they name
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayNodes {
    /// Ordered by `node_pub_key`
    pub nodes: Vec<GatewayNode>,
    /// Registrations without a valid node key, such as a Lightning v2
    /// endpoint that hasn't reported routing info yet
    pub unlinked: Vec<GatewayRegistration>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayNode {
    /// Compressed public key, lowercase hex
    pub node_pub_key: String,
    /// Lightning v1 registrations first, then Lightning v2
    pub registrations: Vec<GatewayRegistration>,
}

/// A Lightning v1 gateway that isn't retired, or a registered Lightning v2
/// endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "protocol", rename_all = "snake_case")]
pub enum GatewayRegistration {
    Lnv1 {
        gateway_id: String,
        api_endpoint: String,
        first_seen: DateTime<Utc>,
        last_seen: DateTime<Utc>,
    },
    Lnv2 {
        module_instance_id: u16,
        api_endpoint: String,
        first_seen: DateTime<Utc>,
        last_seen: DateTime<Utc>,
        /// As `GatewayV2Info::status`
        status: String,
        /// Whether a Lightning v1 registration of the same node announces this
        /// exact endpoint. The endpoint reports its node key itself, so
        /// `false` means its link to the node is unverified.
        url_matches_lnv1: bool,
    },
}

#[cfg(test)]
mod tests;
