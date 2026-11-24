import type { GatewayInfo, GatewayStatus, GatewayUptimeBucket, GatewayWindow } from '../../../types/api';

export type UptimeStripStatus = 'online' | 'degraded' | 'offline' | 'unknown';

export const GATEWAY_WINDOWS: readonly GatewayWindow[] = ['24h', '7d', '30d', '90d'];
export const GATEWAY_FILTERS = ['all', 'online', 'degraded', 'offline', 'unknown', 'retired'] as const;
export type GatewayFilter = (typeof GATEWAY_FILTERS)[number];
export const GATEWAY_SORTS = ['freshness', 'status', 'uptime', 'activity'] as const;
export type GatewaySort = (typeof GATEWAY_SORTS)[number];
export const SORT_DIRECTIONS = ['asc', 'desc'] as const;
export type SortDirection = (typeof SORT_DIRECTIONS)[number];

export const DEFAULT_WINDOW: GatewayWindow = '7d';
export const WINDOW_MINUTES: Record<GatewayWindow, number> = {
  '24h': 24 * 60,
  '7d': 7 * 24 * 60,
  '30d': 30 * 24 * 60,
  '90d': 90 * 24 * 60,
};
export const UPTIME_SEGMENTS = 30;

const RETIRED_AFTER_MINUTES = 7 * 24 * 60;

export interface GatewayWithStatus extends GatewayInfo {
  firstSeenDate: Date | null;
  lastSeenDate: Date | null;
  status: GatewayStatus;
  minutesSinceLastSeen: number | null;
  estimatedOfflineMinutes: number;
  estimatedOnlineMinutes: number;
  estimatedUnknownMinutes: number;
  estimatedUptimePct: number;
  coveragePct: number;
  realActivityScore: number | null;
  fundCountWindow: number;
  settleCountWindow: number;
  cancelCountWindow: number;
  totalVolumeMsatWindow: number;
  searchText: string;
}

