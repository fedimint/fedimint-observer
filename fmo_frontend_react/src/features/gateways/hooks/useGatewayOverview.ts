import { useEffect, useRef, useState } from 'react';
import { api } from '../../../services/api';
import type { GatewayInfo, GatewayOverviewSummary, GatewayUptimeTrendPoint, GatewayWindow } from '../../../types/api';
import { errorMessage, isNetworkError } from '../../../utils/errors';
import type { LoadError } from '../lib/gatewaySelection';

// Gateways are polled and their overview rebuilt every 5 minutes on the server
const REFRESH_INTERVAL_MS = 60_000;
const REFRESH_CHECK_MS = 10_000;
const CLOCK_MS = 30_000;
// Showing the tab again refreshes data older than this
const STALE_ON_SHOW_MS = 30_000;

/**
 * The observer's recorded gateways for a window, with the summary and the
 * availability trend, refreshed a minute after each load while the page is
 * visible. `now` ticks along so statuses and "seen" times stay current.
 */
export function useGatewayOverview(id: string, timeWindow: GatewayWindow) {
  // Each response is stored with the window it belongs to, so a window switch never
  // recomputes the previous window's numbers against the new window's length
  const [observed, setObserved] = useState<{ window: GatewayWindow; gateways: GatewayInfo[]; summary: GatewayOverviewSummary } | null>(null);
  const [trend, setTrend] = useState<{ window: GatewayWindow; points: GatewayUptimeTrendPoint[] } | null>(null);
  const [error, setError] = useState<LoadError | null>(null);
  const [loading, setLoading] = useState(true);
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [reloadKey, setReloadKey] = useState(0); // Try again
  const [refreshKey, setRefreshKey] = useState(0); // background refresh
  const loadInFlight = useRef(false);
  const lastLoadAt = useRef(0); // when the last load finished

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;
    loadInFlight.current = true;
    setLoading(true);

    // The table and the trend come in one answer, so they always describe the same moment
    api.getFederationGatewayOverview(id, timeWindow, signal)
      .then((overview) => {
        if (signal.aborted) return;
        setObserved({ window: timeWindow, gateways: overview.gateways ?? [], summary: overview.summary });
        setTrend({ window: timeWindow, points: overview.uptime_trend ?? [] });
        setError(null);
        setUpdatedAt(Date.parse(overview.computed_at));
      })
      .catch((err: unknown) => {
        if (signal.aborted) return;
        setError({
          window: timeWindow,
          message: errorMessage(err),
          network: isNetworkError(err),
        });
      })
      .finally(() => {
        if (signal.aborted) return;
        setLoading(false);
        loadInFlight.current = false;
        lastLoadAt.current = Date.now();
      });

    return () => {
      controller.abort();
      loadInFlight.current = false;
    };
  }, [id, timeWindow, reloadKey, refreshKey]);

  // Keep statuses and "seen" times current, and refresh a minute after the last load
  // finished. A load in flight is never interrupted, so slow windows can complete.
  useEffect(() => {
    const clock = setInterval(() => setNow(Date.now()), CLOCK_MS);
    const refresh = setInterval(() => {
      if (document.visibilityState !== 'visible' || loadInFlight.current) return;
      if (Date.now() - lastLoadAt.current >= REFRESH_INTERVAL_MS) setRefreshKey((key) => key + 1);
    }, REFRESH_CHECK_MS);
    const onVisibilityChange = () => {
      if (document.visibilityState !== 'visible') return;
      setNow(Date.now());
      if (!loadInFlight.current && Date.now() - lastLoadAt.current > STALE_ON_SHOW_MS) setRefreshKey((key) => key + 1);
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      clearInterval(clock);
      clearInterval(refresh);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, []);

  return {
    observed,
    trend,
    /** The latest failure, for whichever window it happened in */
    error,
    loading,
    updatedAt,
    now,
    reload: () => setReloadKey((key) => key + 1),
  };
}
