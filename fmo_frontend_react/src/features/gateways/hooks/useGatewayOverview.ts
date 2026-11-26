import { useEffect, useMemo, useReducer, useRef, useState } from 'react';
import { api } from '../../../services/api';
import type { GatewayInfo, GatewayListQuery, GatewayOverviewSummary, GatewayUptimeTrendPoint, GatewayWindow, ListPage } from '../../../types/api';
import { errorMessage, isNetworkError } from '../../../utils/errors';
import { initialPagedList, pagedListReducer, requestedCursors, shownPageNumber } from '../../../utils/pagedList';
import type { LoadError } from '../lib/gatewaySelection';

const REFRESH_INTERVAL_MS = 60_000;
const REFRESH_CHECK_MS = 10_000;
const CLOCK_MS = 30_000;
const STALE_ON_SHOW_MS = 30_000;


interface OverviewPage {
  window: GatewayWindow;
  gateways: GatewayInfo[];
  summary: GatewayOverviewSummary;
  page: ListPage | null;
  trend: GatewayUptimeTrendPoint[];
  computedAt: number;
}


export function useGatewayOverview(id: string, timeWindow: GatewayWindow, list: Omit<GatewayListQuery, 'after' | 'include'>) {
  const { search, status, sort, dir, limit } = list;
  const listKey = JSON.stringify([id, timeWindow, search, status, sort, dir, limit]);
  const [paged, dispatch] = useReducer(pagedListReducer<OverviewPage>, listKey, initialPagedList<OverviewPage>);
  if (paged.key !== listKey) dispatch({ type: 'query', key: listKey });
  const requested = requestedCursors(paged);
  const { attempt } = paged;

  const [error, setError] = useState<LoadError | null>(null);
  const [loading, setLoading] = useState(true);
  const [now, setNow] = useState(() => Date.now());
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
        dispatch({ type: 'failed', key: listKey, cursors: requested });
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
  }, [id, timeWindow, search, status, sort, dir, limit, listKey, requested, attempt]);

  // Keep statuses and "seen" times current, and refresh a minute after the last load finished. A load in flight is never interrupted, so slow windows can complete.
  useEffect(() => {
    const clock = setInterval(() => setNow(Date.now()), CLOCK_MS);
    const refresh = setInterval(() => {
      if (document.visibilityState !== 'visible' || loadInFlight.current) return;
      if (Date.now() - lastLoadAt.current >= REFRESH_INTERVAL_MS) dispatch({ type: 'refresh' });
    }, REFRESH_CHECK_MS);
    const onVisibilityChange = () => {
      if (document.visibilityState !== 'visible') return;
      setNow(Date.now());
      if (!loadInFlight.current && Date.now() - lastLoadAt.current > STALE_ON_SHOW_MS) dispatch({ type: 'refresh' });
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    return () => {
      clearInterval(clock);
      clearInterval(refresh);
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, []);

  const data = paged.data;
  const observed = useMemo(() => data && { window: data.window, gateways: data.gateways, summary: data.summary, page: data.page }, [data]);
  const trend = useMemo(() => data && { window: data.window, points: data.trend }, [data]);
  return {
    observed,
    trend,

    error,
    loading,
    updatedAt: data?.computedAt ?? null,
    now,

    reload: () => dispatch({ type: 'retry' }),

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
