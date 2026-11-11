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
    /// That successor's name, when the observer tracks it
    #[serde(default)]
    pub successor_name: Option<String>,
}

/// One page of `GET /federations/page`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationPage {
    pub federations: Vec<FederationSummary>,
    /// Opaque cursor of the next page (`?after=`); null on the last page
    pub next_after: Option<String>,
    /// Federations matching the request across all pages
    pub total: u64,
    /// The largest daily figures over every federation, so charts on every
    /// page share one scale
    pub activity_scale: ActivityScale,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ActivityScale {
    pub max_transactions: u64,
    pub max_volume_msat: u64,
}

/// One page of `GET /nostr/federations/page`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrFederationPage {
    pub federations: Vec<NostrFederationEntry>,
    pub next_after: Option<String>,
    pub total: u64,
}

/// A federation announced on Nostr, with what the observer knows of it
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NostrFederationEntry {
    pub id: FederationId,
    pub invite: String,
    /// Set when the observer tracks the federation
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
    /// Left out when `include` doesn't ask for `gateways`; one page of them,
    /// searched, filtered and ordered as the request asks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateways: Option<Vec<GatewayInfo>>,
    /// Where the gateway list stands; with `gateways` only
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<ListPage>,
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

/// Window of `GET /federations/:id/health/history`
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum GuardianHealthWindow {
    #[serde(rename = "24h")]
    Day,
    #[serde(rename = "7d")]
    Week,
    #[serde(rename = "30d")]
    Month,
}

/// Each guardian's availability and latency over a window, from the hourly
/// health rollup. Historical: the latest polls are in `FederationLiveHealth`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealthHistory {
    pub window: GuardianHealthWindow,
    /// Seconds each bucket spans: an hour for 24h and 7d, a UTC day for 30d
    pub bucket_seconds: u32,
    /// UTC start of the first bucket; buckets follow every `bucket_seconds`
    /// up to and including the current, partial one
    pub start: DateTime<Utc>,
    /// Buckets in the window, the current partial one included
    pub bucket_count: u32,
    pub generated_at: DateTime<Utc>,
    /// Guardians with health samples on record, by guardian id
    pub guardians: Vec<GuardianHealthSeries>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealthSeries {
    pub guardian_id: u16,
    /// Online share of the polled time in the window, weighted by poll
    /// interval; None without samples in the window
    pub uptime_pct: Option<f32>,
    /// Mean latency of the answered, timed polls in the window
    pub avg_latency_ms: Option<f32>,
    /// Start of the last hour, within the last 90 days, in which the guardian
    /// answered at least one poll
    pub last_online_hour: Option<DateTime<Utc>>,
    /// Only buckets with samples, oldest first: a missing bucket was not
    /// polled, which is not the same as offline
    pub buckets: Vec<GuardianHealthBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianHealthBucket {
    pub start: DateTime<Utc>,
    pub samples: u32,
    pub online_samples: u32,
    /// Seconds the samples stand for, and of those the answered ones
    pub polled_s: u64,
    pub online_s: u64,
    pub avg_latency_ms: Option<f32>,
}

/// Each guardian's latest polls, from the last hour of raw health samples
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationLiveHealth {
    pub generated_at: DateTime<Utc>,
    pub guardians: Vec<GuardianLiveStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianLiveStatus {
    pub guardian_id: u16,
    /// The latest poll in the last hour, answered or not
    pub polled_at: Option<DateTime<Utc>>,
    /// The latest answered poll in the last hour; everything below is from it
    pub answered_at: Option<DateTime<Utc>>,
    pub latency_ms: Option<u32>,
    pub session_count: Option<u64>,
    /// Counts the guardian reports of its own peers
    pub peers_online: Option<u64>,
    pub peers_offline: Option<u64>,
    pub peers_flagged: Option<u64>,
    /// Session the guardian announced it shuts down at
    pub scheduled_shutdown: Option<u64>,
    /// How this guardian sees its peers' P2P connections, as it reported them
    pub peers: Vec<GuardianPeerView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianPeerView {
    pub peer_id: u16,
    pub connected: bool,
    /// Not contributing to consensus for a long time, by the reporter's view
    pub flagged: bool,
    /// Last session the peer contributed to, by the reporter's view
    pub last_contribution: Option<u64>,
}

/// The federation's Manifold seat-binding directory, the consensus meta field
/// `fedi:fman_seat_bindings`, as the observer last read and verified it
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldSeatBindingsReport {
    /// Whether the federation runs the meta module the directory lives in
    pub has_meta_module: bool,
    /// When the observer last read the guardians' consensus meta; None before
    /// the first successful read
    pub observed_at: Option<DateTime<Utc>>,
    pub status: SeatBindingsStatus,
    /// Why the directory is invalid; set only for `Invalid`
    pub error: Option<String>,
    /// The bindings, only once every one verified against the federation's
    /// config: an invalid directory binds no seat at all
    pub bindings: Vec<ManifoldSeatBinding>,
    /// Distinct Manifold Guardian identities among the verified bindings; one
    /// MG may operate several seats
    pub distinct_guardians: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeatBindingsStatus {
    /// The consensus meta has not been read yet, or the federation has no
    /// meta module
    NotObserved,
    /// The consensus meta has no `fedi:fman_seat_bindings` field
    NotPublished,
    /// The field is there but fails the Manifold specification's checks
    Invalid,
    /// Every seat has a binding whose signature and claims verified
    Verified,
}

/// One guardian seat bound to the Manifold Guardian that signed for it. A
/// verified binding proves who operates the seat, not that the operator holds
/// a trust credential: that is resolved separately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldSeatBinding {
    pub peer_id: u16,
    /// Lowercase x-only hex Nostr key of the Manifold Guardian
    pub fman_pubkey: String,
    /// Manifold's deterministic name for that key; presentation only, names
    /// can collide
    pub display_name: String,
    /// The seat's broadcast public key from the federation config
    pub guardian_identity: String,
    /// Single-signature BTC depositor key the seat takes guardian fees to
    pub guardian_fee_pubkey: String,
    /// Unix seconds the binding was signed
    pub issued_at: u64,
}

/// A discovery answer with the collector's provenance: Manifold data comes
/// from the observer's own relay polls, never from relays per request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldDiscovery<T> {
    /// The last relay sync that succeeded; None before the first
    pub observed_at: Option<DateTime<Utc>>,
    pub relays: Vec<String>,
    pub data: T,
    /// Paging of a list answer; absent on single-item answers
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<ListPage>,
    /// The Guardian directory's filter choices, over every listed guardian
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facets: Option<ManifoldDirectoryFacets>,
}

/// Where a paged list stands
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPage {
    /// Opaque cursor of the next page (`?after=`); null on the last page
    pub next_after: Option<String>,
    /// Rows matching the request across all pages
    pub total: u64,
}

/// What the directory's filters can choose from, independent of the page shown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifoldDirectoryFacets {
    /// Every listed guardian, before filters
    pub guardians: u64,
    /// Advertised minor versions ("0.12"), newest first
    pub versions: Vec<String>,
    /// Advertised federation sizes, ascending
    pub sizes: Vec<u16>,
}

/// Advertisement freshness by the FA's rule: issued, unexpired, at most 2h old
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
    /// Every required check passed
    Verified,
    /// A check the observer ran failed
    Failed,
    /// Not every check could run, so the credential is not trusted
    Unavailable,
    /// No credential presented
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialCheck {
    pub id: String,
    pub label: String,
    /// None when the check did not run
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

/// The advertisement a Manifold Guardian is shown with: its newest fresh v2
/// one, else its newest legacy one; fields are never merged across the two
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MgAdvertisement {
    pub event_kind: u16,
    pub event_id: String,
    pub issued_at: u64,
    pub expires_at: u64,
    /// Only verified advertisements are stored, so always true
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
    /// From verified seat bindings of observed federations only
    pub seat_count: u32,
    pub federation_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatedSeat {
    pub federation_id: String,
    pub federation_name: Option<String>,
    pub federation_guardians: u32,
    pub peer_id: u16,
    /// Unix seconds the binding was signed
    pub bound_at: u64,
    /// When the observer last read the directory it verified
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
    /// UTC day, "YYYY-MM-DD"
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
    /// Self-asserted
    pub display: Option<FlipDisplay>,
    pub supported_sources: Vec<String>,
    pub supported_networks: Vec<String>,
    pub accepted_attester_policies: Vec<FlipAttesterPolicy>,
    pub api_endpoints: Vec<String>,
    pub api_versions: Vec<u64>,
    pub relay_hints: Vec<String>,
    pub credential: TrustCredential,
    /// The observer has no way yet to confirm a provider serves a federation
    pub confirmed_federations: Vec<serde_json::Value>,
}
