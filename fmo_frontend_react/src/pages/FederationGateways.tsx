import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import type { GatewayUptimeTrendPoint } from '../types/api';
import { GatewayWarningPage } from '../features/gateways/components/GatewayWarningPage';
import { GatewayPageHeader, GatewaySummaryBar } from '../features/gateways/components/GatewayPageHeader';
import { UptimeTrendSection } from '../features/gateways/components/UptimeTrendSection';
import { GatewayDirectory } from '../features/gateways/components/GatewayDirectory';
import { RawAnnouncementDialog } from '../features/gateways/components/RawAnnouncementDialog';
import { useGatewayView } from '../features/gateways/hooks/useGatewayView';
import { useGatewayFederation } from '../features/gateways/hooks/useGatewayFederation';
import { useGatewayOverview } from '../features/gateways/hooks/useGatewayOverview';
import { useLiveGatewayLookup } from '../features/gateways/hooks/useLiveGatewayLookup';
import { selectGatewayData } from '../features/gateways/lib/gatewaySelection';
import {
  GATEWAYS_PER_BATCH,
  WINDOW_MINUTES,
  filterAndSortGateways,
  formatRelative,
  toGatewayRow,
} from '../features/gateways/lib/gatewayRows';

const EMPTY_TREND: GatewayUptimeTrendPoint[] = [];

export function FederationGateways() {
  // The router remounts this page per federation (App.tsx), so id never changes here
  const { id = '' } = useParams<{ id: string }>();
  const view = useGatewayView();
  const { timeWindow } = view;
  const { federation, failed: federationFailed, retry: retryFederation } = useGatewayFederation(id);
  const overview = useGatewayOverview(id, timeWindow);
  const { observed, trend, now } = overview;
  const federationOffline = federation?.health === 'offline';
  const live = useLiveGatewayLookup({
    invite: federation?.invite,
    offline: federationOffline,
    federationPending: federation === undefined && !federationFailed,
  });

  const [gatewaySearch, setGatewaySearch] = useState('');
  const [visibleGatewayCount, setVisibleGatewayCount] = useState(GATEWAYS_PER_BATCH);
  const [rawAnnouncement, setRawAnnouncement] = useState<{ gatewayName: string; raw: Record<string, unknown> } | null>(null);
  const closeRawAnnouncement = useCallback(() => setRawAnnouncement(null), []);
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
    federationFailed,
    federationOffline,
    dataWindow,
  }), [currentError, dataWindow, federationFailed, federationOffline, live.lookup, observed]);
  const gateways = selection.gateways;

  // Try again repeats only what failed, and is ignored while an attempt is running
  const retryBusy = overview.loading;
  const retry = () => {
    if (retryBusy) return;
    focusAfterRetry.current = true;
    overview.reload();
    if (federationFailed) retryFederation();
    live.retryIfFailed();
  };

  // When a retry succeeds, its banner disappears together with the focused Try again
  // button; put keyboard focus on the page heading instead of losing it
  useEffect(() => {
    if (focusAfterRetry.current && !overview.loading && document.activeElement === document.body) {
      focusAfterRetry.current = false;
      headingRef.current?.focus();
    }
  });

  const rows = useMemo(() => gateways.map((gateway) => toGatewayRow(gateway, now, windowMinutes)), [gateways, now, windowMinutes]);
  // Counted on the server over all the federation's recorded gateways, as of
  // computed_at; retired ones count only in `retired`. Live registry rows alone have none.
  const summary = observed?.summary ?? null;
  const filteredRows = useMemo(
    () => filterAndSortGateways(rows, view.filter, gatewaySearch, view.sort, view.direction),
    [rows, view.filter, gatewaySearch, view.sort, view.direction],
  );

  useEffect(() => {
    setVisibleGatewayCount(GATEWAYS_PER_BATCH);
  }, [view.filter, gatewaySearch, view.sort, view.direction]);

  if (federation === null && observed !== null && observed.gateways.length === 0) {
    return (
      <div className="flex min-h-[400px] flex-col items-center justify-center gap-3 text-center">
        <p className="text-gray-700 dark:text-gray-300">This federation is not tracked by the observer.</p>
        <Link to="/" className="text-sm text-blue-700 hover:underline dark:text-blue-300">← All federations</Link>
      </div>
    );
  }

  // A window switch shows the loading state rather than the previous window's trend
  const uptimeTrend = trend?.window === timeWindow ? trend.points : EMPTY_TREND;
  const metricsWindow = (rows.find((row) => row.metrics_window)?.metrics_window ?? dataWindow).toUpperCase();
  // Until the recorded gateways arrive there are no numbers to show, only the controls
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
    <div className="py-5 sm:py-8">
      <Link to={`/federations/${id}`} className="text-sm text-blue-700 hover:underline dark:text-blue-300">← Federation details</Link>
      <GatewayPageHeader
        headingRef={headingRef}
        name={federation?.name ?? null}
        nameLoading={federation === undefined && !federationFailed}
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
        <div role="status" className="flex min-h-[300px] items-center justify-center text-gray-500 dark:text-gray-400">Loading gateways…</div>
      )}

      {hasData && (
        <>
          <UptimeTrendSection timeWindow={timeWindow} points={uptimeTrend} placeholder={trendPlaceholder} />
          <GatewayDirectory
            rows={rows}
            filteredRows={filteredRows}
            visibleCount={visibleGatewayCount}
            onShowMore={() => setVisibleGatewayCount((count) => count + GATEWAYS_PER_BATCH)}
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
            onViewRaw={(gatewayName, raw) => setRawAnnouncement({ gatewayName, raw })}
          />
        </>
      )}
      {rawAnnouncement && (
        <RawAnnouncementDialog gatewayName={rawAnnouncement.gatewayName} raw={rawAnnouncement.raw} onClose={closeRawAnnouncement} />
      )}
    </div>
  );
}
