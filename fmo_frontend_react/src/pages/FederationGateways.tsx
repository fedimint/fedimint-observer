import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import { api } from '../services/api';
import type {
  FederationSummary,
  GatewayInfo,
  GatewayUptimeTrendPoint,
} from '../types/api';
import { GatewayWarningPage, type GatewayWarningState } from '../components/GatewayWarningPage';
import { ErrorBoundary, ReloadMessage } from '../components/ErrorBoundary';
import { readStorage, writeStorage } from '../utils/storage';
import { useTheme } from '../hooks/useTheme';
import type { EChartsOption } from 'echarts';

type GatewayStatus = 'online' | 'degraded' | 'offline' | 'unknown' | 'retired';
type UptimeStripStatus = 'online' | 'degraded' | 'offline' | 'unknown';

const GATEWAY_WINDOWS = ['24h', '7d', '30d', '90d'] as const;
type SelectableWindow = (typeof GATEWAY_WINDOWS)[number];
// 'all' means every active gateway; retired ones have their own filter
const GATEWAY_FILTERS = ['all', 'online', 'degraded', 'offline', 'unknown', 'retired'] as const;
const GATEWAY_SORTS = ['freshness', 'status', 'uptime', 'activity'] as const;
type GatewaySort = (typeof GATEWAY_SORTS)[number];
const SORT_DIRECTIONS = ['asc', 'desc'] as const;
type SortDirection = (typeof SORT_DIRECTIONS)[number];

