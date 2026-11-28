export interface FedimintTotals {
  federations: number;
  tx_volume: number;
  tx_count: number;
  /** msat; null if unavailable or no active federation reports, absent on older servers */
  active_assets?: number | null;
  /** Active federations with no known balance yet, left out of active_assets */
  active_assets_unknown?: number | null;
  active_assets_reporting?: number | null;
}

export interface FederationOverview {
  session_count: number;
  total_assets_msat: number | null;

  module_sessions: Record<string, number>;
}

export interface WalletV2OnChainTransaction {
  index: number;
  txid: string;
  input_msat: number;
  output_msat: number;
  fee_msat: number;
  vbytes: number;
  created: number;
}

export interface WalletV2History {
  module_instance_id: number;
  transactions: WalletV2OnChainTransaction[];
  next_before: number | null;
  observed_at: string | null;
  stale: boolean;
  balance_msat: number | null;
  balance_observed_at: string | null;
  balance_stale: boolean;
  accounting_pending: boolean;
}

export interface GatewayV2RoutingInfo {
  lightning_public_key: string;
  lightning_alias?: string | null;
  module_public_key: string;
  send_fee_minimum: { base: number; parts_per_million: number };
  send_fee_default: { base: number; parts_per_million: number };
  receive_fee: { base: number; parts_per_million: number };
  expiration_delta_minimum: number;
  expiration_delta_default: number;
}

export interface GatewayV2Registry {
  activity_caught_up: boolean;
  window: GatewayWindow;
  module_instance_id: number;
  observed_at: string | null;
  stale: boolean;
  successful_guardians: number;
  total_guardians: number;
  next_after: string | null;

  totals: GatewayV2Totals;

  availability_trend: GatewayV2AvailabilityBucket[];
  gateways: {
    api_endpoint: string;
    announcing_guardians: number[];
    registered: boolean;
    first_seen: string;
    last_seen: string;
    status: 'online' | 'degraded' | 'offline' | 'unknown' | 'removed';
    routing_info: GatewayV2RoutingInfo | null;
    routing_observed_at: string | null;
    probed_at: string | null;
    reachable: boolean | null;
    federation_supported: boolean | null;
    latency_ms: number | null;
    samples: number;
    online_samples: number;
    avg_latency_ms: number | null;
    availability: GatewayV2AvailabilityBucket[];
    activity: {
      incoming_contracts: number; outgoing_contracts: number;
      incoming_msat: number; outgoing_msat: number;
      outgoing_claimed: number; outgoing_refunded: number; outgoing_cancelled: number;
    };

    attribution: 'self_reported' | 'contested' | 'none';
    contested_keys: number;
  }[];
}

export interface FederationSummary {
  id: string;
  name: string | null;
  last_7d_activity: FederationActivity[];
  deposits: number | null;
  /**
   * Estimated time of the first and last sessions this observer dated, in Unix
   * seconds; not announced start or shutdown dates. Only shut-down rows of a
   * page carry them, and older servers omit both
   */
  first_session_timestamp?: number | null;
  last_session_timestamp?: number | null;
  invite: string;
  health: FederationHealth;
  expiry_timestamp: number | null;

  successor: string | null;

  successor_id: string | null;

  successor_name?: string | null;
  /** Set by the single-federation summary only, so its header needs no config */
  module_kinds?: Record<string, string>;
  network?: string;
}


export interface ListPage {

  next_after: string | null;

  total: number;
}

export type FederationGroup = 'active' | 'offline' | 'shut_down';
export type FederationSortKey = 'assets' | 'volume' | 'transactions' | 'name';
export type SortDir = 'asc' | 'desc';

export interface FederationPageQuery {
  group: FederationGroup;
  search: string;
  sort: FederationSortKey;
  dir: SortDir;
  limit: number;
  after?: string;
}


export interface FederationPage {
  federations: FederationSummary[];
  next_after: string | null;
  total: number;

  activity_scale: { max_transactions: number; max_volume_msat: number };
}


export type NostrGroup = 'online' | 'offline' | 'unobserved';


export interface NostrFederationPage {
  federations: { id: string; invite: string; name: string | null; health: FederationHealth | null }[];
  next_after: string | null;
  total: number;
}

export interface FederationActivity {
  num_transactions: number;
  amount_transferred: number;
}


export type HistogramAverage = 0 | 7 | 30;


export interface FederationHistogramDay {

  date: string;
  count: number;
  volume_msat: number;

  avg_count: number | null;
  avg_volume_msat: number | null;

  outlier: boolean;
}


export interface FederationHistogram {
  days: FederationHistogramDay[];
  total_count: number;
  total_volume_msat: number;
}

export interface GuardianHealth {
  avg_uptime: number;
  avg_latency: number;
  software_version: string | null;
  latest: GuardianHealthLatest | null;
}

export interface GatewayV2AvailabilityBucket {

  start: string;
  samples: number;
  online_samples: number;
}

export interface GatewayV2Totals {
  registered: number;
  online: number;
  degraded: number;
  offline: number;
  unknown: number;
  removed: number;
  samples: number;
  online_samples: number;
  median_latency_ms: number | null;
}

export interface GuardianHealthLatest {
  block_height: number | null;
  block_outdated: boolean;
  session_count: number;
  session_outdated: boolean;
}


export interface ClientConfig {
  global?: {
    api_endpoints?: Record<string, { url?: string; name?: string }>;

    broadcast_public_keys?: Record<string, string>;
    meta?: Record<string, unknown>;
  };

  modules?: Record<string, { kind?: string; network?: string; finality_delay?: number }>;
}


