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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoncesRequest {
    pub nonces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonceSpendInfo {
    pub session_index: u64,
    pub estimated_timestamp: Option<chrono::DateTime<chrono::Utc>>,
}
