export interface FedimintTotals {
  federations: number;
  tx_volume: number;
  tx_count: number;
}

export interface FederationOverview {
  session_count: number;
  total_assets_msat: number;
  /** Sessions each module has processed, by module kind; behind session_count while it catches up */
  module_sessions: Record<string, number>;
}

export interface FederationSummary {
  id: string;
  name: string | null;
  last_7d_activity: FederationActivity[];
  deposits: number;
  invite: string;
  nostr_votes: FederationRating;
  health: FederationHealth;
}

export interface FederationRating {
  count: number;
  avg: number | null;
}

export interface FederationActivity {
  num_transactions: number;
  amount_transferred: number;
}

/** Activity per calendar day, keyed by date (e.g. "2024-05-31"), oldest first */
export type FederationHistogram = Record<string, FederationActivity>;

export interface GuardianHealth {
  avg_uptime: number;
  avg_latency: number;
  software_version: string | null;
  latest: GuardianHealthLatest | null;
}

export interface GuardianHealthLatest {
  block_height: number;
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

export interface FederationUtxosResponse {
  /** Guardians that must agree before an output counts as held or not held */
  threshold: number;
  guardians: GuardianUtxoReport[];
  /** Most severe status first, then largest amount first */
  utxos: UtxoComparisonRow[];
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
}

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
}

export interface GatewayUptimeTrendPoint {
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
}

export type FederationHealth = 'online' | 'degraded' | 'offline';
