export interface FedimintTotals {
  federations: number;
  tx_volume: number;
  tx_count: number;
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

export interface FederationUtxo {
  address: string;
  out_point: string;
  amount: number;
}

export interface FederationUtxosResponse {
  observed: FederationUtxo[];
  guardian_claims: GuardianUtxoClaim[];
  disagreements: GuardianUtxoDisagreement[];
}

export interface GuardianUtxoClaim {
  guardian_id: number;
  status: 'unavailable' | 'ok' | 'error';
  utxos: GuardianClaimedUtxo[];
  error: string | null;
}

export interface GuardianClaimedUtxo {
  out_point: string;
  amount: number;
  state: GuardianClaimedUtxoState;
  onchain?: GuardianClaimedUtxoOnchain;
  resolution_error?: string;
}

export interface GuardianClaimedUtxoOnchain {
  script_pubkey: string;
  address: string | null;
  amount: number;
  confirmed: boolean;
  block_height: number | null;
}

export type GuardianClaimedUtxoState =
  | 'spendable'
  | 'unsigned_peg_out'
  | 'unsigned_change'
  | 'unconfirmed_peg_out'
  | 'unconfirmed_change';

export interface GuardianUtxoDisagreement {
  out_point: string;
  description: string;
}

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

export type GatewayWindow = '1h' | '24h' | '7d' | '30d' | '90d';

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

export type FederationHealth = 'online' | 'degraded' | 'offline';

export interface NavItem {
  name: string;
  href: string;
  active: boolean;
}
