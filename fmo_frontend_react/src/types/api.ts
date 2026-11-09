export interface FedimintTotals {
  federations: number;
  tx_volume: number;
  tx_count: number;
}

export interface FederationOverview {
  session_count: number;
  total_assets_msat: number | null;
  /** Sessions each module has processed, by module kind; behind session_count while it catches up */
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
  /** Over every gateway of the module instance, not just this page */
  totals: GatewayV2Totals;
  /** Probes of every gateway of the module instance, in the same UTC buckets as each gateway's `availability` */
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
    /** Module keys are self-reported and unverified; contested keys credit no endpoint */
    attribution: 'self_reported' | 'contested' | 'none';
    contested_keys: number;
  }[];
}

export interface FederationSummary {
  id: string;
  name: string | null;
  last_7d_activity: FederationActivity[];
  deposits: number | null;
  invite: string;
  nostr_votes: FederationRating;
  health: FederationHealth;
  expiry_timestamp: number | null;
  /** `federation_successor` meta field as published, normally an invite code */
  successor: string | null;
  /** Federation the successor invite code points to, when it parses as one */
  successor_id: string | null;
}

export interface FederationRating {
  count: number;
  avg: number | null;
}

export interface FederationActivity {
  num_transactions: number;
  amount_transferred: number;
}

/** Days a moving average spans in the histogram; 0 is none */
export type HistogramAverage = 0 | 7 | 30;

/** One UTC day of activity */
export interface FederationHistogramDay {
  /** UTC calendar day, e.g. "2024-05-31" */
  date: string;
  count: number;
  volume_msat: number;
  /** Trailing averages over `average` days; null when no average was asked for */
  avg_count: number | null;
  avg_volume_msat: number | null;
  /** Volume far above the federation's usual days */
  outlier: boolean;
}

/** Every UTC day from the first activity through today, quiet days included, oldest first */
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
  /** RFC 3339, UTC */
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

/**
 * Fedimint's JsonClientConfig, which is not part of fmo_api_types. Only the
 * fields the pages read are listed; the rest is still there and shown as is.
 */
export interface ClientConfig {
  global?: {
    api_endpoints?: Record<string, { url?: string }>;
    meta?: Record<string, unknown>;
  };
  /** Keyed by module instance id */
  modules?: Record<string, { kind?: string; network?: string; finality_delay?: number }>;
}

/** Which rows `/federations/:id/utxos?view=` lists */
export type UtxoView = 'all' | 'verified' | 'pending' | 'mismatch';

/** How many outputs have each status */
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
  /** The requested page of the rows matching the view and search; most severe status first, then largest amount first */
  utxos: UtxoComparisonRow[];
  /** Rows per status over every output, before view and search */
  counts: UtxoStatusCounts;
  /** Rows matching the view and search, across all pages */
  total_rows: number;
  /** The 1-based page served: the requested one, clamped to the last page */
  page: number;
  /** Rows per page */
  limit: number;
  /** Pages of rows matching the view and search, at least 1 */
  page_count: number;
}

export interface GuardianUtxoReport {
  guardian_id: number;
  status: 'unavailable' | 'ok' | 'lagging' | 'error';
  session_count: number | null;
  error: string | null;
  /** Outputs a threshold of guardians agrees are held that this guardian does not list */
  missing_outputs: number;
  /** Outputs this guardian lists that a threshold of guardians agrees are not held */
  extra_outputs: number;
  /** Outputs this guardian lists with another amount than the agreed one */
  wrong_amounts: number;
}

export interface UtxoComparisonRow {
  out_point: string;
  amount: number;
  address: string | null;
  /** Keyed by guardian id */
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
  /** Server-computed availability for the window; missing from live registry lookups */
  status?: GatewayStatus;
  /** Share of the window covered by polls */
  coverage_pct?: number;
  /** Online share of the polled time */
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
  /** The window in equal time slots, oldest first; missing from older servers */
  buckets?: GatewayUptimeBucket[];
}

export interface GatewayUptimeBucket {
  seen_samples: number;
  total_samples: number;
}

export interface GatewayUptimeTrendPoint {
  /** UTC midnight of the day, e.g. "2026-11-07T00:00:00Z" */
  day: string;
  seen_samples: number;
  total_samples: number;
  uptime_pct: number;
}

export type GatewayWindow = '24h' | '7d' | '30d' | '90d';

// Gateways and their trend for one window, computed together on the server
export interface GatewayOverview {
  window: GatewayWindow;
  computed_at: string;
  // Both are present unless the request narrows them with ?include=
  gateways?: GatewayInfo[];
  uptime_trend?: GatewayUptimeTrendPoint[];
  /** Over all the federation's gateways, whatever ?include= asks for */
  summary: GatewayOverviewSummary;
}

/** A federation's gateways summed up for the overview's window; retired ones count only in `retired` */
export interface GatewayOverviewSummary {
  /** Gateways that aren't retired */
  total: number;
  online: number;
  degraded: number;
  offline: number;
  unknown: number;
  retired: number;
  /** Of the gateways that aren't retired */
  vetted: number;
  /** Mean estimated_uptime_pct of non-retired gateways polled in the window */
  avg_uptime_pct: number | null;
  /** Mean coverage_pct of non-retired gateways */
  avg_coverage_pct: number | null;
}

export type FederationHealth = 'online' | 'degraded' | 'offline';
