import { useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { FederationSummary, GatewayUptimeTrendPoint } from '../../../types/api';
import { GatewayWarningPage } from './GatewayWarningPage';
import { GatewayPageHeader, GatewaySummaryBar } from './GatewayPageHeader';
import { UptimeTrendSection } from './UptimeTrendSection';
import { GatewayDirectory } from './GatewayDirectory';
import { GatewayDetail } from './GatewayDetail';
import { useGatewayView } from '../hooks/useGatewayView';
import { useGatewayOverview } from '../hooks/useGatewayOverview';
import { useLiveGatewayLookup } from '../hooks/useLiveGatewayLookup';
import { liveLookupGate, selectGatewayData } from '../lib/gatewaySelection';
import { GATEWAY_LIST } from '../lib/gatewayView';
import { api } from '../../../services/api';
import { useApiResource } from '../../../hooks/useApiResource';
import {
  buildUptimeStrip,
  WINDOW_MINUTES,
  formatRelative,
  toGatewayRow,
} from '../lib/gatewayRows';
import { CursorPager } from '../../../components/ui/CursorPager';
import { usePageSize } from '../../../hooks/usePageSize';
import { useDebouncedValue } from '../../../hooks/useDebouncedValue';

const EMPTY_TREND: GatewayUptimeTrendPoint[] = [];


export function GatewaysPanel({ id, federation }: { id: string; federation: FederationSummary }) {
  const view = useGatewayView();
  const { timeWindow } = view;
  const [gatewaySearch, setGatewaySearch] = useState('');
  const [pageSize, setPageSize] = usePageSize(GATEWAY_LIST);
  const search = useDebouncedValue(gatewaySearch);
  const overview = useGatewayOverview(id, timeWindow, {
    search,
    status: view.filter,
    sort: view.sort,
    dir: view.direction,
    limit: pageSize,
  });
  const { observed, trend, now } = overview;
  const recheck = useApiResource((signal) => api.recheckFederationSummary(id, signal), [id], {
    enabled: federation.health === 'offline',
    label: 'federation health',
  });
  const gate = liveLookupGate(federation.health, { health: recheck.data?.health, loading: recheck.loading });
  const federationOffline = gate === 'offline';
  const live = useLiveGatewayLookup(federation.invite, gate);

  const [searchParams, setSearchParams] = useSearchParams();
  const selectedId = searchParams.get('gateway');
  const selectGateway = (gatewayId: string | null) => {
    const next = new URLSearchParams(window.location.search);
    if (gatewayId === null) next.delete('gateway');
    else next.set('gateway', gatewayId);
    setSearchParams(next, { replace: gatewayId === null });
  };
  const focusAfterRetry = useRef(false);
  const headingRef = useRef<HTMLHeadingElement>(null);

  // An error only describes the window it happened in, never one that is still loading
  const currentError = overview.error?.window === timeWindow ? overview.error : null;
  const dataWindow = observed?.window ?? timeWindow;
  const windowMinutes = WINDOW_MINUTES[dataWindow];
  const selection = useMemo(() => selectGatewayData({
    observed: observed?.gateways ?? null,
    observedError: currentError,
    live: live.lookup,
    federationFailed: false,
    federationOffline,
    dataWindow,
    listNarrowed: Boolean(search.trim()) || view.filter !== 'all' || overview.pageNumber > 1,
  }), [currentError, dataWindow, federationOffline, live.lookup, observed, search, view.filter, overview.pageNumber]);
  const gateways = selection.gateways;

  const retryBusy = overview.loading;
  const retry = () => {
    if (retryBusy) return;
    focusAfterRetry.current = true;
    overview.reload();
    if (recheck.failed) recheck.reload();
    live.retryIfFailed();
  };

  useEffect(() => {
    if (focusAfterRetry.current && !overview.loading && document.activeElement === document.body) {
      focusAfterRetry.current = false;
      headingRef.current?.focus();
    }
  });

  const rows = useMemo(() => gateways.map((gateway) => toGatewayRow(gateway, now, windowMinutes)), [gateways, now, windowMinutes]);
  const summary = observed?.summary ?? null;

  const uptimeTrend = trend?.window === timeWindow ? trend.points : EMPTY_TREND;
  const selected = rows.find((row) => row.gateway_id === selectedId) ?? null;
  const metricsWindow = (rows.find((row) => row.metrics_window)?.metrics_window ?? dataWindow).toUpperCase();
  const hasData = observed !== null || gateways.length > 0;
  const switchingWindow = observed !== null && dataWindow !== timeWindow && overview.loading;
  const trendPlaceholder = trend?.window === timeWindow
    ? 'No gateway poll history is available for this window.'
    : overview.loading
      ? 'Loading availability trend…'
      : currentError ? 'The availability trend could not be loaded.' : '';
  const windowStatus = overview.loading && (observed === null || dataWindow !== timeWindow)
    ? `Loading ${timeWindow.toUpperCase()}…`
    : observed === null
      ? ' '
      : dataWindow !== timeWindow
        ? `Showing ${dataWindow.toUpperCase()}`
        : overview.updatedAt ? `Updated ${formatRelative(new Date(overview.updatedAt), now)}` : 'Refreshing…';

  return (
    <div>
      <GatewayPageHeader
        headingRef={headingRef}
        timeWindow={timeWindow}
        onSelectWindow={view.selectWindow}
        windowStatus={windowStatus}
      >
        {hasData && <GatewaySummaryBar summary={summary} liveCount={rows.length} switchingWindow={switchingWindow} />}
      </GatewayPageHeader>

      {(hasData || currentError) && selection.warning && (
        <GatewayWarningPage
          warning={selection.warning}
          className="mb-4"
          action={selection.retry ? { label: retryBusy ? 'Retrying…' : 'Try again', onClick: retry, busy: retryBusy } : undefined}
        />
      )}

      {!hasData && !currentError && (
        <div role="status" className="flex min-h-[300px] items-center justify-center text-ink-3">Loading gateways…</div>
      )}

      {hasData && (
        <>
          <UptimeTrendSection timeWindow={timeWindow} points={uptimeTrend} placeholder={trendPlaceholder} />
          <GatewayDirectory
            rows={rows}
            matched={observed?.page?.total ?? rows.length}
            pager={
              <CursorPager
                label="Gateway pages"
                page={overview.pageNumber}
                hasNext={overview.hasNext}
                loading={overview.loading}
                onPrevious={overview.previous}
                onNext={overview.next}
                pageSize={pageSize}
                onPageSizeChange={setPageSize}
                total={observed?.page?.total}
              />
            }
            summary={summary}
            search={gatewaySearch}
            onSearchChange={setGatewaySearch}
            filter={view.filter}
            onFilterChange={view.selectFilter}
            sort={view.sort}
            onSortChange={view.selectSort}
            direction={view.direction}
            onReverseDirection={view.reverseDirection}
            now={now}
            dataWindow={dataWindow}
            windowMinutes={windowMinutes}
            metricsWindow={metricsWindow}
            switchingWindow={switchingWindow}
            onSelect={selectGateway}
          />
        </>
      )}
      {selected && (
        <GatewayDetail
          gateway={selected}
          strip={buildUptimeStrip(selected)}
          now={now}
          dataWindow={dataWindow}
          windowMinutes={windowMinutes}
          metricsWindow={metricsWindow}
          onClose={() => selectGateway(null)}
        />
      )}
    </div>
  );
}
