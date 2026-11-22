import { useMemo, useState } from 'react';
import { Icon } from '../../../components/ui/Icon';
import { InfoTip } from '../../../components/ui/Tooltip';
import { StatusPill } from '../../../components/ui/Status';
import { formatRelative, formatUtcDateTime } from '../../../utils/time';
import type { GuardianRow, GuardianState } from '../lib/guardianModel';
import { formatMs } from '../lib/format';
import { GuardianIdentity, GuardianStatePill, UptimeValue } from './GuardianBits';

type SortKey = 'id' | 'state' | 'uptime7d' | 'uptime30d' | 'latency' | 'version' | 'lastSeen' | 'session';

const STATE_ORDER: Record<GuardianState, number> = { offline: 0, unknown: 1, online: 2 };

function sortValue(row: GuardianRow, key: SortKey): number | string | null {
  switch (key) {
    case 'id': return row.id;
    case 'state': return STATE_ORDER[row.state];
    case 'uptime7d': return row.uptime7d;
    case 'uptime30d': return row.uptime30d;
    case 'latency': return row.latencyMs;
    case 'version': return row.version;
    case 'lastSeen': return row.lastSeen ? Date.parse(row.lastSeen) : null;
    case 'session': return row.sessionCount;
  }
}

function sortRows(rows: readonly GuardianRow[], key: SortKey, ascending: boolean): GuardianRow[] {
  return [...rows].sort((a, b) => {
    const va = sortValue(a, key);
    const vb = sortValue(b, key);
    // Missing values sort last in either direction
    if (va === null || vb === null) return va === vb ? a.id - b.id : va === null ? 1 : -1;
    const order = typeof va === 'string' ? va.localeCompare(String(vb), undefined, { numeric: true }) : va - (vb as number);
    return (ascending ? order : -order) || a.id - b.id;
  });
}

function LastSeen({ row }: { row: GuardianRow }) {
  if (!row.lastSeen) return <span className="text-ink-3">Not in 90 days</span>;
  const text = row.lastSeenApproximate ? `≈ ${formatRelative(row.lastSeen)}` : formatRelative(row.lastSeen);
  return <span title={`${row.lastSeenApproximate ? 'Within the hour starting ' : ''}${formatUtcDateTime(row.lastSeen)}`}>{text}</span>;
}

function Session({ row }: { row: GuardianRow }) {
  if (row.sessionCount === null) return <span className="text-ink-3">—</span>;
  return (
    <span className="inline-flex flex-wrap items-center justify-end gap-1 tabular md:flex-nowrap">
      {row.sessionCount.toLocaleString('en-US')}
      {row.sessionLag !== null && row.sessionLag > 1 && <StatusPill tone="warn" title={`${row.sessionLag.toLocaleString('en-US')} sessions behind`}>−{row.sessionLag.toLocaleString('en-US')}</StatusPill>}
    </span>
  );
}

function ManifoldCell({ row }: { row: GuardianRow }) {
  if (!row.binding) return <span className="text-xs text-ink-3">Not bound</span>;
  return <StatusPill tone="manifold" icon="checkCircle">Binding verified</StatusPill>;
}

function Issues({ row }: { row: GuardianRow }) {
  if (row.warnings.length === 0) return <span className="text-ink-3" aria-label="No warnings">—</span>;
  const worst = row.warnings.some((w) => w.tone === 'warn' || w.tone === 'crit') ? 'warn' : 'info';
  return (
    <span title={row.warnings.map((w) => w.message).join('\n')}>
      <StatusPill tone={worst}>{row.warnings.length} {row.warnings.length === 1 ? 'issue' : 'issues'}</StatusPill>
    </span>
  );
}

interface ColumnHeaderProps {
  label: string;
  sortKey: SortKey;
  current: SortKey;
  ascending: boolean;
  onSort: (key: SortKey) => void;
  help?: string;
  align?: 'left' | 'right';
}

function ColumnHeader({ label, sortKey, current, ascending, onSort, help, align = 'left' }: ColumnHeaderProps) {
  const active = current === sortKey;
  return (
    <th scope="col" aria-sort={active ? (ascending ? 'ascending' : 'descending') : 'none'} className={`px-3 py-2.5 font-medium ${align === 'right' ? 'text-right' : 'text-left'}`}>
      <span className={`inline-flex items-center gap-1 ${align === 'right' ? 'flex-row-reverse' : ''}`}>
        <button type="button" onClick={() => onSort(sortKey)} className={`inline-flex items-center gap-1 rounded hover:text-ink ${active ? 'text-ink' : ''}`}>
          {label}
          <Icon name="sort" className={`size-3 ${active ? 'opacity-100' : 'opacity-40'}`} />
        </button>
        {help && <InfoTip label={`About ${label}`} align={align === 'right' ? 'end' : 'start'}>{help}</InfoTip>}
      </span>
    </th>
  );
}

interface GuardianTableProps {
  rows: readonly GuardianRow[];
  onSelect: (id: number) => void;
}

