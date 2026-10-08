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
pub struct FederationUtxosResponse {
    /// How many guardians must agree before an output counts as held or not
    /// held: `n - f`, the same threshold the federation itself acts on
    pub threshold: usize,
    pub guardians: Vec<GuardianUtxoReport>,
    /// One row per output, most severe status first, then largest amount first
    pub utxos: Vec<UtxoComparisonRow>,
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
