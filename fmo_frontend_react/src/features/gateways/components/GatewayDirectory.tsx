import { useMemo } from 'react';
import type { GatewayOverviewSummary, GatewayWindow } from '../../../types/api';
import { FilterChips } from '../../../components/ui/FilterChips';
import { buildUptimeStrip, GATEWAY_FILTERS, GATEWAYS_PER_BATCH, type GatewayFilter, type GatewaySort, type GatewayWithStatus, type SortDirection } from '../lib/gatewayRows';
import { GatewayTableRow } from './GatewayTableRow';

interface GatewayDirectoryProps {
  /** Every gateway, before filtering */
  rows: GatewayWithStatus[];
  /** The gateways matching the filters, in order */
  filteredRows: GatewayWithStatus[];
  /** How many of `filteredRows` to render */
  visibleCount: number;
  onShowMore: () => void;
  summary: GatewayOverviewSummary | null;
  search: string;
  onSearchChange: (search: string) => void;
  filter: GatewayFilter;
  onFilterChange: (filter: GatewayFilter) => void;
  sort: GatewaySort;
  onSortChange: (sort: GatewaySort) => void;
  direction: SortDirection;
  onReverseDirection: () => void;
  now: number;
  dataWindow: GatewayWindow;
  windowMinutes: number;
  metricsWindow: string;
  /** Dimmed while another window loads */
  switchingWindow: boolean;
  onViewRaw: (gatewayName: string, raw: Record<string, unknown>) => void;
}

function StatusLegend() {
  return (
    <div className="mt-3 flex flex-wrap gap-x-3 gap-y-1 text-xs text-gray-500 dark:text-gray-400">
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-green-500" />Online</span>
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-yellow-500" />Degraded</span>
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-red-500" />Offline</span>
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-gray-300 dark:bg-gray-600" />Unknown</span>
    </div>
  );
}

/** The searchable, filterable and sortable table of gateways, rendered in batches */
export function GatewayDirectory(props: GatewayDirectoryProps) {
  const { rows, filteredRows, visibleCount, summary, search, filter, sort, direction, switchingWindow } = props;
  const visibleRows = filteredRows.slice(0, visibleCount);
  const remaining = filteredRows.length - visibleRows.length;
  const strips = useMemo(() => new Map(rows.map((gateway) => [gateway.gateway_id, buildUptimeStrip(gateway)])), [rows]);

  return (
    <section aria-busy={switchingWindow} className={`mb-4 overflow-hidden rounded-xl border border-gray-200 bg-white shadow-md transition-opacity dark:border-gray-700 dark:bg-gray-800 ${switchingWindow ? 'opacity-60' : ''}`}>
      <div className="border-b border-gray-200 p-4 dark:border-gray-700 sm:p-5">
        <div className="flex flex-col justify-between gap-3 lg:flex-row lg:items-end">
          <div>
            <h2 className="text-lg font-semibold text-gray-950 dark:text-white">Gateway directory</h2>
            <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">{filteredRows.length} of {rows.length} gateways · every row includes its 30-bucket availability strip, ordered oldest to newest.</p>
          </div>
          <div className="flex flex-col gap-2 sm:flex-row">
            <input value={search} onChange={(event) => props.onSearchChange(event.target.value)} aria-label="Search gateways" placeholder="Search gateway, node, endpoint" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 outline-hidden focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-900 dark:text-white" />
            <select value={sort} onChange={(event) => props.onSortChange(event.target.value as GatewaySort)} aria-label="Sort gateways" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm text-gray-700 dark:border-gray-600 dark:bg-gray-900 dark:text-gray-200">
              <option value="freshness">Last seen</option>
              <option value="status">Status</option>
              <option value="uptime">Uptime</option>
              <option value="activity">Activity</option>
            </select>
            <button type="button" onClick={props.onReverseDirection} className="rounded-lg border border-blue-300 bg-blue-50 px-3 py-2 text-sm font-semibold text-blue-800 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-200" title={`Sort ${direction === 'asc' ? 'ascending' : 'descending'}`}>
              {direction === 'asc' ? '↑ Asc' : '↓ Desc'}
            </button>
          </div>
        </div>
        <FilterChips
          className="mt-3 pb-1"
          label="Filter by status"
          options={GATEWAY_FILTERS}
          value={filter}
          onChange={props.onFilterChange}
          renderLabel={(option) => <>{option === 'all' ? 'All active' : option}{summary && ` (${option === 'all' ? summary.total : summary[option]})`}</>}
        />
        <StatusLegend />
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
                  {filter !== 'all' || search.trim()
                    ? 'No gateways match these filters.'
                    : summary && summary.retired > 0
                      ? `No active gateways. ${summary.retired} retired ${summary.retired === 1 ? 'one is' : 'ones are'} under the Retired filter.`
                      : 'No gateways to show.'}
                </td>
              </tr>
            )}

            {visibleRows.map((gateway) => (
              <GatewayTableRow
                key={gateway.gateway_id}
                gateway={gateway}
                strip={strips.get(gateway.gateway_id) ?? []}
                now={props.now}
                dataWindow={props.dataWindow}
                windowMinutes={props.windowMinutes}
                metricsWindow={props.metricsWindow}
                onViewRaw={props.onViewRaw}
              />
            ))}
          </tbody>
        </table>
      </div>
      {remaining > 0 && (
        <div className="border-t border-gray-200 p-4 text-center dark:border-gray-700">
          <button
            type="button"
            onClick={props.onShowMore}
            className="rounded-lg border border-blue-300 bg-blue-50 px-4 py-2 text-sm font-semibold text-blue-800 hover:bg-blue-100 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-200"
          >
            Show {Math.min(GATEWAYS_PER_BATCH, remaining)} more ({remaining} remaining)
          </button>
        </div>
      )}
    </section>
  );
}