const UptimeTrendChart = lazy(() => import('../components/UptimeTrendChart').then((module) => ({ default: module.UptimeTrendChart })));
const INITIAL_RENDER_COUNT = 50;
const DEFAULT_WINDOW: SelectableWindow = '7d';
const WINDOW_STORAGE_KEY = 'gatewayWindow';
const WINDOW_MINUTES: Record<SelectableWindow, number> = {
  '24h': 24 * 60,
  '7d': 7 * 24 * 60,
  '30d': 30 * 24 * 60,
  '90d': 90 * 24 * 60,
};
// Gateways are polled and their overview rebuilt every 5 minutes on the server
const REFRESH_INTERVAL_MS = 60_000;
const REFRESH_CHECK_MS = 10_000;
const LIVE_LOOKUP_TIMEOUT_MS = 10_000;
const EMPTY_TREND: GatewayUptimeTrendPoint[] = [];
const RETIRED_AFTER_MINUTES = 7 * 24 * 60;
const STATUS_RANK: Record<GatewayStatus, number> = {
  offline: 0,
  degraded: 1,
  unknown: 2,
  online: 3,
  retired: 4,
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

function getGatewayStatus(lastSeen: Date | null, now: number): GatewayStatus {
  if (!lastSeen) return 'unknown';
  const minutes = (now - lastSeen.getTime()) / (1000 * 60);
  // Gateways are polled every 5 minutes and the overview is rebuilt every 5, so allow
  // up to three polls before a healthy gateway stops counting as online
  if (minutes <= 15) return 'online';
  if (minutes <= 30) return 'degraded';
  // Gone from the registry for a week: it has left the federation (same rule as the backend)
  if (minutes > RETIRED_AFTER_MINUTES) return 'retired';
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

function formatRelative(date: Date | null, now: number): string {
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

function shortId(value: string): string {
  if (value.length <= 16) return value;
  return `${value.slice(0, 8)}...${value.slice(-8)}`;
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

  if (gateway.status === 'unknown' || gateway.status === 'retired' || windowMinutes <= 0) {
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
  retry: boolean;
}

interface RawAnnouncementDialog {
  gatewayName: string;
  raw: Record<string, unknown>;
}

type LiveLookup =
  | { status: 'pending' }
  | { status: 'skipped' }
  | { status: 'done'; gateways: GatewayInfo[]; error: string | null };

interface LoadError {
  window: SelectableWindow;
  message: string;
  network: boolean; // fetch itself failed, as opposed to an error answer from the API
}

interface GatewaySources {
  observed: GatewayInfo[] | null; // recorded history, null until it has loaded once
  observedError: LoadError | null; // the latest request for the selected window failed
  live: LiveLookup; // the federation's own registry, which only adds details
  federationFailed: boolean;
  federationOffline: boolean;
  dataWindow: SelectableWindow;
}

// Banners are only for problems or empty states; normal pages show none
function selectGatewayData({
  observed,
  observedError,
  live,
  federationFailed,
  federationOffline,
  dataWindow,
}: GatewaySources): GatewaySelection {
  const liveGateways = live.status === 'done' ? live.gateways : [];
  const liveError = live.status === 'done' ? live.error : null;

  if (observed !== null) {
    const gateways = liveGateways.length > 0 ? mergeGatewayData(observed, liveGateways) : observed;
    if (observedError) {
      return {
        gateways,
        retry: true,
        warning: {
          level: 'warning',
          title: `Could not load ${observedError.window.toUpperCase()} gateway data`,
          message: dataWindow === observedError.window
            ? 'Showing the last data that loaded.'
            : `Showing ${dataWindow.toUpperCase()} data instead.`,
          detail: observedError.message,
        },
      };
    }
    if (federationFailed) {
      return {
        gateways,
        retry: true,
        warning: {
          level: 'warning',
          title: "Could not load the federation's details",
          message: 'Its name and the live gateway check are missing until they load.',
        },
      };
    }
    if (observed.length > 0) return { gateways, warning: null, retry: false };
    if (liveGateways.length > 0) {
      return {
        gateways,
        retry: false,
        warning: {
          level: 'info',
          title: 'No recorded history yet',
          message: "These gateways come from the federation's live registry. Their status history has not been recorded yet.",
        },
      };
    }
    if (live.status === 'pending') {
      return {
        gateways,
        retry: false,
        warning: { level: 'info', title: 'Checking for gateways', message: "Looking up the federation's gateway registry…" },
      };
    }
    if (liveError) {
      return {
        gateways,
        retry: true,
        warning: {
          level: 'warning',
          title: "Could not check the federation's gateway registry",
          message: 'No gateways have been recorded for this federation, and its own registry could not be reached.',
          detail: liveError,
        },
      };
    }
    return {
      gateways,
      retry: false,
      warning: federationOffline
        ? { level: 'info', title: 'Federation offline', message: 'This federation is offline and no gateways have been recorded for it.' }
        : { level: 'info', title: 'No gateways', message: 'No gateways have been recorded for this federation.' },
    };
  }

  // Registry-only rows stand in for the history only once loading it has failed
  if (observedError && liveGateways.length > 0) {
    return {
      gateways: liveGateways,
      retry: true,
      warning: {
        level: 'warning',
        title: 'Gateway history unavailable',
        message: "Showing gateways from the federation's live registry only.",
        detail: observedError?.message,
      },
    };
  }

  return {
    gateways: [],
    retry: true,
    warning: {
      level: 'error',
      title: 'Could not load gateway data',
      message: observedError?.network
        ? 'Check your connection and try again.'
        : 'The observer API returned an error. Try again in a moment.',
      detail: observedError?.message,
    },
  };
}

function parseOption<T extends string>(options: readonly T[], value: string | null): T | null {
  const lower = value?.toLowerCase();
  return options.find((option) => option === lower) ?? null;
}

function defaultDirection(sort: GatewaySort): SortDirection {
  return sort === 'status' ? 'asc' : 'desc';
}

function errorMessage(err: unknown, fallback: string): string {
  return err instanceof Error ? err.message : fallback;
}

const DARK_QUERY = '(prefers-color-scheme: dark)';
function subscribeToColorScheme(onChange: () => void) {
  const media = window.matchMedia(DARK_QUERY);
  media.addEventListener('change', onChange);
  return () => media.removeEventListener('change', onChange);
}

// Re-renders when the system switches between light and dark, for the 'auto' theme
function usePrefersDark(): boolean {
  return useSyncExternalStore(subscribeToColorScheme, () => window.matchMedia(DARK_QUERY).matches);
}

export function FederationGateways() {
  // The router remounts this page per federation (App.tsx), so id never changes here
  const { id = '' } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  // The view lives in the URL so a refresh, Back or a shared link keeps it. Without a
  // window in the URL, e.g. coming from "Gateway Details", the last one picked applies;
  // it is read once, so picking a window in another tab does not change this one.
  const [storedWindow] = useState(() => parseOption(GATEWAY_WINDOWS, readStorage(WINDOW_STORAGE_KEY)));
  const timeWindow = parseOption(GATEWAY_WINDOWS, searchParams.get('window')) ?? storedWindow ?? DEFAULT_WINDOW;
  const gatewayFilter = parseOption(GATEWAY_FILTERS, searchParams.get('status')) ?? 'all';
  const gatewaySort = parseOption(GATEWAY_SORTS, searchParams.get('sort')) ?? 'freshness';
  const sortDirection = parseOption(SORT_DIRECTIONS, searchParams.get('dir')) ?? defaultDirection(gatewaySort);

  const [federation, setFederation] = useState<FederationSummary | null | undefined>(undefined);
  const [federationFailed, setFederationFailed] = useState(false);
  const [federationKey, setFederationKey] = useState(0);
  // Each response is stored with the window it belongs to, so a window switch never
  // recomputes the previous window's numbers against the new window's length
  const [observed, setObserved] = useState<{ window: SelectableWindow; gateways: GatewayInfo[] } | null>(null);
  const [observedError, setObservedError] = useState<LoadError | null>(null);
  const [observedLoading, setObservedLoading] = useState(true);
  const [live, setLive] = useState<LiveLookup>({ status: 'pending' });
  const [liveKey, setLiveKey] = useState(0);
  const [trend, setTrend] = useState<{ window: SelectableWindow; points: GatewayUptimeTrendPoint[] } | null>(null);
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [reloadKey, setReloadKey] = useState(0); // Try again
  const [refreshKey, setRefreshKey] = useState(0); // background refresh
  const [gatewaySearch, setGatewaySearch] = useState('');
  const [visibleGatewayCount, setVisibleGatewayCount] = useState(INITIAL_RENDER_COUNT);
  const [rawAnnouncement, setRawAnnouncement] = useState<RawAnnouncementDialog | null>(null);
  const loadInFlight = useRef(false);
  const lastLoadAt = useRef(0); // when the last load finished
  const focusAfterRetry = useRef(false);
  const headingRef = useRef<HTMLHeadingElement>(null);

  // One navigation per action; replace keeps Back working. It starts from the address
  // bar because the router applies URL updates in a transition, so the hook's copy can
  // lag behind a quick second click and drop the first change.
  const updateView = useCallback((changes: Partial<Record<'window' | 'status' | 'sort' | 'dir', string | null>>) => {
    const next = new URLSearchParams(window.location.search);
    for (const [key, value] of Object.entries(changes)) {
      if (value == null) next.delete(key);
      else next.set(key, value);
    }
    setSearchParams(next, { replace: true });
  }, [setSearchParams]);

  const selectWindow = (nextWindow: SelectableWindow) => {
    writeStorage(WINDOW_STORAGE_KEY, nextWindow);
    updateView({ window: nextWindow });
  };

  useEffect(() => {
    let cancelled = false;
    api.getFederations()
      .then((federations) => {
        if (cancelled) return;
        setFederation(federations.find((item) => item.id === id) ?? null);
        setFederationFailed(false);
      })
      .catch(() => {
        // Only the name and the live lookup need it; the recorded data still shows
        if (!cancelled) setFederationFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [id, federationKey]);

  // Keep retrying the federation list while it is missing
  useEffect(() => {
    if (!federationFailed) return;
    const timer = setTimeout(() => setFederationKey((key) => key + 1), REFRESH_INTERVAL_MS);
    return () => clearTimeout(timer);
  }, [federationFailed, federationKey]);

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;
    loadInFlight.current = true;
    setObservedLoading(true);

    // The table and the trend come in one answer, so they always describe the same moment
    api.getFederationGatewayOverview(id, timeWindow, signal)
      .then((overview) => {
        if (signal.aborted) return;
        setObserved({ window: timeWindow, gateways: overview.gateways ?? [] });
        setTrend({ window: timeWindow, points: overview.uptime_trend ?? [] });
        setObservedError(null);
        setUpdatedAt(Date.parse(overview.computed_at));
      })
      .catch((err: unknown) => {
        if (signal.aborted) return;
        setObservedError({
          window: timeWindow,
          message: errorMessage(err, 'Failed to fetch gateways'),
          network: err instanceof TypeError,
        });
      })
      .finally(() => {
        if (signal.aborted) return;
        setObservedLoading(false);
        loadInFlight.current = false;
        lastLoadAt.current = Date.now();
      });

    return () => {
      controller.abort();
      loadInFlight.current = false;
    };
  }, [id, timeWindow, reloadKey, refreshKey]);

  // The federation's own registry only adds details, so it never holds up the page
  const invite = federation?.invite;
  const federationOffline = federation?.health === 'offline';
  useEffect(() => {
    // An offline federation's guardians can't answer; the lookup would hang for a minute
    if (!invite || federationOffline) return;
    const controller = new AbortController();
    let cancelled = false;
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, LIVE_LOOKUP_TIMEOUT_MS);
    setLive({ status: 'pending' });
    api.getFederationGatewaysByInvite(invite, controller.signal)
      .then((gateways) => {
        if (!cancelled) setLive({ status: 'done', gateways, error: null });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setLive({
          status: 'done',
          gateways: [],
          error: timedOut ? 'The live registry lookup timed out' : errorMessage(err, 'The live registry lookup failed'),
        });
      })
      .finally(() => clearTimeout(timeout));
    return () => {
      cancelled = true;
      clearTimeout(timeout);
      controller.abort();
    };
  }, [invite, federationOffline, liveKey]);

  // Keep statuses and "seen" times current, and refresh a minute after the last load
  // finished. A load in flight is never interrupted, so slow windows can complete.
  useEffect(() => {
    const clock = setInterval(() => setNow(Date.now()), 30_000);
    const refresh = setInterval(() => {
      if (document.visibilityState !== 'visible' || loadInFlight.current) return;
      if (Date.now() - lastLoadAt.current >= REFRESH_INTERVAL_MS) setRefreshKey((key) => key + 1);
    }, REFRESH_CHECK_MS);
    const onVisibilityChange = () => {
      if (document.visibilityState !== 'visible') return;
      setNow(Date.now());
      if (!loadInFlight.current && Date.now() - lastLoadAt.current > 30_000) setRefreshKey((key) => key + 1);
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      clearInterval(clock);
      clearInterval(refresh);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, []);

  const liveLookup = useMemo<LiveLookup>(() => {
    if (invite && !federationOffline) return live;
    return { status: federation === undefined && !federationFailed ? 'pending' : 'skipped' };
  }, [federation, federationFailed, federationOffline, invite, live]);

  // An error only describes the window it happened in, never one that is still loading
  const currentError = observedError?.window === timeWindow ? observedError : null;
  const dataWindow = observed?.window ?? timeWindow;
  const windowMinutes = WINDOW_MINUTES[dataWindow];
  const selection = useMemo(() => selectGatewayData({
    observed: observed?.gateways ?? null,
    observedError: currentError,
    live: liveLookup,
    federationFailed,
    federationOffline,
    dataWindow,
  }), [currentError, dataWindow, federationFailed, federationOffline, liveLookup, observed]);
  const gateways = selection.gateways;

  // Try again repeats only what failed, and is ignored while an attempt is running
  const retryBusy = observedLoading;
  const retry = () => {
    if (retryBusy) return;
    focusAfterRetry.current = true;
    setReloadKey((key) => key + 1);
    if (federationFailed) setFederationKey((key) => key + 1);
    if (live.status === 'done' && live.error) setLiveKey((key) => key + 1);
  };

  // When a retry succeeds, its banner disappears together with the focused Try again
  // button; put keyboard focus on the page heading instead of losing it
  useEffect(() => {
    if (focusAfterRetry.current && !observedLoading && document.activeElement === document.body) {
      focusAfterRetry.current = false;
      headingRef.current?.focus();
    }
  });

  const rows = useMemo<GatewayWithStatus[]>(() => {
    return gateways
      .map((gateway) => {
        const firstSeenDate = parseTimestamp(gateway.first_seen);
        const lastSeenDate = parseTimestamp(gateway.last_seen);
        const status = getGatewayStatus(lastSeenDate, now);
        const minutesSinceLastSeen = lastSeenDate
          ? (now - lastSeenDate.getTime()) / (1000 * 60)
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
  }, [gateways, now, windowMinutes]);

  // Retired gateways are listed separately and left out of every figure
  const activeRows = useMemo(() => rows.filter((row) => row.status !== 'retired'), [rows]);

  const totals = useMemo(() => {
    const total = activeRows.length;
    const online = activeRows.filter((row) => row.status === 'online').length;
    const degraded = activeRows.filter((row) => row.status === 'degraded').length;
    const offline = activeRows.filter((row) => row.status === 'offline').length;
    const unknown = total - online - degraded - offline;
    const vetted = activeRows.filter((row) => row.vetted).length;
    const retired = rows.length - total;

    return { total, online, degraded, offline, unknown, vetted, retired };
  }, [activeRows, rows]);

  const avgUptime = useMemo(() => {
    const observedRows = activeRows.filter((row) => row.coveragePct > 0);
    if (observedRows.length === 0) return 0;
    const total = observedRows.reduce((sum, row) => sum + row.estimatedUptimePct, 0);
    return total / observedRows.length;
  }, [activeRows]);

  const avgCoverage = useMemo(() => {
    if (activeRows.length === 0) return 0;
    const total = activeRows.reduce((sum, row) => sum + row.coveragePct, 0);
    return total / activeRows.length;
  }, [activeRows]);

  const filteredRows = useMemo(() => {
    const query = gatewaySearch.trim().toLowerCase();
    const filtered = rows.filter((row) => (
      (gatewayFilter === 'all' ? row.status !== 'retired' : row.status === gatewayFilter)
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
  }, [gatewayFilter, gatewaySearch, gatewaySort, sortDirection]);

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

  const { theme } = useTheme();
  const prefersDark = usePrefersDark();
  const darkMode = theme === 'dark' || (theme === 'auto' && prefersDark);
  const chartColors = useMemo(() => (darkMode
    ? { label: '#9ca3af', axis: '#4b5563', grid: '#374151', line: '#60a5fa', area: 'rgba(96, 165, 250, 0.15)' }
    : { label: '#6b7280', axis: '#d1d5db', grid: '#e5e7eb', line: '#2563eb', area: 'rgba(37, 99, 235, 0.12)' }), [darkMode]);

  // A window switch shows the loading state rather than the previous window's trend
  const uptimeTrend = trend?.window === timeWindow ? trend.points : EMPTY_TREND;
  const uptimeTrendOption = useMemo<EChartsOption>(() => ({
    animationDuration: 200,
    grid: { top: 18, right: 18, bottom: 30, left: 42 },
    tooltip: { trigger: 'axis', valueFormatter: (value) => `${Number(value).toFixed(1)}%` },
    xAxis: {
      type: 'category', boundaryGap: false,
      // Days are bucketed in the server's time zone (midnight there is e.g. 22:00Z). Half a
      // day later, formatted in UTC, names that same calendar day for every viewer.
      data: uptimeTrend.map((point) => new Date(Date.parse(point.day) + 12 * 60 * 60 * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone: 'UTC' })),
      axisLabel: { color: chartColors.label, fontSize: 11 }, axisLine: { lineStyle: { color: chartColors.axis } },
    },
    yAxis: { type: 'value', min: 0, max: 100, axisLabel: { formatter: '{value}%', color: chartColors.label, fontSize: 11 }, splitLine: { lineStyle: { color: chartColors.grid } } },
    series: [{ name: 'Gateway availability', type: 'line', smooth: true, symbol: uptimeTrend.length > 45 ? 'none' : 'circle', symbolSize: 5, data: uptimeTrend.map((point) => Number(point.uptime_pct.toFixed(2))), lineStyle: { color: chartColors.line, width: 2.5 }, itemStyle: { color: chartColors.line }, areaStyle: { color: chartColors.area } }],
  }), [chartColors, uptimeTrend]);

  if (federation === null && observed !== null && observed.gateways.length === 0) {
    return (
      <div className="flex min-h-[400px] flex-col items-center justify-center gap-3 text-center">
        <p className="text-gray-700 dark:text-gray-300">This federation is not tracked by the observer.</p>
        <Link to="/" className="text-sm text-blue-700 hover:underline dark:text-blue-300">← All federations</Link>
      </div>
    );
  }

  const metricsWindow = (rows.find((row) => row.metrics_window)?.metrics_window ?? dataWindow)
    .toUpperCase();
  // Until the recorded gateways arrive there are no numbers to show, only the controls
  const hasData = observed !== null || gateways.length > 0;
  const switchingWindow = observed !== null && dataWindow !== timeWindow && observedLoading;
  const trendPlaceholder = trend?.window === timeWindow
    ? 'No gateway poll history is available for this window.'
    : observedLoading
      ? 'Loading availability trend…'
      : currentError ? 'The availability trend could not be loaded.' : '';
  const windowStatus = observedLoading && (observed === null || dataWindow !== timeWindow)
    ? `Loading ${timeWindow.toUpperCase()}…`
    : observed === null
      ? ' '
      : dataWindow !== timeWindow
        ? `Showing ${dataWindow.toUpperCase()}`
        : updatedAt ? `Updated ${formatRelative(new Date(updatedAt), now)}` : 'Refreshing…';

  return (
    <div className="py-5 sm:py-8">
      <Link to={`/federations/${id}`} className="text-sm text-blue-700 hover:underline dark:text-blue-300">← Federation details</Link>
      <header className="mt-4 mb-4 rounded-2xl border border-slate-200 bg-gradient-to-br from-white to-blue-50 p-5 shadow-sm dark:border-gray-700 dark:from-gray-800 dark:to-blue-950/30 sm:p-6">
        <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
          <div>
            <div className="text-xs font-semibold uppercase tracking-[0.16em] text-blue-700 dark:text-blue-300">Lightning gateway observatory</div>
            <h1 ref={headingRef} tabIndex={-1} className="mt-2 break-words text-3xl font-bold tracking-tight text-gray-950 outline-none dark:text-white">
              {federation?.name || (federation === undefined && !federationFailed
                ? <span aria-hidden="true" className="inline-block h-8 w-56 max-w-full animate-pulse rounded-lg bg-slate-200 align-middle dark:bg-gray-700" />
                : 'Federation')}
            </h1>
          </div>
          <div className="rounded-xl bg-slate-100 p-1 dark:bg-gray-900">
            <div className="flex gap-1" role="group" aria-label="Time window">
              {GATEWAY_WINDOWS.map((option) => (
                <button
                  key={option}
                  type="button"
                  aria-pressed={timeWindow === option}
                  onClick={() => selectWindow(option)}
                  className={`rounded-lg px-3 py-2 text-sm font-semibold ${timeWindow === option ? 'bg-white text-blue-700 shadow-sm dark:bg-gray-700 dark:text-blue-300' : 'text-gray-600 dark:text-gray-400'}`}
                >
                  {option.toUpperCase()}
                </button>
              ))}
            </div>
            <div className="px-2 pt-1 text-right text-[11px] text-gray-600 dark:text-gray-400">{windowStatus}</div>
          </div>
        </div>
        {hasData && (
          <div aria-busy={switchingWindow} className={`mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-slate-200 pt-3 text-sm transition-opacity dark:border-gray-700 ${switchingWindow ? 'opacity-60' : ''}`}><span className="font-semibold text-gray-950 dark:text-white">{totals.total} gateways</span><span className="text-green-700 dark:text-green-300">● {totals.online} online</span><span className="text-yellow-700 dark:text-yellow-300">● {totals.degraded} degraded</span><span className="text-red-700 dark:text-red-300">● {totals.offline} offline</span>{totals.unknown > 0 && <span className="text-gray-600 dark:text-gray-300">● {totals.unknown} unknown</span>}{totals.retired > 0 && <span className="text-gray-500 dark:text-gray-400">{totals.retired} retired</span>}<span className="text-gray-600 dark:text-gray-300">{totals.vetted} vetted</span><span className="sm:ml-auto font-semibold text-indigo-700 dark:text-indigo-300">{avgCoverage > 0 ? <>{avgUptime.toFixed(1)}% uptime <span className="font-normal text-xs text-gray-500 dark:text-gray-400">({avgCoverage.toFixed(0)}% observed)</span></> : 'No uptime data'}</span></div>
        )}
      </header>

      {(hasData || currentError) && selection.warning && (
        <GatewayWarningPage
          warning={selection.warning}
          className="mb-4"
          action={selection.retry ? { label: retryBusy ? 'Retrying…' : 'Try again', onClick: retry, busy: retryBusy } : undefined}
        />
      )}

      {!hasData && !currentError && (
        <div role="status" className="flex min-h-[300px] items-center justify-center text-gray-500 dark:text-gray-400">Loading gateways…</div>
      )}

      {hasData && (<>
      <section className="mb-4 rounded-xl border border-gray-200 bg-white px-4 pt-4 shadow-sm dark:border-gray-700 dark:bg-gray-800 sm:px-5" aria-labelledby="uptime-trend-heading">
        <div className="flex flex-wrap items-baseline justify-between gap-2"><div><h2 id="uptime-trend-heading" className="text-base font-semibold text-gray-950 dark:text-white">Gateway availability trend ({timeWindow.toUpperCase()})</h2><p className="mt-0.5 text-xs text-gray-500 dark:text-gray-400">Each point combines every gateway poll snapshot recorded on that calendar day.</p></div>{uptimeTrend.length > 0 && <span className="text-xs text-gray-500 dark:text-gray-400">{uptimeTrend.length} calendar day{uptimeTrend.length === 1 ? '' : 's'} with data</span>}</div>
        {uptimeTrend.length > 0 ? (
          <ErrorBoundary fallback={<div className="flex h-[210px] items-center justify-center"><ReloadMessage message="The availability chart could not be loaded." /></div>}>
            <Suspense fallback={<div className="flex h-[210px] items-center justify-center text-sm text-gray-500 dark:text-gray-400">Loading chart…</div>}>
              <UptimeTrendChart option={uptimeTrendOption} />
            </Suspense>
          </ErrorBoundary>
        ) : (
          <div className="flex h-[210px] items-center justify-center text-sm text-gray-500 dark:text-gray-400">{trendPlaceholder}</div>
        )}
      </section>

      <section aria-busy={switchingWindow} className={`mb-4 overflow-hidden rounded-xl border border-gray-200 bg-white shadow-md transition-opacity dark:border-gray-700 dark:bg-gray-800 ${switchingWindow ? 'opacity-60' : ''}`}>
        <div className="border-b border-gray-200 p-4 dark:border-gray-700 sm:p-5">
          <div className="flex flex-col justify-between gap-3 lg:flex-row lg:items-end"><div><h2 className="text-lg font-semibold text-gray-950 dark:text-white">Gateway directory</h2><p className="mt-1 text-sm text-gray-500 dark:text-gray-400">{filteredRows.length} of {rows.length} gateways · every row includes its 30-bucket availability strip, ordered oldest to newest.</p></div><div className="flex flex-col gap-2 sm:flex-row"><input value={gatewaySearch} onChange={(event) => setGatewaySearch(event.target.value)} aria-label="Search gateways" placeholder="Search gateway, node, endpoint" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 outline-none focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-900 dark:text-white" /><select value={gatewaySort} onChange={(event) => { const nextSort = event.target.value as GatewaySort; updateView({ sort: nextSort === 'freshness' ? null : nextSort, dir: null }); }} aria-label="Sort gateways" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700 dark:border-gray-600 dark:bg-gray-900 dark:text-gray-200"><option value="freshness">Last seen</option><option value="status">Status</option><option value="uptime">Uptime</option><option value="activity">Activity</option></select><button type="button" onClick={() => { const nextDirection = sortDirection === 'asc' ? 'desc' : 'asc'; updateView({ dir: nextDirection === defaultDirection(gatewaySort) ? null : nextDirection }); }} className="rounded-lg border border-blue-300 bg-blue-50 px-3 py-2 text-sm font-semibold text-blue-800 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-200" title={`Sort ${sortDirection === 'asc' ? 'ascending' : 'descending'}`}>{sortDirection === 'asc' ? '↑ Asc' : '↓ Desc'}</button></div></div>
          <div className="mt-3 flex flex-wrap gap-2 pb-1" role="group" aria-label="Filter by status">{GATEWAY_FILTERS.map((filter) => <button key={filter} type="button" aria-pressed={gatewayFilter === filter} onClick={() => updateView({ status: filter === 'all' ? null : filter })} className={`whitespace-nowrap rounded-full border px-3 py-1.5 text-xs font-medium capitalize ${gatewayFilter === filter ? 'border-blue-600 bg-blue-600 text-white' : 'border-gray-300 text-gray-600 dark:border-gray-600 dark:text-gray-300'}`}>{filter === 'all' ? 'All active' : filter} ({filter === 'all' ? totals.total : totals[filter]})</button>)}</div>
          <div className="mt-3 flex flex-wrap gap-x-3 gap-y-1 text-xs text-gray-500 dark:text-gray-400"><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-green-500" />Online</span><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-yellow-500" />Degraded</span><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-red-500" />Offline</span><span><i className="mr-1 inline-block h-2 w-2 rounded-sm bg-gray-300 dark:bg-gray-600" />Unknown</span></div>
        </div>
        {/* Phones get one stacked card per gateway instead of a 960px table behind a sideways scroll */}
        <div className="sm:overflow-x-auto">
        <table className="block w-full text-left text-sm text-gray-500 dark:text-gray-400 sm:table sm:min-w-[960px] sm:table-fixed">
          <colgroup className="hidden sm:table-column-group">
            <col className="w-[22%]" />
            <col className="w-[28%]" />
            <col className="w-[17%]" />
            <col className="w-[17%]" />
            <col className="w-[16%]" />
          </colgroup>
          <thead className="hidden bg-slate-50 text-xs uppercase tracking-wide text-gray-600 dark:bg-gray-700/70 dark:text-gray-300 sm:table-header-group">
            <tr>
              <th scope="col" className="px-4 py-3 sm:px-5">Gateway</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Availability</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Activity</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Trust & history</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Endpoint</th>
            </tr>
          </thead>
          <tbody className="block sm:table-row-group">
            {filteredRows.length === 0 && (
              <tr className="block bg-white border-b dark:bg-gray-800 dark:border-gray-700 sm:table-row">
                <td colSpan={5} className="block px-4 sm:px-6 py-6 text-center text-gray-500 dark:text-gray-400 sm:table-cell">
                  {gatewayFilter !== 'all' || gatewaySearch.trim()
                    ? 'No gateways match these filters.'
                    : totals.retired > 0
                      ? `No active gateways. ${totals.retired} retired ${totals.retired === 1 ? 'one is' : 'ones are'} under the Retired filter.`
                      : 'No gateways to show.'}
                </td>
              </tr>
            )}

            {visibleRows.map((gateway) => (
              <tr
                key={gateway.gateway_id}
                className="block py-2 align-top border-b border-gray-200 bg-white transition-colors last:border-b-0 hover:bg-blue-50/40 dark:border-gray-700 dark:bg-gray-800 dark:hover:bg-blue-950/20 sm:table-row sm:py-0"
              >
                <td className="block px-4 py-2 sm:table-cell sm:px-5 sm:py-4">
                  <div className="truncate font-semibold text-gray-950 dark:text-white" title={gateway.lightning_alias || 'Unnamed Gateway'}>
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
                <td className="block px-4 py-2 sm:table-cell sm:px-5 sm:py-4">
                  <div className="flex items-center justify-between gap-3">
                    <span className={`rounded-full px-2.5 py-1 text-xs font-semibold ${statusClasses(gateway.status)}`}>
                      {gateway.status}
                    </span>
                    <span className="whitespace-nowrap text-xs text-gray-500 dark:text-gray-400">{gateway.lastSeenDate ? `seen ${formatRelative(gateway.lastSeenDate, now)}` : 'never seen'}</span>
                  </div>
                  <div className="mt-2 flex items-baseline justify-between gap-3">
                    <span className="font-semibold text-gray-950 dark:text-white">{gateway.coveragePct > 0 ? `${gateway.estimatedUptimePct.toFixed(1)}% uptime` : 'No samples'}</span>
                    <span className="text-xs text-gray-500 dark:text-gray-400">{gateway.coveragePct.toFixed(0)}% observed</span>
                  </div>
                  {/* One stop for keyboards and screen readers instead of 30 per row */}
                  <div
                    className="mt-2.5 flex gap-0.5"
                    role="img"
                    aria-label={`${dataWindow.toUpperCase()} availability: ${gateway.coveragePct > 0 ? `${gateway.estimatedUptimePct.toFixed(1)}% uptime` : 'no samples'}, ${gateway.coveragePct.toFixed(0)}% observed`}
                  >
                    {(uptimeStripByGatewayId.get(gateway.gateway_id) ?? []).map((status, index) => {
                      const tooltip = getUptimeBucketTooltip(status, index, 30, windowMinutes);
                      // On phones the strip spans the whole card, so more tooltips anchor to an edge
                      const tooltipPosition = index < 4
                        ? 'left-0'
                        : index > 25
                          ? 'right-0'
                          : index < 10
                            ? 'left-0 sm:left-1/2 sm:-translate-x-1/2'
                            : index > 19
                              ? 'right-0 sm:right-auto sm:left-1/2 sm:-translate-x-1/2'
                              : 'left-1/2 -translate-x-1/2';

                      return (
                        <span
                          key={`${gateway.gateway_id}-${index}`}
                          title={tooltip}
                          className="group relative h-3.5 flex-1 cursor-help rounded-[3px]"
                        >
                          <span className={`block h-full rounded-[3px] transition-transform duration-150 group-hover:scale-y-125 ${getUptimeStripClass(status)}`} />
                          <span className={`pointer-events-none absolute bottom-full z-20 mb-2 hidden w-max rounded-md bg-gray-950 px-2 py-1 text-[11px] font-medium text-white shadow-lg group-hover:block dark:bg-black ${tooltipPosition}`}>
                            {tooltip}
                          </span>
                        </span>
                      );
                    })}
                  </div>
                  <div className="mt-2 flex flex-wrap gap-x-2 text-xs text-gray-500 dark:text-gray-400"><span>On {formatCompactDuration(gateway.estimatedOnlineMinutes)}</span><span>Off {formatCompactDuration(gateway.estimatedOfflineMinutes)}</span><span>Unknown {formatCompactDuration(gateway.estimatedUnknownMinutes)}</span></div>
                </td>
                <td className="block px-4 py-2 text-gray-700 dark:text-gray-300 sm:table-cell sm:px-5 sm:py-4">
                  <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400 sm:hidden">Activity</div>
                  {gateway.realActivityScore !== null ? (
                    <>
                      <div className="font-semibold text-gray-950 dark:text-white">
                        {gateway.realActivityScore.toLocaleString()} <span className="cursor-help text-xs font-normal text-gray-500 dark:text-gray-400" title="Activity score: funds + 3 × settles − 1.5 × cancels, plus a small bonus for volume">score</span>
                      </div>
                      <div className="mt-1 text-xs leading-5 text-gray-500 dark:text-gray-400">
                        {metricsWindow} · {gateway.fundCountWindow} funds · {gateway.settleCountWindow} settles · {gateway.cancelCountWindow} cancels<br />
                        {formatMsats(gateway.totalVolumeMsatWindow)} volume
                      </div>
                    </>
                  ) : (
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      No activity recorded in {metricsWindow}
                    </div>
                  )}
                </td>
                <td className="block px-4 py-2 sm:table-cell sm:px-5 sm:py-4">
                  <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400 sm:hidden">Trust & history</div>
                  <span className={`text-sm font-medium ${gateway.vetted ? 'text-green-600 dark:text-green-400' : 'text-gray-500 dark:text-gray-400'}`}>
                    {gateway.vetted ? '✓ Vetted' : 'Not vetted'}
                  </span>
                  <div className="mt-2 text-xs text-gray-500 dark:text-gray-400">First seen<br /><span className="text-gray-700 dark:text-gray-300">{formatDateTime(gateway.firstSeenDate)}</span></div>
                  <div className="mt-2 text-xs text-gray-500 dark:text-gray-400">Last seen<br /><span className="text-gray-700 dark:text-gray-300">{formatDateTime(gateway.lastSeenDate)}</span></div>
                </td>
                <td className="block px-4 py-2 sm:table-cell sm:px-5 sm:py-4">
                  <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400 sm:hidden">Endpoint</div>
                  {/* Gateway APIs (iroh:// or /v1 endpoints) are not pages, so show them as text to copy */}
                  <span
                    title={gateway.api_endpoint}
                    className="block select-all truncate font-mono text-xs text-gray-700 dark:text-gray-300"
                  >
                    {gateway.api_endpoint}
                  </span>
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
              Show {Math.min(INITIAL_RENDER_COUNT, filteredRows.length - visibleRows.length)} more ({filteredRows.length - visibleRows.length} remaining)
            </button>
          </div>
        )}
      </section>
      </>)}
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
