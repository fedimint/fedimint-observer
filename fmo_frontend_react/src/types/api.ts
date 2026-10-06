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

export type ProbeOutcome =
  | 'success'
  | 'insufficient_liquidity'
  | 'unreachable'
  | 'route_failure'
  | 'no_route'
  | 'local_failure'
  | 'timeout'
  | 'error';

export interface GatewayProbeResult {
  node_pub_key: string;
  probe_time: string;
  amount_msat: number;
  outcome: ProbeOutcome;
  latency_ms?: number;
  failure_code?: string;
  failure_source_index?: number;
  route_hops?: number;
  route_fee_msat?: number;
}

export interface GatewayProbeSummary {
  window: GatewayWindow;
  /** Conclusive probes at the base amount */
  conclusive_probes: number;
  successes: number;
  /** Routing success rate at the base amount */
  success_rate_pct: number | null;
  inconclusive_probes: number;
  outcome_counts: Partial<Record<ProbeOutcome, number>>;
  last_probe_time: string | null;
  base_amount_msat: number | null;
  base_amount_last_outcome: ProbeOutcome | null;
  latency_p50_ms: number | null;
  latency_p95_ms: number | null;
  max_successful_amount_msat: number | null;
}

export interface GatewayProbeTrendPoint {
  bucket: string;
  conclusive_probes: number;
  successes: number;
  success_rate_pct: number | null;
  latency_p50_ms: number | null;
}

export interface GatewayProbeReport {
  node_pub_key: string;
  summary: GatewayProbeSummary;
  trend: GatewayProbeTrendPoint[];
  recent: GatewayProbeResult[];
}

export type FederationHealth = 'online' | 'degraded' | 'offline';
