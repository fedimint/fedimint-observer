import { useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { api } from '../../../services/api';
import type { GatewayInfo, GatewayListQuery, GatewayOverviewSummary, GatewayUptimeTrendPoint, GatewayWindow, ListPage } from '../../../types/api';
import { errorMessage, isNetworkError } from '../../../utils/errors';
import { initialPagedList, pagedListReducer, requestedCursors, shownPageNumber } from '../../../utils/pagedList';
import type { LoadError } from '../lib/gatewaySelection';

// Gateways are polled and their overview rebuilt every 5 minutes on the server
const REFRESH_INTERVAL_MS = 60_000;
const REFRESH_CHECK_MS = 10_000;
const CLOCK_MS = 30_000;
// Showing the tab again refreshes data older than this
const STALE_ON_SHOW_MS = 30_000;

/** One answer: the table and the trend always describe the same moment and window */
interface OverviewPage {
  window: GatewayWindow;
  gateways: GatewayInfo[];
  summary: GatewayOverviewSummary;
  page: ListPage | null;
  trend: GatewayUptimeTrendPoint[];
  computedAt: number;
}

/**
 * The observer's recorded gateways for a window, with the summary and the
 * availability trend, refreshed a minute after each load while the page is
 * visible. `now` ticks along so statuses and "seen" times stay current.
 *
 * The gateway list comes one page at a time, searched, filtered and ordered
 * by the backend as `list` asks; `next` and `previous` request another page,
 * and the page number only moves once that page has arrived.
 */
export function useGatewayOverview(id: string, timeWindow: GatewayWindow, list: Omit<GatewayListQuery, 'after' | 'include'>) {
  const { search, status, sort, dir, limit } = list;
  // Another window or list query is another list: back to its page 1
  const listKey = JSON.stringify([id, timeWindow, search, status, sort, dir, limit]);
  const [paged, dispatch] = useReducer(pagedListReducer<OverviewPage>, listKey, initialPagedList<OverviewPage>);
  if (paged.key !== listKey) dispatch({ type: 'query', key: listKey });
  const requested = requestedCursors(paged);
  const { attempt } = paged;

  const [error, setError] = useState<LoadError | null>(null);
  const [loading, setLoading] = useState(true);
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

    api.getFederationGatewayOverview(id, timeWindow, { search, status, sort, dir, limit, after: requested.at(-1) }, signal)
      .then((overview) => {
        if (signal.aborted) return;
        dispatch({
          type: 'loaded',
          key: listKey,
          cursors: requested,
          data: {
            window: timeWindow,
            gateways: overview.gateways ?? [],
            summary: overview.summary,
            page: overview.page ?? null,
            trend: overview.uptime_trend ?? [],
            computedAt: Date.parse(overview.computed_at),
          },
        });
        setError(null);
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
    // `listKey` is derived from the query values listed here
  }, [id, timeWindow, search, status, sort, dir, limit, listKey, requested, attempt, reloadKey, refreshKey]);

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

  const data = paged.data;
  // Kept per answer, so views that memoize on them only recompute when one arrives
  const observed = useMemo(() => data && { window: data.window, gateways: data.gateways, summary: data.summary, page: data.page }, [data]);
  const trend = useMemo(() => data && { window: data.window, points: data.trend }, [data]);
  return {
    observed,
    trend,
    /** The latest failure, for whichever window or page it happened in */
    error,
    loading,
    updatedAt: data?.computedAt ?? null,
    now,
    reload: () => setReloadKey((key) => key + 1),
    /** 1-based number of the page shown; moves only once the page asked for arrives */
    pageNumber: shownPageNumber(paged),
    hasNext: Boolean(data?.page?.next_after),
    next: () => {
      const cursor = data?.page?.next_after;
      if (cursor && !loading) dispatch({ type: 'next', cursor });
    },
    previous: () => {
      if (!loading) dispatch({ type: 'previous' });
    },
  };
}
