import { useEffect, useMemo, useRef, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { api } from '../services/api';
import type { FederationSummary, GatewayInfo, GatewayWindow } from '../types/api';
import { GatewayWarningPage, type GatewayWarningState } from '../components/GatewayWarningPage';

type GatewayStatus = 'online' | 'degraded' | 'offline' | 'unknown';
type UptimeStripStatus = 'online' | 'degraded' | 'offline' | 'unknown';

interface GatewayWithStatus extends GatewayInfo {
  firstSeenDate: Date | null;
  lastSeenDate: Date | null;
  status: GatewayStatus;
  minutesSinceLastSeen: number | null;
  inferredOfflineMinutes: number;
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

function formatRelative(date: Date | null): string {
  if (!date) return 'Never seen';

  const diffMs = Date.now() - date.getTime();
  const minutes = Math.floor(diffMs / (1000 * 60));
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

function shortId(value: string): string {
  if (value.length <= 16) return value;
  return `${value.slice(0, 8)}...${value.slice(-8)}`;
}

function formatDuration(minutes: number): string {
  const safe = Math.max(0, Math.floor(minutes));
  const days = Math.floor(safe / (60 * 24));
  const hours = Math.floor((safe % (60 * 24)) / 60);
  const mins = safe % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${mins}m`;
  return `${mins}m`;
}

function formatCompactDuration(minutes: number): string {
  const safe = Math.max(0, Math.round(minutes));
  if (safe >= 60 * 24) return `${Math.round(safe / (60 * 24))}d`;
  if (safe >= 60) return `${Math.round(safe / 60)}h`;
  if (safe === 0) return '0m';
  return `${safe}m`;
}

function formatMsats(msat: number): string {
  const sats = msat / 1000;
  if (sats >= 100_000_000) return `${(sats / 100_000_000).toFixed(2)} BTC`;
  if (sats >= 100_000) return `${(sats / 100_000).toFixed(1)}M sats`;
  if (sats >= 1_000) return `${(sats / 1000).toFixed(1)}k sats`;
  return `${Math.round(sats).toLocaleString()} sats`;
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

    (async () => {
      try {
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

        return {
          ...gateway,
          firstSeenDate,
          lastSeenDate,
          status,
          minutesSinceLastSeen,
          inferredOfflineMinutes: inferredOfflineFromRecency,
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
        };
      })
      .sort((a, b) => {
        const left = a.lastSeenDate?.getTime() ?? 0;
        const right = b.lastSeenDate?.getTime() ?? 0;
        return right - left;
      });
  }, [gateways, windowMinutes]);

  const totals = useMemo(() => {
    const total = rows.length;
    const online = rows.filter((row) => row.status === 'online').length;
    const degraded = rows.filter((row) => row.status === 'degraded').length;
    const offline = rows.filter((row) => row.status === 'offline').length;
    const vetted = rows.filter((row) => row.vetted).length;

    return { total, online, degraded, offline, vetted };
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

  const uptimeStrips = useMemo(() => {
    return [...rows]
      .sort((a, b) => {
        if (b.estimatedUptimePct !== a.estimatedUptimePct) {
          return b.estimatedUptimePct - a.estimatedUptimePct;
        }
        return (b.realActivityScore ?? 0) - (a.realActivityScore ?? 0);
      })
      .map((gateway) => ({
        gateway,
        strip: buildUptimeStrip(gateway, windowMinutes),
      }));
  }, [rows, windowMinutes]);

  const mostActive = useMemo(() => {
    return rows
      .filter((row) => row.realActivityScore !== null)
      .sort((a, b) => (b.realActivityScore ?? 0) - (a.realActivityScore ?? 0))
      .slice(0, 5);
  }, [rows]);

  const hasRealActivityData = useMemo(
    () => rows.some((row) => row.realActivityScore !== null),
    [rows],
  );

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
    <div className="py-4 sm:py-8 px-4 sm:px-0">
      <div className="mb-4 sm:mb-6">
        <Link
          to={`/federations/${id}`}
          className="text-sm sm:text-base text-blue-600 dark:text-blue-400 hover:underline"
        >
          ← Back to Federation Details
        </Link>
      </div>

      <h1 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white mb-2 break-words">
        {federationName} Gateways
      </h1>
      <div className="flex flex-wrap items-center justify-between gap-3 mb-6 sm:mb-8">
        <p className="text-sm sm:text-base text-gray-600 dark:text-gray-400">
        Gateway registry view with latest-seen freshness and federation LN gateway metadata.
        </p>
        <div className="flex items-center gap-2">
          {windowLoading && (
            <span className="text-xs text-gray-500 dark:text-gray-400">Updating...</span>
          )}
          {(['24h', '7d', '30d', '90d'] as GatewayWindow[]).map((window) => (
            <button
              key={window}
              disabled={windowLoading}
              onClick={() => setTimeWindow(window)}
              className={`px-3 py-1.5 text-sm rounded-xl border ${
                timeWindow === window
                  ? 'border-blue-400 bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300'
                  : 'border-gray-300 text-gray-600 dark:border-gray-600 dark:text-gray-300'
              } ${windowLoading ? 'opacity-70 cursor-wait' : ''}`}
            >
              {window.toUpperCase()}
            </button>
          ))}
        </div>
      </div>

      {gatewayWarning && (
        <GatewayWarningPage warning={gatewayWarning} className="mb-6" />
      )}

      <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-5 gap-3 sm:gap-4 mb-6">
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500 dark:text-gray-400">Total Gateways</div>
          <div className="text-2xl font-bold text-gray-900 dark:text-white mt-2">{totals.total}</div>
        </div>
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500 dark:text-gray-400">Online</div>
          <div className="text-2xl font-bold text-green-700 dark:text-green-300 mt-2">{totals.online}</div>
        </div>
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500 dark:text-gray-400">Degraded</div>
          <div className="text-2xl font-bold text-yellow-700 dark:text-yellow-300 mt-2">{totals.degraded}</div>
        </div>
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500 dark:text-gray-400">Offline</div>
          <div className="text-2xl font-bold text-red-700 dark:text-red-300 mt-2">{totals.offline}</div>
        </div>
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500 dark:text-gray-400">Vetted</div>
          <div className="text-2xl font-bold text-blue-700 dark:text-blue-300 mt-2">{totals.vetted}</div>
        </div>
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:col-span-2 xl:col-span-5">
          <div className="text-xs uppercase tracking-wide text-gray-500 dark:text-gray-400">Avg Uptime (Observed)</div>
          <div className="text-2xl font-bold text-indigo-700 dark:text-indigo-300 mt-2">{avgUptime.toFixed(1)}%</div>
          <div className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            Window: {timeWindow.toUpperCase()} · Coverage: {avgCoverage.toFixed(1)}%
          </div>
        </div>
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-3 gap-4 sm:gap-6 mb-6">
        <div className="xl:col-span-2 bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:p-6">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-1">
            Gateway Uptime ({timeWindow.toUpperCase()} Window)
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-4">
            30-bucket availability strip over the selected window (newest at right).
          </p>
          <div className="text-xs text-gray-400 dark:text-gray-500 mb-3">oldest ← newest</div>
          <div className="space-y-4">
            {uptimeStrips.length === 0 && (
              <div className="text-sm text-gray-500 dark:text-gray-400">No gateways discovered yet.</div>
            )}
            {uptimeStrips.map(({ gateway, strip }) => (
              <div key={gateway.gateway_id}>
                <div className="flex items-center justify-between text-xs sm:text-sm mb-2 gap-2">
                  <span className="text-gray-900 dark:text-white font-medium truncate">
                    {gateway.lightning_alias || shortId(gateway.gateway_id)}
                  </span>
                  <span className="text-gray-500 dark:text-gray-400 whitespace-nowrap">
                    {gateway.estimatedUptimePct.toFixed(1)}%
                  </span>
                </div>
                <div className="flex gap-1">
                  {strip.map((status, idx) => (
                    <div
                      key={`${gateway.gateway_id}-${idx}`}
                      className={`h-3 flex-1 rounded-sm ${getUptimeStripClass(status)}`}
                      title={getUptimeBucketLabel(idx, strip.length, windowMinutes)}
                    />
                  ))}
                </div>
              </div>
            ))}
            {uptimeStrips.length > 0 && (
              <div className="flex items-center gap-4 pt-1 text-xs text-gray-500 dark:text-gray-400">
                <div className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-sm bg-green-500 dark:bg-green-400" /> Online</div>
                <div className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-sm bg-yellow-500 dark:bg-yellow-400" /> Degraded</div>
                <div className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-sm bg-red-500 dark:bg-red-400" /> Offline</div>
                <div className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-sm bg-gray-300 dark:bg-gray-600" /> Unknown</div>
              </div>
            )}
          </div>
        </div>

        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:p-6">
          <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-1">
            Most Active Gateways (Real {metricsWindow})
          </h2>
          <p className="text-sm text-gray-500 dark:text-gray-400 mb-3">
            Ranked by real {metricsWindow} contract activity (fund/settle/cancel + volume).
          </p>
          {!hasRealActivityData && (
            <p className="text-xs text-gray-500 dark:text-gray-400 mb-3">
              No gateway contract events were found in the backend {metricsWindow} activity window.
            </p>
          )}
          <div className="space-y-3">
            {mostActive.map((gateway) => (
              <div key={gateway.gateway_id} className="border border-gray-200 dark:border-gray-700 rounded-lg p-2.5">
                <div className="flex items-center justify-between text-sm">
                  <span className="font-mono text-gray-900 dark:text-white">
                    {gateway.lightning_alias || shortId(gateway.gateway_id)}
                  </span>
                  <span className="font-semibold text-gray-900 dark:text-white">
                    {(gateway.realActivityScore ?? 0).toLocaleString()}
                  </span>
                </div>
                <div className="text-xs text-gray-600 dark:text-gray-400 mt-1">
                  {metricsWindow}: fund {gateway.fundCountWindow} · settle {gateway.settleCountWindow} · cancel {gateway.cancelCountWindow}
                </div>
                <div className="w-full h-2 mt-2 rounded-full bg-gray-200 dark:bg-gray-700 overflow-hidden">
                  <div
                    className="h-full bg-blue-600"
                    style={{
                      width: `${
                        mostActive[0] && (mostActive[0].realActivityScore ?? 0) > 0
                          ? Math.max(
                              8,
                              ((gateway.realActivityScore ?? 0) / (mostActive[0].realActivityScore ?? 1)) * 100,
                            )
                          : 8
                      }%`,
                    }}
                  />
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="relative overflow-x-auto bg-white dark:bg-gray-800 shadow-md rounded-lg border border-gray-200 dark:border-gray-700">
        <div className="p-4 sm:p-5 text-lg font-semibold text-left text-gray-900 dark:text-white">
          Gateway Details
          <p className="mt-1 text-sm font-normal text-gray-500 dark:text-gray-400">
            Complete list of gateways discovered for this federation.
          </p>
        </div>
        <table className="w-full text-sm text-left text-gray-500 dark:text-gray-400">
          <thead className="text-xs text-gray-700 uppercase bg-gray-100 dark:bg-gray-700 dark:text-gray-300">
            <tr>
              <th scope="col" className="px-4 sm:px-6 py-3">Gateway</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Status</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Uptime %</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Online</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Offline</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Unknown</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Activity</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Vetted</th>
              <th scope="col" className="px-4 sm:px-6 py-3">First Seen</th>
              <th scope="col" className="px-4 sm:px-6 py-3">Last Seen</th>
              <th scope="col" className="px-4 sm:px-6 py-3">API Endpoint</th>
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr className="bg-white border-b dark:bg-gray-800 dark:border-gray-700">
                <td colSpan={11} className="px-4 sm:px-6 py-6 text-center text-gray-500 dark:text-gray-400">
                  No gateways available for this federation yet.
                </td>
              </tr>
            )}

            {rows.map((gateway) => (
              <tr
                key={gateway.gateway_id}
                className="bg-white border-b dark:bg-gray-800 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700/60 align-top"
              >
                <td className="px-4 sm:px-6 py-4">
                  <div className="font-medium text-gray-900 dark:text-white">
                    {gateway.lightning_alias || 'Unnamed Gateway'}
                  </div>
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
                <td className="px-4 sm:px-6 py-4">
                  <span className={`px-2.5 py-1 rounded-full text-xs font-medium ${statusClasses(gateway.status)}`}>
                    {gateway.status}
                  </span>
                  <div className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    {formatRelative(gateway.lastSeenDate)}
                  </div>
                </td>
                <td className="px-4 sm:px-6 py-4 text-gray-700 dark:text-gray-300">
                  {gateway.estimatedUptimePct.toFixed(1)}%
                </td>
                <td className="px-4 sm:px-6 py-4 text-gray-700 dark:text-gray-300">
                  {formatDuration(gateway.estimatedOnlineMinutes)}
                </td>
                <td className="px-4 sm:px-6 py-4 text-gray-700 dark:text-gray-300">
                  {formatDuration(gateway.estimatedOfflineMinutes)}
                </td>
                <td className="px-4 sm:px-6 py-4 text-gray-700 dark:text-gray-300">
                  {formatDuration(gateway.estimatedUnknownMinutes)}
                </td>
                <td className="px-4 sm:px-6 py-4 text-gray-700 dark:text-gray-300">
                  {gateway.realActivityScore !== null ? (
                    <>
                      <div className="font-medium text-gray-900 dark:text-white">
                        {gateway.realActivityScore.toLocaleString()}
                      </div>
                      <div className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                        {metricsWindow} F:{gateway.fundCountWindow} S:{gateway.settleCountWindow} C:{gateway.cancelCountWindow}<br />
                        Vol: {formatMsats(gateway.totalVolumeMsatWindow)}
                      </div>
                    </>
                  ) : (
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      N/A (no real {metricsWindow} data)
                    </div>
                  )}
                </td>
                <td className="px-4 sm:px-6 py-4">
                  <span className={gateway.vetted ? 'text-green-600 dark:text-green-400' : 'text-gray-500 dark:text-gray-400'}>
                    {gateway.vetted ? 'Yes' : 'No'}
                  </span>
                </td>
                <td className="px-4 sm:px-6 py-4 text-gray-700 dark:text-gray-300">
                  {formatDateTime(gateway.firstSeenDate)}
                </td>
                <td className="px-4 sm:px-6 py-4 text-gray-700 dark:text-gray-300">
                  {formatDateTime(gateway.lastSeenDate)}
                </td>
                <td className="px-4 sm:px-6 py-4">
                  <a
                    href={gateway.api_endpoint}
                    target="_blank"
                    rel="noreferrer"
                    className="text-blue-600 dark:text-blue-400 hover:underline break-all"
                  >
                    {gateway.api_endpoint}
                  </a>
                  {gateway.raw && (
                    <details className="mt-2">
                      <summary className="text-xs text-gray-600 dark:text-gray-400 cursor-pointer hover:text-gray-800 dark:hover:text-gray-200">
                        Raw announcement
                      </summary>
                      <pre className="mt-2 text-[11px] p-2 rounded bg-gray-100 dark:bg-gray-900 border border-gray-200 dark:border-gray-700 overflow-x-auto">
                        {JSON.stringify(gateway.raw, null, 2)}
                      </pre>
                    </details>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