function parseTimestamp(value?: string): Date | null {
  if (!value) return null;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

export function getGatewayStatus(lastSeen: Date | null, now: number): GatewayStatus {
  if (!lastSeen) return 'unknown';
  const minutes = (now - lastSeen.getTime()) / (1000 * 60);
  if (minutes <= 15) return 'online';
  if (minutes <= 30) return 'degraded';
  if (minutes > RETIRED_AFTER_MINUTES) return 'retired';
  return 'offline';
}

export function formatRelative(date: Date | null, now: number): string {
  if (!date) return 'Never seen';

  const diffMs = now - date.getTime();
  const minutes = Math.floor(diffMs / (1000 * 60));
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function formatCompactDuration(minutes: number): string {
  const safe = Math.max(0, Math.round(minutes));
  if (safe >= 60 * 24) return `${Math.round(safe / (60 * 24))}d`;
  if (safe >= 60) return `${Math.round(safe / 60)}h`;
  if (safe === 0) return '0m';
  return `${safe}m`;
}

export function statusClasses(status: GatewayStatus): string {
  switch (status) {
    case 'online':
      return 'bg-ok-soft text-ok-ink';
    case 'degraded':
      return 'bg-warn-soft text-warn-ink';
    case 'offline':
      return 'bg-crit-soft text-crit-ink';
    default:
      return 'bg-surface-3 text-ink-2';
  }
}

export function getUptimeStripClass(status: UptimeStripStatus): string {
  switch (status) {
    case 'online':
      return 'bg-ok';
    case 'degraded':
      return 'bg-warn';
    case 'offline':
      return 'bg-crit';
    default:
      return 'bg-line-strong';
  }
}

/** The polls recorded in each slot of the window, when the server sends them */
export function uptimeBuckets(gateway: GatewayInfo): GatewayUptimeBucket[] | null {
  const buckets = gateway.uptime_window?.buckets;
  return buckets && buckets.length === UPTIME_SEGMENTS ? buckets : null;
}

function bucketStatus(bucket: GatewayUptimeBucket): UptimeStripStatus {
  if (bucket.total_samples === 0) return 'unknown';
  if (bucket.seen_samples === bucket.total_samples) return 'online';
  return bucket.seen_samples === 0 ? 'offline' : 'degraded';
}

export function buildUptimeStrip(gateway: GatewayInfo): UptimeStripStatus[] {
  const buckets = uptimeBuckets(gateway);
  return buckets ? buckets.map(bucketStatus) : Array.from({ length: UPTIME_SEGMENTS }, () => 'unknown');
}

function getUptimeBucketLabel(bucketIndex: number, totalBuckets: number, windowMinutes: number): string {
  const minutesPerBucket = windowMinutes / totalBuckets;
  const newestEndMinutes = (totalBuckets - bucketIndex) * minutesPerBucket;
  const newestStartMinutes = (totalBuckets - bucketIndex - 1) * minutesPerBucket;

  const start = formatCompactDuration(newestStartMinutes);
  const end = formatCompactDuration(newestEndMinutes);
  return `${start}–${end} ago`;
}

export function getUptimeBucketTooltip(
  bucketIndex: number,
  totalBuckets: number,
  windowMinutes: number,
  bucket?: GatewayUptimeBucket,
): string {
  const label = `Bucket ${bucketIndex + 1} of ${totalBuckets} · ${getUptimeBucketLabel(
    bucketIndex,
    totalBuckets,
    windowMinutes,
  )}`;
  if (!bucket) return `${label} · No poll history`;
  return bucket.total_samples === 0
    ? `${label} · No polls recorded`
    : `${label} · Seen in ${bucket.seen_samples} of ${bucket.total_samples} polls`;
}

/** Where a strip segment's tooltip anchors; on phones the strip spans the whole card, so more anchor to an edge */
export function uptimeTooltipPosition(index: number): string {
  return index < 4
    ? 'left-0'
    : index > 25
      ? 'right-0'
      : index < 10
        ? 'left-0 sm:left-1/2 sm:-translate-x-1/2'
        : index > 19
          ? 'right-0 sm:right-auto sm:left-1/2 sm:-translate-x-1/2'
          : 'left-1/2 -translate-x-1/2';
}

/** One of `options` matching `value` regardless of case, or null */
export function parseOption<T extends string>(options: readonly T[], value: string | null): T | null {
  const lower = value?.toLowerCase();
  return options.find((option) => option === lower) ?? null;
}

export function defaultDirection(sort: GatewaySort): SortDirection {
  return sort === 'status' ? 'asc' : 'desc';
}

/** A gateway with the figures the page shows, as of `now` */
export function toGatewayRow(gateway: GatewayInfo, now: number, windowMinutes: number): GatewayWithStatus {
  const firstSeenDate = parseTimestamp(gateway.first_seen);
  const lastSeenDate = parseTimestamp(gateway.last_seen);
  const status = lastSeenDate ? getGatewayStatus(lastSeenDate, now) : gateway.status ?? 'unknown';
  const minutesSinceLastSeen = lastSeenDate
    ? (now - lastSeenDate.getTime()) / (1000 * 60)
    : null;
  const estimatedOnlineMinutes = gateway.estimated_online_minutes ?? 0;
  const estimatedOfflineMinutes = gateway.estimated_offline_minutes ?? 0;
  const estimatedUnknownMinutes = gateway.estimated_unknown_minutes ?? windowMinutes;
  const estimatedUptimePct = gateway.estimated_uptime_pct ?? 0;
  const coveragePct = gateway.coverage_pct ?? 0;
  const activityWindow = gateway.activity_window ?? gateway.activity_7d;
  const fundCountWindow = activityWindow?.fund_count ?? 0;
  const settleCountWindow = activityWindow?.settle_count ?? 0;
  const cancelCountWindow = activityWindow?.cancel_count ?? 0;
  const totalVolumeMsatWindow = activityWindow?.total_volume_msat ?? 0;
  const hasRealActivity = Boolean(activityWindow);
  const realActivityScore = hasRealActivity
    ? Math.max(
        0,
        Math.round(
          (fundCountWindow * 1.0)
            + (settleCountWindow * 3.0)
            + (0.5 * Math.log1p(totalVolumeMsatWindow / 1_000_000))
            - (cancelCountWindow * 1.5),
        ),
      )
    : null;
  const searchText = [
    gateway.lightning_alias,
    gateway.gateway_id,
    gateway.node_pub_key,
    gateway.api_endpoint,
  ].join(' ').toLowerCase();

  return {
    ...gateway,
    firstSeenDate,
    lastSeenDate,
    status,
    minutesSinceLastSeen,
    estimatedOfflineMinutes,
    estimatedOnlineMinutes,
    estimatedUnknownMinutes,
    estimatedUptimePct,
    coveragePct,
    realActivityScore,
    fundCountWindow,
    settleCountWindow,
    cancelCountWindow,
    totalVolumeMsatWindow,
    searchText,
  };
}