export function GuardianTable({ rows, onSelect }: GuardianTableProps) {
  const [sortKey, setSortKey] = useState<SortKey>('id');
  const [ascending, setAscending] = useState(true);
  const sorted = useMemo(() => sortRows(rows, sortKey, ascending), [rows, sortKey, ascending]);
  const onSort = (key: SortKey) => {
    if (key === sortKey) setAscending((value) => !value);
    else {
      setSortKey(key);
      // Worst first is the useful default for health figures
      setAscending(key === 'id' || key === 'state' || key === 'uptime7d' || key === 'uptime30d' || key === 'session' || key === 'version');
    }
  };
  const header = { current: sortKey, ascending, onSort };

  return (
    <>
      {/* Tablet and desktop */}
      <div className="hidden overflow-x-auto md:block scrollbar-thin">
        <table className="w-full min-w-[900px] text-sm">
          <caption className="sr-only">Guardians of this federation. Select a row for details.</caption>
          <thead className="border-b border-line text-[0.7rem] uppercase tracking-wide text-ink-3">
            <tr>
              <ColumnHeader label="Guardian" sortKey="id" {...header} help="Fedimint peer id and the name in the federation config. A Manifold identity shows only when a verified seat binding names one." />
              <ColumnHeader label="Status" sortKey="state" {...header} />
              <ColumnHeader label="7d" sortKey="uptime7d" align="right" {...header} help="Share of the observer's polls answered over the last 7 days." />
              <ColumnHeader label="30d" sortKey="uptime30d" align="right" {...header} help="Share of the observer's polls answered over the last 30 days." />
              <ColumnHeader label="Latency" sortKey="latency" align="right" {...header} help="Response time of the latest status request, measured by the observer." />
              <ColumnHeader label="Version" sortKey="version" {...header} />
              <ColumnHeader label="Last seen" sortKey="lastSeen" {...header} />
              <ColumnHeader label="Session" sortKey="session" align="right" {...header} help="Latest consensus session the guardian reports; the badge shows how far it trails the furthest guardian." />
              <th scope="col" className="px-3 py-2.5 text-left font-medium">Manifold</th>
              <th scope="col" className="px-3 py-2.5 text-left font-medium">Issues</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line">
            {sorted.map((row) => (
              <tr
                key={row.id}
                onClick={() => onSelect(row.id)}
                className="cursor-pointer align-middle transition-colors hover:bg-surface-2"
              >
                <td className="max-w-56 px-3 py-2.5">
                  <GuardianIdentity row={row} onSelect={() => onSelect(row.id)} />
                </td>
                <td className="px-3 py-2.5"><GuardianStatePill state={row.state} /></td>
                <td className="px-3 py-2.5 text-right"><UptimeValue value={row.uptime7d} /></td>
                <td className="px-3 py-2.5 text-right"><UptimeValue value={row.uptime30d} /></td>
                <td className="px-3 py-2.5 text-right text-ink-2 tabular">{formatMs(row.latencyMs)}</td>
                <td className="max-w-36 truncate px-3 py-2.5 font-mono text-xs text-ink-2" title={row.version ?? undefined}>{row.version ?? '—'}</td>
                <td className="px-3 py-2.5 text-xs text-ink-2"><LastSeen row={row} /></td>
                <td className="px-3 py-2.5 text-right text-ink-2"><Session row={row} /></td>
                <td className="px-3 py-2.5"><ManifoldCell row={row} /></td>
                <td className="px-3 py-2.5"><Issues row={row} /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Phones: stacked cards */}
      <ul className="space-y-2 md:hidden">
        {sorted.map((row) => (
          <li key={row.id}>
            <button
              type="button"
              onClick={() => onSelect(row.id)}
              className="w-full rounded-lg border border-line bg-surface-2 p-3 text-left hover:border-line-strong"
              aria-label={`Details for guardian ${row.id}${row.configName ? `, ${row.configName}` : ''}`}
            >
              <div className="flex items-start justify-between gap-2">
                <GuardianIdentity row={row} linkManifold={false} />
                <GuardianStatePill state={row.state} />
              </div>
              <dl className="mt-3 grid grid-cols-3 gap-2 text-xs">
                <div><dt className="text-ink-3">7d</dt><dd className="mt-0.5"><UptimeValue value={row.uptime7d} /></dd></div>
                <div><dt className="text-ink-3">30d</dt><dd className="mt-0.5"><UptimeValue value={row.uptime30d} /></dd></div>
                <div><dt className="text-ink-3">Latency</dt><dd className="mt-0.5 text-ink tabular">{formatMs(row.latencyMs)}</dd></div>
                <div><dt className="text-ink-3">Session</dt><dd className="mt-0.5 text-ink"><Session row={row} /></dd></div>
                <div><dt className="text-ink-3">Last seen</dt><dd className="mt-0.5 text-ink"><LastSeen row={row} /></dd></div>
                <div><dt className="text-ink-3">Version</dt><dd className="mt-0.5 truncate font-mono text-ink" title={row.version ?? undefined}>{row.version ?? '—'}</dd></div>
              </dl>
              {(row.binding || row.warnings.length > 0) && (
                <div className="mt-2.5 flex flex-wrap gap-1.5">
                  {row.binding && <ManifoldCell row={row} />}
                  {row.warnings.length > 0 && <Issues row={row} />}
                </div>
              )}
            </button>
          </li>
        ))}
      </ul>
    </>
  );
}
