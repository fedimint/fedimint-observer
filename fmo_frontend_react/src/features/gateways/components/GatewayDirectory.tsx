import { useMemo, type ReactNode } from 'react';
import type { GatewayOverviewSummary, GatewayWindow } from '../../../types/api';
import { FilterChips } from '../../../components/ui/FilterChips';
import { Button } from '../../../components/ui/Button';
import { SearchInput } from '../../../components/ui/SearchInput';
import { Select } from '../../../components/ui/Select';
import { buildUptimeStrip, GATEWAY_FILTERS, type GatewayFilter, type GatewaySort, type GatewayWithStatus, type SortDirection } from '../lib/gatewayRows';
import { GatewayTableRow } from './GatewayTableRow';
import { capitalize } from '../../../utils/format';

interface GatewayDirectoryProps {

  rows: GatewayWithStatus[];

  matched: number;

  pager: ReactNode;
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

  switchingWindow: boolean;
  onSelect: (gatewayId: string) => void;
}

function StatusLegend() {
  return (
    <div className="mt-3 flex flex-wrap gap-x-3 gap-y-1 text-xs text-ink-3">
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-ok" />Online</span>
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-warn" />Degraded</span>
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-crit" />Offline</span>
      <span><i className="mr-1 inline-block h-2 w-2 rounded-xs bg-line-strong" />Unknown</span>
    </div>
  );
}

/** The searchable, filterable and sortable table of gateways, rendered in batches */
export function GatewayDirectory(props: GatewayDirectoryProps) {
  const { rows, matched, summary, search, filter, sort, direction, switchingWindow } = props;
  const strips = useMemo(() => new Map(rows.map((gateway) => [gateway.gateway_id, buildUptimeStrip(gateway)])), [rows]);

  return (
    <section aria-busy={switchingWindow} className={`mb-4 overflow-hidden rounded-xl border border-line bg-surface shadow-[0_1px_2px_rgba(10,17,34,0.04)] transition-opacity ${switchingWindow ? 'opacity-60' : ''}`}>
      <div className="border-b border-line p-4 sm:p-5">
        <div className="flex flex-col justify-between gap-3 lg:flex-row lg:items-start">
          <div>
            <h2 className="text-base font-medium text-ink">Gateway directory</h2>
            <p className="mt-0.5 text-xs text-ink-3 sm:text-sm">{matched} {matched === 1 ? 'gateway' : 'gateways'} · Select a gateway for its keys, endpoint, history and raw announcement.</p>
          </div>
          <div className="flex flex-col gap-2 sm:flex-row">
            <SearchInput value={search} onChange={props.onSearchChange} label="Search gateways" placeholder="Search gateway, node, endpoint" className="sm:w-64" />
            <Select value={sort} onChange={(event) => props.onSortChange(event.target.value as GatewaySort)} aria-label="Sort gateways">
              <option value="freshness">Last seen</option>
              <option value="status">Status</option>
              <option value="uptime">Uptime</option>
              <option value="activity">Activity</option>
            </Select>
            <Button icon="sort" onClick={props.onReverseDirection} title={`Sort ${direction === 'asc' ? 'ascending' : 'descending'}`}>{direction === 'asc' ? 'Asc' : 'Desc'}</Button>
          </div>
        </div>
        <FilterChips
          className="mt-3 pb-1"
          label="Filter by status"
          options={GATEWAY_FILTERS}
          value={filter}
          onChange={props.onFilterChange}
          renderLabel={(option) => <>{option === 'all' ? 'All active' : capitalize(option)}{summary && ` (${option === 'all' ? summary.total : summary[option]})`}</>}
        />
        <StatusLegend />
      </div>

      <div className="sm:overflow-x-auto">
        <table className="block w-full text-left text-sm text-ink-3 sm:table sm:min-w-[760px] sm:table-fixed">
          <colgroup className="hidden sm:table-column-group">
            <col className="w-[30%]" />
            <col className="w-[36%]" />
            <col className="w-[20%]" />
            <col className="w-[14%]" />
          </colgroup>
          <thead className="hidden bg-surface-2 text-xs text-ink-2 sm:table-header-group">
            <tr>
              <th scope="col" className="px-4 py-3 sm:px-5">Gateway</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Availability</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Activity</th>
              <th scope="col" className="px-4 py-3 sm:px-5">Trust</th>
            </tr>
          </thead>
          <tbody className="block sm:table-row-group">
            {rows.length === 0 && (
              <tr className="block bg-surface border-b sm:table-row">
                <td colSpan={4} className="block px-4 sm:px-6 py-6 text-center text-ink-3 sm:table-cell">
                  {filter !== 'all' || search.trim()
                    ? 'No gateways match these filters.'
                    : summary && summary.retired > 0
                      ? `No active gateways. ${summary.retired} retired ${summary.retired === 1 ? 'one is' : 'ones are'} under the Retired filter.`
                      : 'No gateways to show.'}
                </td>
              </tr>
            )}

            {rows.map((gateway) => (
              <GatewayTableRow
                key={gateway.gateway_id}
                gateway={gateway}
                strip={strips.get(gateway.gateway_id) ?? []}
                now={props.now}
                dataWindow={props.dataWindow}
                windowMinutes={props.windowMinutes}
                metricsWindow={props.metricsWindow}
                onSelect={() => props.onSelect(gateway.gateway_id)}
              />
            ))}
          </tbody>
        </table>
      </div>
      <div className="border-t border-line p-3 sm:px-5">{props.pager}</div>
    </section>
  );
}