export type UtxoView = 'all' | 'verified' | 'pending' | 'mismatch';


export interface UtxoStatusCounts {
  all: number;
  verified: number;
  pending: number;
  mismatch: number;
}

export interface FederationUtxosResponse {
  /** Guardians that must agree before an output counts as held or not held */
  threshold: number;
  guardians: GuardianUtxoReport[];

  utxos: UtxoComparisonRow[];

  counts: UtxoStatusCounts;

  total_rows: number;

  page: number;

  limit: number;

  page_count: number;
}

export interface GuardianUtxoReport {
  guardian_id: number;
  status: 'unavailable' | 'ok' | 'lagging' | 'error';
  session_count: number | null;
  error: string | null;

  missing_outputs: number;

  extra_outputs: number;

  wrong_amounts: number;
}

export interface UtxoComparisonRow {
  out_point: string;
  amount: number;
  address: string | null;

  guardian_states: Record<string, GuardianClaimedUtxoState>;
  status: 'mismatch' | 'pending' | 'verified';
  disagreement: 'evidence_mismatch' | 'inventory_difference' | 'observer_difference' | 'on_chain_conflict' | null;
  detail: string | null;
}

export type GuardianClaimedUtxoState =
  | 'spendable'
  | 'unsigned_peg_out'
  | 'unsigned_change'
  | 'unconfirmed_peg_out'
  | 'unconfirmed_change';

export interface GatewayInfo {
  gateway_id: string;
  node_pub_key: string;
  lightning_alias: string;
  api_endpoint: string;
  vetted: boolean;
  raw?: Record<string, unknown>;
  first_seen?: string;
  last_seen?: string;
  activity_7d?: GatewayActivityMetrics;
  activity_window?: GatewayActivityMetrics;
  uptime_window?: GatewayUptimeMetrics;
  metrics_window?: GatewayWindow;

  status?: GatewayStatus;

  coverage_pct?: number;

  estimated_uptime_pct?: number;
  estimated_online_minutes?: number;
  estimated_offline_minutes?: number;
  estimated_unknown_minutes?: number;
}

export type GatewayStatus = 'online' | 'degraded' | 'offline' | 'unknown' | 'retired';

export interface GatewayActivityMetrics {
  fund_count: number;
  settle_count: number;
  cancel_count: number;
  total_volume_msat: number;
}

export interface GatewayUptimeMetrics {
  sample_count: number;
  seen_samples: number;
  online_minutes: number;
  offline_minutes: number;
  uptime_pct: number;

  buckets?: GatewayUptimeBucket[];
}

export interface GatewayUptimeBucket {
  seen_samples: number;
  total_samples: number;
}

export interface GatewayUptimeTrendPoint {

  day: string;
  seen_samples: number;
  total_samples: number;
  uptime_pct: number;
}

export type GatewayWindow = '24h' | '7d' | '30d' | '90d';

export interface GatewayOverview {
  window: GatewayWindow;
  computed_at: string;
  gateways?: GatewayInfo[];
  uptime_trend?: GatewayUptimeTrendPoint[];

  summary: GatewayOverviewSummary;

  page?: ListPage;
}


export interface GatewayListQuery {

  include?: string;
  search: string;
  status: 'all' | 'online' | 'degraded' | 'offline' | 'unknown' | 'retired';
  sort: 'freshness' | 'status' | 'uptime' | 'activity';
  dir: SortDir;
  limit: number;
  after?: string;
}


export interface GatewayOverviewSummary {

  total: number;
  online: number;
  degraded: number;
  offline: number;
  unknown: number;
  retired: number;

  vetted: number;

  avg_uptime_pct: number | null;

  avg_coverage_pct: number | null;
}

export type FederationHealth = 'online' | 'degraded' | 'offline';


export type GuardianHealthWindow = '24h' | '7d' | '30d';


export interface GuardianHealthHistory {
  window: GuardianHealthWindow;

  bucket_seconds: number;

  start: string;

  bucket_count: number;
  generated_at: string;
  guardians: GuardianHealthSeries[];
}

export interface GuardianHealthSeries {
  guardian_id: number;

  uptime_pct: number | null;
  avg_latency_ms: number | null;

  last_online_hour: string | null;

  buckets: GuardianHealthBucket[];
}

export interface GuardianHealthBucket {
  start: string;
  samples: number;
  online_samples: number;
  polled_s: number;
  online_s: number;
  avg_latency_ms: number | null;
}


export interface FederationLiveHealth {
  generated_at: string;
  guardians: GuardianLiveStatus[];
}

export interface GuardianLiveStatus {
  guardian_id: number;

  polled_at: string | null;

  answered_at: string | null;
  latency_ms: number | null;
  session_count: number | null;
  peers_online: number | null;
  peers_offline: number | null;
  peers_flagged: number | null;
  scheduled_shutdown: number | null;

  peers: GuardianPeerView[];
}

export interface GuardianPeerView {
  peer_id: number;
  connected: boolean;
  flagged: boolean;
  last_contribution: number | null;
}

export type SeatBindingsStatus = 'not_observed' | 'not_published' | 'invalid' | 'verified';


export interface ManifoldSeatBindingsReport {
  has_meta_module: boolean;
  observed_at: string | null;
  status: SeatBindingsStatus;

  error: string | null;

  bindings: ManifoldSeatBinding[];
  distinct_guardians: number;
}

export interface ManifoldSeatBinding {
  peer_id: number;

  fman_pubkey: string;

  display_name: string;
  guardian_identity: string;
  guardian_fee_pubkey: string;

  issued_at: number;
}
