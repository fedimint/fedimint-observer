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
    /// All active federations; `None` if unavailable or none reports a balance
    pub active_assets: Option<Amount>,
    /// Active federations with no known balance, left out of `active_assets`
    pub active_assets_unknown: Option<u64>,
    pub active_assets_reporting: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationOverview {
    pub session_count: u64,
    pub total_assets_msat: Option<Amount>,
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
    /// Estimated time of the first session this observer dated, in Unix
    /// seconds; not an announced start date. Set on shut-down page rows only
    pub first_session_timestamp: Option<u64>,
    /// Estimated time of the last session this observer dated, in Unix
    /// seconds; not an announced shutdown date. Set on shut-down page rows only
    pub last_session_timestamp: Option<u64>,
    pub invite: String,
    pub nostr_votes: FederationRating,
    pub health: FederationHealth,
    pub expiry_timestamp: Option<u64>,
    pub successor: Option<String>,
    pub successor_id: Option<FederationId>,
    #[serde(default)]
    pub successor_name: Option<String>,
    /// Module kind by instance id; only the single-federation summary sets it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module_kinds: Option<BTreeMap<u16, String>>,
    /// Wallet network; only the single-federation summary sets it
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationPage {
    pub federations: Vec<FederationSummary>,
    pub next_after: Option<String>,
    pub total: u64,
    pub activity_scale: ActivityScale,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ActivityScale {
    pub max_transactions: u64,
    pub max_volume_msat: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrFederationPage {
    pub federations: Vec<NostrFederationEntry>,
    pub next_after: Option<String>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrFederationEntry {
    pub id: FederationId,
    pub invite: String,
    pub name: Option<String>,
    pub health: Option<FederationHealth>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionHistogram {
    pub days: Vec<TransactionHistogramDay>,
    pub total_count: u64,
    pub total_volume_msat: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionHistogramDay {
    pub date: chrono::NaiveDate,
    pub count: u64,
    pub volume_msat: u64,
    pub avg_count: Option<f64>,
    pub avg_volume_msat: Option<f64>,
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
    pub threshold: usize,
    pub guardians: Vec<GuardianUtxoReport>,
    pub utxos: Vec<UtxoComparisonRow>,
    pub counts: UtxoStatusCounts,
    pub total_rows: usize,
    pub page: usize,
    pub limit: usize,
    pub page_count: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtxoView {
    #[default]
    All,
    Verified,
    Pending,
    Mismatch,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UtxoStatusCounts {
    pub all: usize,
    pub verified: usize,
    pub pending: usize,
    pub mismatch: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianUtxoReport {
    pub guardian_id: u16,
    pub status: GuardianUtxoClaimStatus,
    pub session_count: Option<u64>,
    pub error: Option<String>,
    pub missing_outputs: u32,
    pub extra_outputs: u32,
    pub wrong_amounts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoComparisonRow {
    pub out_point: bitcoin::OutPoint,
    pub amount: Amount,
    pub address: Option<bitcoin::Address<NetworkUnchecked>>,
    pub guardian_states: BTreeMap<u16, GuardianClaimedUtxoState>,
    pub status: UtxoComparisonStatus,
    pub disagreement: Option<GuardianUtxoDisagreementKind>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UtxoComparisonStatus {
    Mismatch,
    Pending,
    Verified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianUtxoClaim {
    pub guardian_id: u16,
    pub status: GuardianUtxoClaimStatus,
    pub session_count: Option<u64>,
    pub utxos: Vec<GuardianClaimedUtxo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianUtxoClaimStatus {
    Unavailable,
    Ok,
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
    EvidenceMismatch,
    InventoryDifference,
    ObserverDifference,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayInfo {
    pub gateway_id: String,
    pub node_pub_key: String,
    pub lightning_alias: String,
    pub api_endpoint: String,
    pub vetted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_7d: Option<GatewayActivityMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_window: Option<GatewayActivityMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_window: Option<GatewayUptimeMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics_window: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<GatewayStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_uptime_pct: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_online_minutes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_offline_minutes: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_unknown_minutes: Option<f64>,
}

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
    #[serde(default)]
    pub buckets: Vec<GatewayUptimeBucket>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct GatewayUptimeBucket {
    pub seen_samples: u64,
    pub total_samples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayUptimeTrendPoint {
    pub day: DateTime<Utc>,
    pub seen_samples: u64,
    pub total_samples: u64,
    pub uptime_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayOverview {
    pub window: String,
    pub computed_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateways: Option<Vec<GatewayInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<ListPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime_trend: Option<Vec<GatewayUptimeTrendPoint>>,
    pub summary: GatewayOverviewSummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayOverviewSummary {
    pub total: u64,
    pub online: u64,
    pub degraded: u64,
    pub offline: u64,
    pub unknown: u64,
    pub retired: u64,
    pub vetted: u64,
    pub avg_uptime_pct: Option<f64>,
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
    pub attribution: String,
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
    pub totals: GatewayV2Totals,
    pub availability_trend: Vec<GatewayV2AvailabilityBucket>,
    pub gateways: Vec<GatewayV2Info>,
    pub next_after: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayV2Totals {
    pub registered: u64,
    pub online: u64,
    pub degraded: u64,
    pub offline: u64,
    pub unknown: u64,
    pub removed: u64,
    pub samples: u64,
    pub online_samples: u64,
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

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum GuardianHealthWindow {
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealthHistory {
    pub window: GuardianHealthWindow,
    pub bucket_seconds: u32,
    pub start: DateTime<Utc>,
    pub bucket_count: u32,
    pub generated_at: DateTime<Utc>,
    pub guardians: Vec<GuardianHealthSeries>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealthSeries {
    pub guardian_id: u16,
    /// Online share of polled time, weighted by poll interval.
    /// `None` means there are no samples.
    pub uptime_pct: Option<f32>,
    pub avg_latency_ms: Option<f32>,
    pub last_online_hour: Option<DateTime<Utc>>,
    /// Sampled buckets, oldest first. An absent bucket was unpolled.
    pub buckets: Vec<GuardianHealthBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealthBucket {
    pub start: DateTime<Utc>,
    pub samples: u32,
    pub online_samples: u32,
    pub polled_s: u64,
    pub online_s: u64,
    pub avg_latency_ms: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationLiveHealth {
    pub generated_at: DateTime<Utc>,
    pub guardians: Vec<GuardianLiveStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianLiveStatus {
    pub guardian_id: u16,
    pub polled_at: Option<DateTime<Utc>>,
    pub answered_at: Option<DateTime<Utc>>,
    pub latency_ms: Option<u32>,
    pub session_count: Option<u64>,
    pub peers_online: Option<u64>,
    pub peers_offline: Option<u64>,
    pub peers_flagged: Option<u64>,
    pub scheduled_shutdown: Option<u64>,
    pub peers: Vec<GuardianPeerView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianPeerView {
    pub peer_id: u16,
    pub connected: bool,
    pub flagged: bool,
    pub last_contribution: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldSeatBindingsReport {
    pub has_meta_module: bool,
    pub observed_at: Option<DateTime<Utc>>,
    pub status: SeatBindingsStatus,
    pub error: Option<String>,
    pub bindings: Vec<ManifoldSeatBinding>,
    pub distinct_guardians: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeatBindingsStatus {
    NotObserved,
    NotPublished,
    Invalid,
    Verified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldSeatBinding {
    pub peer_id: u16,
    pub fman_pubkey: String,
    pub display_name: String,
    pub guardian_identity: String,
    pub guardian_fee_pubkey: String,
    pub issued_at: u64,
}

/// never from relays per request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldDiscovery<T> {
    pub observed_at: Option<DateTime<Utc>>,
    pub relays: Vec<String>,
    pub data: T,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<ListPage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facets: Option<ManifoldDirectoryFacets>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPage {
    pub next_after: Option<String>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldDirectoryFacets {
    pub guardians: u64,
    pub versions: Vec<String>,
    pub sizes: Vec<u16>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdFreshness {
    Fresh,
    Stale,
    Expired,
    NotYetValid,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
    Verified,
    Failed,
    Unavailable,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialCheck {
    pub id: String,
    pub label: String,
    pub passed: Option<bool>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustCredential {
    pub state: CredentialState,
    pub issuer_pubkey: Option<String>,
    pub issuer_label: Option<String>,
    /// The operator: the PeerBadge holder, never the advertiser's own key
    pub holder_pubkey: Option<String>,
    pub trust_level: Option<u64>,
    pub required_trust_level: u64,
    pub schema: Option<String>,
    pub checked_at: Option<DateTime<Utc>>,
    pub checks: Vec<CredentialCheck>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MgPlan {
    InfiniteBestEffort {
        price_msats: u64,
    },
    Subscription {
        initial_price_msats: u64,
        renewal_price_msats: u64,
        period: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MgApiEndpoint {
    pub transport: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MgAvailability {
    pub fedimintd_version: String,
    pub federation_sizes: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MgSoftware {
    pub mg_version: String,
    pub api_version: u64,
}

/// The advertisement a Manifold Guardian is shown with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MgAdvertisement {
    pub event_kind: u16,
    pub event_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub signature_valid: bool,
    pub service_pubkey: String,
    pub api_endpoints: Vec<MgApiEndpoint>,
    pub availability: MgAvailability,
    pub plans: Vec<MgPlan>,
    /// v2 only, asserted by the guardian itself: display, never evidence
    pub self_reported_uptime_permille: Option<BTreeMap<String, u16>>,
    pub software: Option<MgSoftware>,
    pub holder_authorization_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldGuardianSummary {
    pub fman_pubkey: String,
    pub display_name: String,
    pub advertisement: Option<MgAdvertisement>,
    pub freshness: Option<AdFreshness>,
    pub credential: TrustCredential,
    pub seat_count: u32,
    pub federation_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatedSeat {
    pub federation_id: String,
    pub federation_name: Option<String>,
    pub federation_guardians: u32,
    pub peer_id: u16,
    pub bound_at: u64,
    pub verified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MgVersionChange {
    pub observed_at: DateTime<Utc>,
    pub fedimintd_version: String,
    pub mg_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdObservationDay {
    pub date: String,
    pub fresh_polls: u32,
    pub polls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldGuardianProfile {
    #[serde(flatten)]
    pub summary: ManifoldGuardianSummary,
    pub nostr_relays: Vec<String>,
    pub seats: Vec<OperatedSeat>,
    pub version_history: Vec<MgVersionChange>,
    pub observation_history: Vec<AdObservationDay>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlipAttesterPolicy {
    pub attester_pubkey: String,
    pub verification_requirement: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlipDisplay {
    pub name: Option<String>,
    pub website: Option<String>,
    pub contact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlipProvider {
    pub provider_pubkey: String,
    pub event_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub freshness: AdFreshness,
    pub signature_valid: bool,
    pub display: Option<FlipDisplay>,
    pub supported_sources: Vec<String>,
    pub supported_networks: Vec<String>,
    pub accepted_attester_policies: Vec<FlipAttesterPolicy>,
    pub api_endpoints: Vec<String>,
    pub api_versions: Vec<u64>,
    pub relay_hints: Vec<String>,
    pub credential: TrustCredential,
    pub confirmed_federations: Vec<serde_json::Value>,
}
