import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { api } from '../services/api';
import type {
  FederationSummary,
  GatewayInfo,
  GatewayUptimeTrendPoint,
  GatewayWindow,
} from '../types/api';
import { GatewayWarningPage, type GatewayWarningState } from '../components/GatewayWarningPage';
import { formatMsats, formatRelative, shortId } from '../utils/format';

type GatewayStatus = 'online' | 'degraded' | 'offline' | 'unknown';
type UptimeStripStatus = 'online' | 'degraded' | 'offline' | 'unknown';
type GatewayFilter = 'all' | GatewayStatus;
type GatewaySort = 'freshness' | 'status' | 'uptime' | 'activity';
type SortDirection = 'asc' | 'desc';

const UptimeTrendChart = lazy(() => import('echarts-for-react'));
const INITIAL_RENDER_COUNT = 50;
const STATUS_RANK: Record<GatewayStatus, number> = {
  offline: 0,
  degraded: 1,
  unknown: 2,
  online: 3,
};

interface GatewayWithStatus extends GatewayInfo {
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

function getGatewayStatus(lastSeen: Date | null): GatewayStatus {
  if (!lastSeen) return 'unknown';
  const minutes = (Date.now() - lastSeen.getTime()) / (1000 * 60);
  if (minutes <= 10) return 'online';
  if (minutes <= 30) return 'degraded';
  return 'offline';
}

function formatDateTime(date: Date | null): string {
  if (!date) return 'N/A';
  return date.toLocaleString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function formatCompactDuration(minutes: number): string {
  const safe = Math.max(0, Math.round(minutes));
  if (safe >= 60 * 24) return `${Math.round(safe / (60 * 24))}d`;
  if (safe >= 60) return `${Math.round(safe / 60)}h`;
  if (safe === 0) return '0m';
  return `${safe}m`;
}

function statusClasses(status: GatewayStatus): string {
  switch (status) {
    case 'online':
      return 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300';
    case 'degraded':
      return 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300';
    case 'offline':
      return 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300';
    default:
      return 'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300';
  }
}

function getUptimeStripClass(status: UptimeStripStatus): string {
  switch (status) {
    case 'online':
      return 'bg-green-500 dark:bg-green-400';
    case 'degraded':
      return 'bg-yellow-500 dark:bg-yellow-400';
    case 'offline':
      return 'bg-red-500 dark:bg-red-400';
    default:
      return 'bg-gray-300 dark:bg-gray-600';
  }
}

function buildUptimeStrip(gateway: GatewayWithStatus, windowMinutes: number): UptimeStripStatus[] {
  const segments = 30;

  if (gateway.status === 'unknown' || windowMinutes <= 0) {
    return Array.from({ length: segments }, () => 'unknown');
  }

  const strip: UptimeStripStatus[] = Array.from({ length: segments }, () => 'unknown');
  const unknownMinutes = Math.max(0, Math.min(windowMinutes, gateway.estimatedUnknownMinutes));
  const unknownSegments = Math.max(
    0,
    Math.min(segments, Math.round((unknownMinutes / Math.max(1, windowMinutes)) * segments)),
  );
  const observedSegments = Math.max(0, segments - unknownSegments);
  const observedStart = unknownSegments;

  for (let idx = observedStart; idx < segments; idx += 1) {
    strip[idx] = 'online';
  }

  if (observedSegments === 0) {
    return strip;
  }

  const sampledMinutes = Math.max(
    1,
    gateway.estimatedOnlineMinutes + gateway.estimatedOfflineMinutes,
  );
  const offlineMinutes = Math.max(0, gateway.estimatedOfflineMinutes);
  const offlineSegments = Math.max(
    0,
    Math.min(observedSegments, Math.round((offlineMinutes / sampledMinutes) * observedSegments)),
  );
  const offlineStatus: UptimeStripStatus = gateway.status === 'degraded' ? 'degraded' : 'offline';

  for (let idx = segments - 1; idx >= segments - offlineSegments; idx -= 1) {
    if (idx >= 0) strip[idx] = offlineStatus;
  }

  if (gateway.status === 'degraded') {
    strip[segments - 1] = 'degraded';
  } else if (gateway.status === 'offline') {
    strip[segments - 1] = 'offline';
  } else {
    strip[segments - 1] = 'online';
  }

  return strip;
}

function getUptimeBucketLabel(
  bucketIndex: number,
  totalBuckets: number,
  windowMinutes: number,
): string {
  const minutesPerBucket = windowMinutes / totalBuckets;
  const newestEndMinutes = (totalBuckets - bucketIndex) * minutesPerBucket;
  const newestStartMinutes = (totalBuckets - bucketIndex - 1) * minutesPerBucket;

  const start = formatCompactDuration(newestStartMinutes);
  const end = formatCompactDuration(newestEndMinutes);
  return `${start}–${end} ago`;
}

function getUptimeBucketTooltip(
  status: UptimeStripStatus,
  bucketIndex: number,
  totalBuckets: number,
  windowMinutes: number,
): string {
  return `Bucket ${bucketIndex + 1} of ${totalBuckets} · ${getUptimeBucketLabel(
    bucketIndex,
    totalBuckets,
    windowMinutes,
  )} · Estimated ${status}`;
}

function formatEndpointLabel(endpoint: string): string {
  try {
    const url = new URL(endpoint);
    if (url.protocol === 'iroh:') return `iroh://${shortId(url.host)}`;
    return `${url.host}${url.pathname === '/' ? '' : url.pathname}`;
  } catch {
    return shortId(endpoint);
  }
}

function mergeGatewayData(observedGateways: GatewayInfo[], liveGateways: GatewayInfo[]): GatewayInfo[] {
  if (observedGateways.length === 0) return liveGateways;
  if (liveGateways.length === 0) return observedGateways;

  const observedById = new Map(observedGateways.map((gateway) => [gateway.gateway_id, gateway] as const));
  const liveIds = new Set(liveGateways.map((gateway) => gateway.gateway_id));

  const merged = liveGateways.map((liveGateway) => {
    const observedGateway = observedById.get(liveGateway.gateway_id);
    if (!observedGateway) return liveGateway;

    return {
      ...liveGateway,
      lightning_alias: liveGateway.lightning_alias || observedGateway.lightning_alias,
      api_endpoint: liveGateway.api_endpoint || observedGateway.api_endpoint,
      node_pub_key: liveGateway.node_pub_key || observedGateway.node_pub_key,
      vetted: liveGateway.vetted || observedGateway.vetted,
      raw: liveGateway.raw ?? observedGateway.raw,
      first_seen: observedGateway.first_seen ?? liveGateway.first_seen,
      last_seen: observedGateway.last_seen ?? liveGateway.last_seen,
      activity_7d: observedGateway.activity_7d ?? liveGateway.activity_7d,
      activity_window: observedGateway.activity_window ?? liveGateway.activity_window,
      uptime_window: observedGateway.uptime_window ?? liveGateway.uptime_window,
      metrics_window: observedGateway.metrics_window ?? liveGateway.metrics_window,
    };
  });

  for (const observedGateway of observedGateways) {
    if (!liveIds.has(observedGateway.gateway_id)) {
      merged.push(observedGateway);
    }
  }

  return merged;
}

interface GatewaySelection {
  gateways: GatewayInfo[];
  warning: GatewayWarningState | null;
}

interface RawAnnouncementDialog {
  gatewayName: string;
  raw: Record<string, unknown>;
}

function selectGatewayData(
  observedGateways: GatewayInfo[],
  liveGateways: GatewayInfo[],
  observedError: string | null,
  liveError: string | null,
  hasInvite: boolean,
): GatewaySelection {
  if (liveGateways.length > 0) {
    if (observedGateways.length === 0) {
      return {
        gateways: liveGateways,
        warning: {
          level: 'info',
          title: 'Live Gateway Data Only',
          message: 'Showing gateways from invite-based live discovery.',
          detail: 'Observed gateway history is unavailable on this backend.',
        },
      };
    }

    return {
      gateways: mergeGatewayData(observedGateways, liveGateways),
      warning: {
        level: 'info',
        title: 'Merged Gateway Sources',
        message: 'Combined observed history with live invite-based gateway metadata.',
        detail: 'Live data provides the latest registry details, while observed data keeps status and activity context.',
      },
    };
  }

  if (observedGateways.length > 0) {
    if (liveError) {
      return {
        gateways: observedGateways,
        warning: {
          level: 'warning',
          title: 'Live Lookup Failed',
          message: 'Showing observed gateway data from the backend.',
          detail: `Live invite lookup error: ${liveError}`,
        },
      };
    }

    if (hasInvite) {
      return {
        gateways: observedGateways,
        warning: {
          level: 'warning',
          title: 'Live Lookup Returned No Gateways',
          message: 'Showing observed gateway data from the backend.',
          detail: 'Invite-based lookup returned an empty gateway list.',
        },
      };
    }

    return { gateways: observedGateways, warning: null };
  }

  const reason = observedError ?? 'No gateway data available on the configured API backend.';
  if (liveError) {
    return {
      gateways: [],
      warning: {
        level: 'error',
        title: 'Gateway Data Unavailable',
        message: 'Could not load gateway data from backend or invite-based live lookup.',
        detail: `Backend: ${reason}. Live: ${liveError}`,
      },
    };
  }

  if (hasInvite) {
    return {
      gateways: [],
      warning: {
        level: 'warning',
        title: 'No Gateways Returned',
        message: 'Both backend and invite-based lookup returned no gateway records.',
        detail: 'This can happen for new federations or backends with incomplete gateway ingestion.',
      },
    };
  }

  return {
    gateways: [],
    warning: {
      level: 'warning',
      title: 'Gateway Data Unavailable',
      message: 'The configured backend did not return any gateway data.',
      detail: reason,
    },
  };
}

export function FederationGateways() {
  const { id } = useParams<{ id: string }>();
  const [federation, setFederation] = useState<FederationSummary | null>(null);
  const [gateways, setGateways] = useState<GatewayInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [windowLoading, setWindowLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [gatewayWarning, setGatewayWarning] = useState<GatewayWarningState | null>(null);
  const [timeWindow, setTimeWindow] = useState<GatewayWindow>('7d');
  const [uptimeTrend, setUptimeTrend] = useState<GatewayUptimeTrendPoint[]>([]);
  const [uptimeTrendLoading, setUptimeTrendLoading] = useState(true);
  const [gatewayFilter, setGatewayFilter] = useState<GatewayFilter>('all');
  const [gatewaySort, setGatewaySort] = useState<GatewaySort>('freshness');
  const [sortDirection, setSortDirection] = useState<SortDirection>('desc');
  const [gatewaySearch, setGatewaySearch] = useState('');
  const [visibleGatewayCount, setVisibleGatewayCount] = useState(INITIAL_RENDER_COUNT);
  const [rawAnnouncement, setRawAnnouncement] = useState<RawAnnouncementDialog | null>(null);
  const hasLoadedOnce = useRef(false);
  const requestSeq = useRef(0);
  const federationCache = useRef<Map<string, FederationSummary | null>>(new Map());
  const liveGatewayCache = useRef<Map<string, { gateways: GatewayInfo[]; error: string | null }>>(new Map());

  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    const currentRequest = ++requestSeq.current;

    if (!hasLoadedOnce.current) {
      setLoading(true);
    } else {
      setWindowLoading(true);
    }
    setError(null);
    setGatewayWarning(null);
    setUptimeTrend([]);
    setUptimeTrendLoading(true);

    (async () => {
      try {
        void api.getFederationGatewayUptimeTrend(id, timeWindow)
          .then((trend) => {
            if (!cancelled && currentRequest === requestSeq.current) setUptimeTrend(trend);
          })
          .catch(() => {
            if (!cancelled && currentRequest === requestSeq.current) setUptimeTrend([]);
          })
          .finally(() => {
            if (!cancelled && currentRequest === requestSeq.current) setUptimeTrendLoading(false);
          });
        let fed: FederationSummary | null | undefined = federationCache.current.get(id);
        if (fed === undefined) {
          const federations = await api.getFederations();
          fed = federations.find((item) => item.id === id) || null;
          federationCache.current.set(id, fed);
        }
        if (cancelled || currentRequest !== requestSeq.current) return;

        setFederation(fed ?? null);

        let observedGateways: GatewayInfo[] = [];
        let observedError: string | null = null;
        try {
          observedGateways = await api.getFederationGateways(id, timeWindow);
        } catch (observedErr: unknown) {
          observedError =
            observedErr instanceof Error
              ? observedErr.message
              : `Failed to fetch gateways for federation ${id}`;
        }
        if (cancelled || currentRequest !== requestSeq.current) return;

        let liveGateways: GatewayInfo[] = [];
        let liveError: string | null = null;
        if (fed?.invite) {
          const cachedLive = liveGatewayCache.current.get(fed.invite);
          if (cachedLive) {
            liveGateways = cachedLive.gateways;
            liveError = cachedLive.error;
          } else {
            try {
              liveGateways = await api.getFederationGatewaysByInvite(fed.invite);
            } catch (liveErr: unknown) {
              liveError =
                liveErr instanceof Error
                  ? liveErr.message
                  : 'Invite-based gateway lookup failed.';
            }
            liveGatewayCache.current.set(fed.invite, {
              gateways: liveGateways,
              error: liveError,
            });
          }
        }
        if (cancelled || currentRequest !== requestSeq.current) return;

        const selection = selectGatewayData(
          observedGateways,
          liveGateways,
          observedError,
          liveError,
          Boolean(fed?.invite),
        );
        setGateways(selection.gateways);
        setGatewayWarning(selection.warning);
      } catch (err: unknown) {
        const message = err instanceof Error ? err.message : 'Failed to load gateways';
        if (!hasLoadedOnce.current) {
          setError(message);
        } else {
          setGatewayWarning({
            level: 'warning',
            title: 'Refresh Failed',
            message: 'Failed to refresh the selected time window.',
            detail: message,
          });
        }
      } finally {
        if (!cancelled && currentRequest === requestSeq.current) {
          if (!hasLoadedOnce.current) {
            setLoading(false);
            hasLoadedOnce.current = true;
          }
          setWindowLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [id, timeWindow]);

  const windowMinutes = useMemo(() => {
    switch (timeWindow) {
      case '1h':
        return 60;
      case '24h':
        return 24 * 60;
      case '7d':
        return 7 * 24 * 60;
      case '30d':
        return 30 * 24 * 60;
      case '90d':
      default:
        return 90 * 24 * 60;
    }
  }, [timeWindow]);

  const rows = useMemo<GatewayWithStatus[]>(() => {
    return gateways
      .map((gateway) => {
        const firstSeenDate = parseTimestamp(gateway.first_seen);
        const lastSeenDate = parseTimestamp(gateway.last_seen);
        const status = getGatewayStatus(lastSeenDate);
        const minutesSinceLastSeen = lastSeenDate
          ? (Date.now() - lastSeenDate.getTime()) / (1000 * 60)
          : null;
        const observedUptime = gateway.uptime_window;
        const hasObservedSamples = Boolean(observedUptime && observedUptime.sample_count > 0);
        const rawObservedOnlineMinutes = hasObservedSamples ? (observedUptime?.online_minutes ?? 0) : 0;
        const rawObservedOfflineMinutes = hasObservedSamples ? (observedUptime?.offline_minutes ?? 0) : 0;
        const rawObservedTotalMinutes = rawObservedOnlineMinutes + rawObservedOfflineMinutes;
        const clampScale = rawObservedTotalMinutes > windowMinutes
          ? windowMinutes / rawObservedTotalMinutes
          : 1;
        const estimatedOnlineMinutes = rawObservedOnlineMinutes * clampScale;
        const observedOfflineMinutes = rawObservedOfflineMinutes * clampScale;
        const sampledMinutes = estimatedOnlineMinutes + observedOfflineMinutes;
        const baseUnknownMinutes = Math.max(0, windowMinutes - sampledMinutes);
        const inferredOfflineFromRecency = status === 'offline' && minutesSinceLastSeen !== null
          ? Math.max(0, Math.min(baseUnknownMinutes, minutesSinceLastSeen))
          : 0;
        const estimatedOfflineMinutes = observedOfflineMinutes + inferredOfflineFromRecency;
        const estimatedUnknownMinutes = Math.max(0, baseUnknownMinutes - inferredOfflineFromRecency);
        const estimatedUptimePct = sampledMinutes > 0
          ? (estimatedOnlineMinutes / sampledMinutes) * 100
          : 0;
        const coveragePct = windowMinutes > 0
          ? (sampledMinutes / windowMinutes) * 100
          : 0;
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
      });
  }, [gateways, windowMinutes]);

  const totals = useMemo(() => {
    const total = rows.length;
    const online = rows.filter((row) => row.status === 'online').length;
    const degraded = rows.filter((row) => row.status === 'degraded').length;
    const offline = rows.filter((row) => row.status === 'offline').length;
    const unknown = total - online - degraded - offline;
    const vetted = rows.filter((row) => row.vetted).length;

    return { total, online, degraded, offline, unknown, vetted };
  }, [rows]);

  const avgUptime = useMemo(() => {
    const observedRows = rows.filter((row) => row.coveragePct > 0);
    if (observedRows.length === 0) return 0;
    const total = observedRows.reduce((sum, row) => sum + row.estimatedUptimePct, 0);
    return total / observedRows.length;
  }, [rows]);

  const avgCoverage = useMemo(() => {
    if (rows.length === 0) return 0;
    const total = rows.reduce((sum, row) => sum + row.coveragePct, 0);
    return total / rows.length;
  }, [rows]);

  const filteredRows = useMemo(() => {
    const query = gatewaySearch.trim().toLowerCase();
    const filtered = rows.filter((row) => (
      (gatewayFilter === 'all' || row.status === gatewayFilter)
      && (!query || row.searchText.includes(query))
    ));

    return filtered.sort((left, right) => {
      let comparison: number;
      switch (gatewaySort) {
        case 'status':
          comparison = STATUS_RANK[left.status] - STATUS_RANK[right.status]
            || (left.minutesSinceLastSeen ?? 0) - (right.minutesSinceLastSeen ?? 0);
          break;
        case 'uptime':
          comparison = left.estimatedUptimePct - right.estimatedUptimePct;
          break;
        case 'activity':
          comparison = (left.realActivityScore ?? -1) - (right.realActivityScore ?? -1);
          break;
        case 'freshness':
        default:
          comparison = (left.lastSeenDate?.getTime() ?? 0) - (right.lastSeenDate?.getTime() ?? 0);
      }
      return sortDirection === 'asc' ? comparison : -comparison;
    });
  }, [gatewayFilter, gatewaySearch, gatewaySort, rows, sortDirection]);

  useEffect(() => {
    setVisibleGatewayCount(INITIAL_RENDER_COUNT);
  }, [gatewayFilter, gatewaySearch, gatewaySort, id, sortDirection]);

  useEffect(() => {
    if (!rawAnnouncement) return undefined;

    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setRawAnnouncement(null);
    };
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, [rawAnnouncement]);

  const uptimeStripByGatewayId = useMemo(
    () => new Map(rows.map((gateway) => [
      gateway.gateway_id,
      buildUptimeStrip(gateway, windowMinutes),
    ])),
    [rows, windowMinutes],
  );
  const visibleRows = useMemo(
    () => filteredRows.slice(0, visibleGatewayCount),
    [filteredRows, visibleGatewayCount],
  );

  const uptimeTrendOption = useMemo(() => ({
    animationDuration: 200,
    grid: { top: 18, right: 18, bottom: 30, left: 42 },
    tooltip: { trigger: 'axis', valueFormatter: (value: number | string) => `${Number(value).toFixed(1)}%` },
    xAxis: {
      type: 'category', boundaryGap: false,
      data: uptimeTrend.map((point) => new Date(point.day).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })),
      axisLabel: { color: '#6b7280', fontSize: 11 }, axisLine: { lineStyle: { color: '#d1d5db' } },
    },
    yAxis: { type: 'value', min: 0, max: 100, axisLabel: { formatter: '{value}%', color: '#6b7280', fontSize: 11 }, splitLine: { lineStyle: { color: '#e5e7eb' } } },
    series: [{ name: 'Gateway availability', type: 'line', smooth: true, symbol: uptimeTrend.length > 45 ? 'none' : 'circle', symbolSize: 5, data: uptimeTrend.map((point) => Number(point.uptime_pct.toFixed(2))), lineStyle: { color: '#2563eb', width: 2.5 }, itemStyle: { color: '#2563eb' }, areaStyle: { color: 'rgba(37, 99, 235, 0.12)' } }],
  }), [uptimeTrend]);

  if (loading) {
    return (
      <div className="flex justify-center items-center min-h-[400px]">
        <div className="text-gray-500 dark:text-gray-400">Loading gateways...</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex justify-center items-center min-h-[400px]">
        <div className="text-red-500">Error: {error}</div>
      </div>
    );
  }

  const federationName = federation?.name || 'Federation';
  const metricsWindow = (rows.find((row) => row.metrics_window)?.metrics_window ?? timeWindow)
    .toUpperCase();

  return (
    <div className="py-5 sm:py-8">
      <Link to={`/federations/${id}`} className="text-sm text-blue-700 hover:underline dark:text-blue-300">← Federation details</Link>
      <header className="mt-4 mb-4 rounded-2xl border border-slate-200 bg-gradient-to-br from-white to-blue-50 p-5 shadow-sm dark:border-gray-700 dark:from-gray-800 dark:to-blue-950/30 sm:p-6">
        <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end"><div><div className="text-xs font-semibold uppercase tracking-[0.16em] text-blue-700 dark:text-blue-300">Lightning gateway observatory</div><h1 className="mt-2 break-words text-3xl font-bold tracking-tight text-gray-950 dark:text-white">{federationName}</h1></div><div className="rounded-xl bg-slate-100 p-1 dark:bg-gray-900"><div className="flex gap-1">{(['24h', '7d', '30d', '90d'] as GatewayWindow[]).map((window) => <button key={window} disabled={windowLoading} onClick={() => setTimeWindow(window)} className={`rounded-lg px-3 py-2 text-sm font-semibold ${timeWindow === window ? 'bg-white text-blue-700 shadow-sm dark:bg-gray-700 dark:text-blue-300' : 'text-gray-600 dark:text-gray-400'} ${windowLoading ? 'cursor-wait opacity-70' : ''}`}>{window.toUpperCase()}</button>)}</div><div className="px-2 pt-1 text-right text-[11px] text-gray-500">{windowLoading ? 'Refreshing…' : `Observed over ${timeWindow.toUpperCase()}`}</div></div></div>
        <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-slate-200 pt-3 text-sm dark:border-gray-700"><span className="font-semibold text-gray-950 dark:text-white">{totals.total} gateways</span><span className="text-green-700 dark:text-green-300">● {totals.online} online</span><span className="text-yellow-700 dark:text-yellow-300">● {totals.degraded} degraded</span><span className="text-red-700 dark:text-red-300">● {totals.offline} offline</span><span className="text-gray-600 dark:text-gray-300">{totals.vetted} vetted</span><span className="sm:ml-auto font-semibold text-indigo-700 dark:text-indigo-300">{avgUptime.toFixed(1)}% uptime <span className="font-normal text-xs text-gray-500">({avgCoverage.toFixed(0)}% observed)</span></span></div>
      </header>

      {gatewayWarning && <GatewayWarningPage warning={gatewayWarning} className="mb-4" />}

      <section className="mb-4 rounded-xl border border-gray-200 bg-white px-4 pt-4 shadow-sm dark:border-gray-700 dark:bg-gray-800 sm:px-5" aria-labelledby="uptime-trend-heading">
        <div className="flex flex-wrap items-baseline justify-between gap-2"><div><h2 id="uptime-trend-heading" className="text-base font-semibold text-gray-950 dark:text-white">Gateway availability trend ({timeWindow.toUpperCase()})</h2><p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">Each point combines every gateway poll snapshot recorded on that calendar day.</p></div>{uptimeTrend.length > 0 && <span className="text-xs text-gray-500">{uptimeTrend.length} calendar day{uptimeTrend.length === 1 ? '' : 's'} with data</span>}</div>
        {uptimeTrendLoading ? <div className="flex h-44 items-center justify-center text-sm text-gray-500">Loading availability trend…</div> : uptimeTrend.length > 0 ? <Suspense fallback={<div className="flex h-44 items-center justify-center text-sm text-gray-500">Loading chart…</div>}><UptimeTrendChart option={uptimeTrendOption} style={{ height: 210 }} notMerge lazyUpdate /></Suspense> : <div className="flex h-28 items-center justify-center text-sm text-gray-500">No gateway poll history is available for this window.</div>}
      </section>

      <section className="mb-4 overflow-hidden rounded-xl border border-gray-200 bg-white shadow-md dark:border-gray-700 dark:bg-gray-800">
        <div className="border-b border-gray-200 p-4 dark:border-gray-700 sm:p-5">
          <div className="flex flex-col justify-between gap-3 lg:flex-row lg:items-end"><div><h2 className="text-lg font-semibold text-gray-950 dark:text-white">Gateway directory</h2><p className="mt-1 text-sm text-gray-500 dark:text-gray-400">{filteredRows.length} of {rows.length} gateways · every row includes its 30-bucket availability strip, ordered oldest to newest.</p></div><div className="flex flex-col gap-2 sm:flex-row"><input value={gatewaySearch} onChange={(event) => setGatewaySearch(event.target.value)} aria-label="Search gateways" placeholder="Search gateway, node, endpoint" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 outline-none focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-900 dark:text-white" /><select value={gatewaySort} onChange={(event) => { const nextSort = event.target.value as GatewaySort; setGatewaySort(nextSort); setSortDirection(nextSort === 'status' ? 'asc' : 'desc'); }} aria-label="Sort gateways" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700 dark:border-gray-600 dark:bg-gray-900 dark:text-gray-200"><option value="freshness">Last seen</option><option value="status">Status</option><option value="uptime">Uptime</option><option value="activity">Activity</option></select><button type="button" onClick={() => setSortDirection((value) => value === 'asc' ? 'desc' : 'asc')} className="rounded-lg border border-blue-300 bg-blue-50 px-3 py-2 text-sm font-semibold text-blue-800 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-200" title={`Sort ${sortDirection === 'asc' ? 'ascending' : 'descending'}`}>{sortDirection === 'asc' ? '↑ Asc' : '↓ Desc'}</button></div></div>
          <div className="mt-3 flex gap-2 overflow-x-auto pb-1">{(['all', 'online', 'degraded', 'offline', 'unknown'] as GatewayFilter[]).map((filter) => <button key={filter} type="button" onClick={() => setGatewayFilter(filter)} className={`whitespace-nowrap rounded-full border px-3 py-1.5 text-xs font-medium capitalize ${gatewayFilter === filter ? 'border-blue-600 bg-blue-600 text-white' : 'border-gray-300 text-gray-600 dark:border-gray-600 dark:text-gray-300'}`}>{filter} ({filter === 'all' ? totals.total : totals[filter]})</button>)}</div>
          <div className="mt-3 flex flex-wrap gap-x-3 gap-y-1 text-xs text-gray-500 dark:text-gray-400"><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-green-500" />Online</span><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-yellow-500" />Degraded</span><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-red-500" />Offline</span><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-gray-300 dark:bg-gray-600" />Unknown</span></div>
        </div>
        <div className="overflow-x-auto">
        <table className="w-full min-w-[960px] table-fixed text-left text-sm text-gray-500 dark:text-gray-400">
          <colgroup>
            <col className="w-[22%]" />
            <col className="w-[28%]" />
            <col className="w-[17%]" />
            <col className="w-[17%]" />
            <col className="w-[16%]" />
          </colgroup>
          <thead className="bg-slate-50 text-xs uppercase tracking-wide text-gray-600 dark:bg-gray-700/70 dark:text-gray-300">
            <tr>
              <th scope="col" className="px-4 py-3 sm:px-5">Gateway</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Availability</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Activity</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Trust & history</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Endpoint</th>
            </tr>
          </thead>
          <tbody>
            {filteredRows.length === 0 && (
              <tr className="bg-white border-b dark:bg-gray-800 dark:border-gray-700">
                <td colSpan={5} className="px-4 sm:px-6 py-6 text-center text-gray-500 dark:text-gray-400">
                  No gateways match these filters.
                </td>
              </tr>
            )}

            {visibleRows.map((gateway) => (
              <tr
                key={gateway.gateway_id}
                className="align-top border-b border-gray-200 bg-white transition-colors last:border-b-0 hover:bg-blue-50/40 dark:border-gray-700 dark:bg-gray-800 dark:hover:bg-blue-950/20"
              >
                <td className="px-4 py-4 sm:px-5">
                  <Link
                    to={`/federations/${id}/gateways/${gateway.gateway_id}`}
                    className="block truncate font-semibold text-gray-950 hover:text-blue-700 hover:underline dark:text-white dark:hover:text-blue-300"
                    title={`${gateway.lightning_alias || 'Unnamed Gateway'} · view details`}
                  >
                    {gateway.lightning_alias || 'Unnamed Gateway'}
                  </Link>
                  <div
                    className="text-xs font-mono text-gray-600 dark:text-gray-400 mt-1"
                    title={gateway.gateway_id}
                  >
                    {shortId(gateway.gateway_id)}
                  </div>
                  <div
                    className="text-xs font-mono text-gray-500 dark:text-gray-500 mt-1"
                    title={gateway.node_pub_key}
                  >
                    Node: {shortId(gateway.node_pub_key)}
                  </div>
                </td>
                <td className="px-4 py-4 sm:px-5">
                  <div className="flex items-center justify-between gap-3">
                    <span className={`rounded-full px-2.5 py-1 text-xs font-semibold ${statusClasses(gateway.status)}`}>
                      {gateway.status}
                    </span>
                    <span className="whitespace-nowrap text-xs text-gray-500 dark:text-gray-400">seen {formatRelative(gateway.lastSeenDate)}</span>
                  </div>
                  <div className="mt-2 flex items-baseline justify-between gap-3">
                    <span className="font-semibold text-gray-950 dark:text-white">{gateway.coveragePct > 0 ? `${gateway.estimatedUptimePct.toFixed(1)}% uptime` : 'No samples'}</span>
                    <span className="text-xs text-gray-500 dark:text-gray-400">{gateway.coveragePct.toFixed(0)}% observed</span>
                  </div>
                  <div className="mt-2.5 flex gap-0.5" aria-label={`${timeWindow.toUpperCase()} availability, oldest to newest`}>
                    {(uptimeStripByGatewayId.get(gateway.gateway_id) ?? []).map((status, index) => {
                      const tooltip = getUptimeBucketTooltip(status, index, 30, windowMinutes);
                      const tooltipPosition = index < 4
                        ? 'left-0'
                        : index > 25
                          ? 'right-0'
                          : 'left-1/2 -translate-x-1/2';

                      return (
                        <span
                          key={`${gateway.gateway_id}-${index}`}
                          tabIndex={0}
                          aria-label={tooltip}
                          title={tooltip}
                          className="group relative h-3.5 flex-1 cursor-help rounded-[3px] outline-none focus-visible:ring-2 focus-visible:ring-blue-500 focus-visible:ring-offset-2 dark:focus-visible:ring-offset-gray-800"
                        >
                          <span className={`block h-full rounded-[3px] transition-transform duration-150 group-hover:scale-y-125 group-focus:scale-y-125 ${getUptimeStripClass(status)}`} />
                          <span className={`pointer-events-none absolute bottom-full z-20 mb-2 hidden w-max rounded-md bg-gray-950 px-2 py-1 text-[11px] font-medium text-white shadow-lg group-hover:block group-focus:block dark:bg-black ${tooltipPosition}`}>
                            {tooltip}
                          </span>
                        </span>
                      );
                    })}
                  </div>
                  <div className="mt-2 flex flex-wrap gap-x-2 text-xs text-gray-500 dark:text-gray-400"><span>On {formatCompactDuration(gateway.estimatedOnlineMinutes)}</span><span>Off {formatCompactDuration(gateway.estimatedOfflineMinutes)}</span><span>Unknown {formatCompactDuration(gateway.estimatedUnknownMinutes)}</span></div>
                </td>
                <td className="px-4 py-4 text-gray-700 dark:text-gray-300 sm:px-5">
                  {gateway.realActivityScore !== null ? (
                    <>
                      <div className="font-semibold text-gray-950 dark:text-white">
                        {gateway.realActivityScore.toLocaleString()} <span className="text-xs font-normal text-gray-500 dark:text-gray-400">score</span>
                      </div>
                      <div className="mt-1 text-xs leading-5 text-gray-500 dark:text-gray-400">
                        {metricsWindow} · {gateway.fundCountWindow} funds · {gateway.settleCountWindow} settles · {gateway.cancelCountWindow} cancels<br />
                        {formatMsats(gateway.totalVolumeMsatWindow)} volume
                      </div>
                    </>
                  ) : (
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      N/A (no real {metricsWindow} data)
                    </div>
                  )}
                </td>
                <td className="px-4 py-4 sm:px-5">
                  <span className={`text-sm font-medium ${gateway.vetted ? 'text-green-600 dark:text-green-400' : 'text-gray-500 dark:text-gray-400'}`}>
                    {gateway.vetted ? '✓ Vetted' : 'Not vetted'}
                  </span>
                  <div className="mt-2 text-xs text-gray-500 dark:text-gray-400">First seen<br /><span className="text-gray-700 dark:text-gray-300">{formatDateTime(gateway.firstSeenDate)}</span></div>
                  <div className="mt-2 text-xs text-gray-500 dark:text-gray-400">Last seen<br /><span className="text-gray-700 dark:text-gray-300">{formatDateTime(gateway.lastSeenDate)}</span></div>
                </td>
                <td className="px-4 py-4 sm:px-5">
                  <a
                    href={gateway.api_endpoint}
                    target="_blank"
                    rel="noreferrer"
                    title={gateway.api_endpoint}
                    className="block truncate font-medium text-blue-600 hover:underline dark:text-blue-400"
                  >
                    {formatEndpointLabel(gateway.api_endpoint)}
                  </a>
                  {gateway.raw && (
                    <button
                      type="button"
                      onClick={() => setRawAnnouncement({
                        gatewayName: gateway.lightning_alias || shortId(gateway.gateway_id),
                        raw: gateway.raw!,
                      })}
                      className="mt-2 text-xs text-gray-600 hover:text-blue-700 hover:underline dark:text-gray-400 dark:hover:text-blue-300"
                    >
                      View raw announcement
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        </div>
        {filteredRows.length > visibleRows.length && (
          <div className="border-t border-gray-200 p-4 text-center dark:border-gray-700">
            <button
              type="button"
              onClick={() => setVisibleGatewayCount((count) => count + INITIAL_RENDER_COUNT)}
              className="rounded-lg border border-blue-300 bg-blue-50 px-4 py-2 text-sm font-semibold text-blue-800 hover:bg-blue-100 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-200"
            >
              Show 50 more ({filteredRows.length - visibleRows.length} remaining)
            </button>
          </div>
        )}
      </section>
      {rawAnnouncement && (
        <div
          role="presentation"
          className="fixed inset-0 z-50 flex items-center justify-center bg-gray-950/60 p-4 backdrop-blur-sm"
          onMouseDown={() => setRawAnnouncement(null)}
        >
          <section
            role="dialog"
            aria-modal="true"
            aria-labelledby="raw-announcement-title"
            className="flex max-h-[85vh] w-full max-w-3xl flex-col overflow-hidden rounded-xl border border-gray-200 bg-white shadow-2xl dark:border-gray-700 dark:bg-gray-900"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <header className="flex items-center justify-between gap-4 border-b border-gray-200 px-5 py-4 dark:border-gray-700">
              <div className="min-w-0">
                <h2 id="raw-announcement-title" className="truncate font-semibold text-gray-950 dark:text-white">Raw announcement</h2>
                <p className="truncate text-sm text-gray-500 dark:text-gray-400">{rawAnnouncement.gatewayName}</p>
              </div>
              <button
                type="button"
                onClick={() => setRawAnnouncement(null)}
                className="rounded-lg px-3 py-2 text-sm font-semibold text-gray-600 hover:bg-gray-100 hover:text-gray-950 dark:text-gray-300 dark:hover:bg-gray-800 dark:hover:text-white"
              >
                Close
              </button>
            </header>
            <pre className="m-0 max-h-[68vh] overflow-auto bg-slate-950 p-5 text-xs leading-5 text-slate-100">
              {JSON.stringify(rawAnnouncement.raw, null, 2)}
            </pre>
          </section>
        </div>
      )}
    </div>
  );
}
