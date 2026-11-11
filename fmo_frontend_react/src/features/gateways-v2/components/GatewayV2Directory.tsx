import { useCallback, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { api } from '../../../services/api';
import type { GatewayV2Registry, GatewayWindow } from '../../../types/api';
import { errorMessage } from '../../../utils/errors';
import { useCursorPages } from '../../../hooks/useCursorPages';
import { CursorPager } from '../../../components/ui/CursorPager';
import { usePageSize } from '../../../hooks/usePageSize';
import { FilterChips } from '../../../components/ui/FilterChips';
import { StatTile } from '../../../components/ui/StatTile';
import { card, muted } from '../../../components/ui/styles';
import { GATEWAY_V2_FILTERS, observedDate, visibleGateways, type GatewayV2Filter, type GatewayV2Sort } from '../lib/gatewaysV2';
import { GatewayV2AvailabilityTrend } from './GatewayV2AvailabilityTrend';
import { GatewayV2Row } from './GatewayV2Row';
import { GatewayV2Detail } from './GatewayV2Detail';

const REFRESH_MS = 60_000;
const nextAfter = (registry: GatewayV2Registry) => registry.next_after;

interface GatewayV2DirectoryProps {
  id: string;
  instance: number;
  timeWindow: GatewayWindow;
}

/** One Lightning v2 module's gateways: headline figures, the availability trend, and the paged directory */
export function GatewayV2Directory({ id, instance, timeWindow }: GatewayV2DirectoryProps) {
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<GatewayV2Filter>('all');
  const [sort, setSort] = useState<GatewayV2Sort>('name');
  const [pageSize, setPageSize] = usePageSize('gateways-v2', 25);
  const fetchPage = useCallback(
    (after: string | undefined, signal: AbortSignal) => api.getGatewayV2Registry(id, instance, timeWindow, pageSize, after, signal),
    [id, instance, timeWindow, pageSize],
  );
  const pages = useCursorPages({ fetchPage, nextCursor: nextAfter, refreshMs: REFRESH_MS });
  const registry = pages.page;
  const error = pages.error && `${
    pages.error.kind === 'refresh' ? 'Could not refresh gateways' : 'Could not load that page of gateways'
  }: ${errorMessage(pages.error.cause)}`;

  const gateways = registry?.gateways ?? [];
  // Every figure covers all of the module's gateways, as counted by the server
  const totals = registry?.totals;
  const samples = totals?.samples ?? 0;
  const successful = totals?.online_samples ?? 0;
  const medianLatency = totals?.median_latency_ms ?? null;
  const visible = visibleGateways(gateways, filter, search, sort);

  // The selected gateway opens in the side panel, kept in the URL by its endpoint
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedEndpoint = searchParams.get('gateway');
  const selected = gateways.find((gateway) => gateway.api_endpoint === selectedEndpoint) ?? null;
  const selectGateway = (endpoint: string | null) => {
    const next = new URLSearchParams(window.location.search);
    if (endpoint === null) next.delete('gateway');
    else next.set('gateway', endpoint);
    setSearchParams(next, { replace: endpoint === null });
  };

  return (
    <>
      {error && <p role="alert" className="mb-4 rounded-lg bg-amber-50 p-4 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-300">{error}. <button type="button" className="underline" onClick={pages.restart}>Try again</button></p>}
      <div className="mb-4 grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatTile label="Registered gateways" value={totals ? totals.registered.toLocaleString() : '—'} detail={registry?.next_after || pages.onLaterPage ? 'Across all pages; use the arrows for more' : 'Registered with the federation'} />
        <StatTile label="Online now" value={totals ? totals.online.toLocaleString() : '—'} detail={totals ? `${totals.degraded} degraded · ${totals.offline} offline · ${totals.unknown} unknown` : '—'} />
        <StatTile label={`${timeWindow.toUpperCase()} probe success`} value={samples ? `${(successful / samples * 100).toFixed(1)}%` : '—'} detail={samples ? `${samples.toLocaleString()} recorded checks` : 'History starts with the first probe'} />
        <StatTile label="Median response" value={medianLatency === null ? '—' : `${medianLatency.toLocaleString()} ms`} detail="Latest successful API checks" />
      </div>
      {registry && !registry.activity_caught_up && <p role="status" className={`mb-3 text-xs ${muted}`}>Historical contract activity is being indexed. Totals may increase as the backfill catches up.</p>}
      <GatewayV2AvailabilityTrend trend={registry?.availability_trend ?? []} timeWindow={timeWindow} />
      {registry && (
        <div className={`mb-4 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs ${muted}`}>
          <span>Registry: {registry.successful_guardians}/{registry.total_guardians} guardians answered</span>
          <span>Updated {observedDate(registry.observed_at)}</span>
          {registry.stale && <span className="text-amber-600 dark:text-amber-300">Registry collection delayed</span>}
          {registry.successful_guardians < registry.total_guardians && <span>Previous registrations are retained during partial coverage.</span>}
        </div>
      )}
      <section className={`${card} overflow-hidden`}>
        <div className="border-b border-gray-200 p-4 dark:border-gray-700 sm:p-5">
          <div className="flex flex-col justify-between gap-3 lg:flex-row lg:items-center">
            <div>
              <h2 className="text-lg font-semibold">Gateway directory</h2>
              <p className={`mt-1 text-xs ${muted}`}>{visible.length} of {gateways.length} gateways on this page · {timeWindow.toUpperCase()} observed history · select a gateway for its keys, fees and activity</p>
            </div>
            <div className="flex flex-col gap-2 sm:flex-row">
              <input value={search} onChange={(event) => setSearch(event.target.value)} aria-label="Search gateways" placeholder="Search this page" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm outline-hidden focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-900" />
              <select value={sort} onChange={(event) => setSort(event.target.value as GatewayV2Sort)} aria-label="Sort gateways" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm dark:border-gray-600 dark:bg-gray-900">
                <option value="name">Gateway name</option>
                <option value="status">Status</option>
                <option value="latency">Response time</option>
                <option value="activity">Contract activity</option>
              </select>
            </div>
          </div>
          <FilterChips
            className="mt-4"
            label="Filter gateways by status"
            options={GATEWAY_V2_FILTERS}
            value={filter}
            onChange={setFilter}
            renderLabel={(status) => <>{status === 'all' ? 'All active' : status}{totals && ` (${status === 'all' ? totals.registered : totals[status]})`}</>}
          />
        </div>
        {!registry && !error && <p className={`p-8 text-center text-sm ${muted}`}>Loading gateway details…</p>}
        {registry && visible.length === 0 && <p className={`p-8 text-center text-sm ${muted}`}>{gateways.length ? 'No gateways match this view.' : registry.observed_at ? 'No registered gateways have been reported.' : 'Collecting gateway registrations…'}</p>}
        <div className="divide-y divide-gray-200 dark:divide-gray-700">
          {visible.map((gateway) => <GatewayV2Row key={gateway.api_endpoint} gateway={gateway} timeWindow={timeWindow} onSelect={() => selectGateway(gateway.api_endpoint)} />)}
        </div>
        {registry && (
          <div className="border-t border-gray-200 p-3 empty:hidden dark:border-gray-700">
            <CursorPager label="Gateway pages" page={pages.pageNumber} hasNext={pages.hasNext} loading={pages.loadingPage} onPrevious={pages.previous} onNext={pages.next} pageSize={pageSize} onPageSizeChange={setPageSize} total={totals?.registered} />
          </div>
        )}
      </section>
      <p className={`mt-3 text-xs ${muted}`}>Online means the gateway answered a routing-info check and supports this federation. Contract counts describe public federation activity, not payment routing capacity.</p>
      {selected && <GatewayV2Detail gateway={selected} timeWindow={timeWindow} onClose={() => selectGateway(null)} />}
      {pages.onLaterPage && <button type="button" onClick={pages.restart} className="mt-2 text-sm text-blue-600 dark:text-blue-400">Back to page 1 and resume refresh</button>}
    </>
  );
}
