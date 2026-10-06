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
  protocols?: GatewayProtocol[];
  lnv2?: Lnv2GatewayInfo;
}

export type GatewayProtocol = 'lnv1' | 'lnv2';

// 'ok' only means the gateway advertises routing info for this federation,
// not that payments through it succeed.
export type Lnv2RoutingStatus = 'ok' | 'not_serving' | 'unreachable';

export interface Lnv2GatewayInfo {
  api_endpoint: string;
  routing_status: Lnv2RoutingStatus;
  routing_checked_at?: string;
  lightning_public_key?: string;
  module_public_key?: string;
  send_fee_minimum?: GatewayFee;
  send_fee_default?: GatewayFee;
  receive_fee?: GatewayFee;
  uptime_window?: GatewayUptimeMetrics;
}

export interface GatewayFee {
  base_msat: number;
  parts_per_million: number;
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

export type GatewayWindow = '1h' | '24h' | '7d' | '30d' | '90d';

export type FederationHealth = 'online' | 'degraded' | 'offline';
