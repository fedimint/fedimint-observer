import { useMemo, useState, type KeyboardEvent } from 'react';
import type { GuardianHealthHistory, GuardianHealthWindow } from '../../../types/api';
import { Card, CardHeader } from '../../../components/ui/Card';
import { DataStamp } from '../../../components/ui/DataStamp';
import { Segmented } from '../../../components/ui/Segmented';
import { EmptyState, ErrorState, Skeleton } from '../../../components/ui/States';
import { formatUtcDate, formatUtcDayHour } from '../../../utils/time';
import { LEVEL_FILL, SLOT_DESCRIPTION, WINDOW_OPTIONS, describeSlot, slotRange } from '../lib/heatmap';
import { useGuardianHistory } from '../hooks/useGuardianObservability';
import {
  availabilityLevel,
  guardianSlots,
  heatmapGrouping,
  slotUptime,
  type AvailabilityLevel,
  type GuardianRow,
  type HistorySlot,
} from '../lib/guardianModel';
import { formatMs, formatPct } from '../lib/format';

const LEGEND: { level: AvailabilityLevel; label: string }[] = [
  { level: 'ok', label: '≥ 98%' },
  { level: 'minor', label: '90–98%' },
  { level: 'major', label: 'Below 90%' },
  { level: 'down', label: 'Offline' },
  { level: 'none', label: 'Not polled' },
];

interface HeatmapGridProps {
  history: GuardianHealthHistory;
  rows: readonly Pick<GuardianRow, 'id' | 'configName'>[];
  /** Hide the per-row label and summary, for a single guardian's strip */
  compact?: boolean;
}

/** The grid itself, reusable for one guardian (detail panel) or all of them */
export function HeatmapGrid({ history, rows, compact = false }: HeatmapGridProps) {
  const group = heatmapGrouping(history);
  const grid = useMemo(
    () => rows.map((row) => ({ row, slots: guardianSlots(history, row.id, group) })),
    [history, rows, group],
  );
  const columns = grid[0]?.slots.length ?? 0;
  const [active, setActive] = useState<{ r: number; c: number } | null>(null);
  const [focused, setFocused] = useState(false);

  const name = (row: Pick<GuardianRow, 'id' | 'configName'>) => (row.configName ? `Guardian ${row.id} (${row.configName})` : `Guardian ${row.id}`);
  const activeSlot = active ? grid[active.r]?.slots[active.c] : undefined;
  const activeText = active && activeSlot ? describeSlot(name(grid[active.r].row), activeSlot, history.window) : '';

  const onKeyDown = (event: KeyboardEvent) => {
    const current = active ?? { r: 0, c: columns - 1 };
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [0, -1], ArrowRight: [0, 1], ArrowUp: [-1, 0], ArrowDown: [1, 0],
    };
    if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      setActive({ r: current.r, c: event.key === 'Home' ? 0 : columns - 1 });
      return;
    }
    if (event.key === 'Escape') {
      setActive(null);
      return;
    }
    const move = moves[event.key];
    if (!move) return;
    event.preventDefault();
    setActive({
      r: Math.min(grid.length - 1, Math.max(0, current.r + move[0])),
      c: Math.min(columns - 1, Math.max(0, current.c + move[1])),
    });
  };

  const rowSummary = (slots: HistorySlot[]) => {
    const polled = slots.reduce((sum, slot) => sum + slot.polledS, 0);
    const online = slots.reduce((sum, slot) => sum + slot.onlineS, 0);
    return polled > 0 ? (online / polled) * 100 : null;
  };

  const ticks = columns > 0 ? [0, Math.floor(columns / 2), columns - 1] : [];
  const tooltipLeft = active ? ((active.c + 0.5) / columns) * 100 : 0;

  return (
    <div>
      <div
        role="group"
        tabIndex={0}
        aria-label={`Availability heatmap, ${history.window}, ${SLOT_DESCRIPTION[history.window]}. Use the arrow keys to read each block.`}
        onKeyDown={onKeyDown}
        onFocus={() => {
          setFocused(true);
          setActive((value) => value ?? { r: 0, c: columns - 1 });
        }}
        onBlur={() => {
          setFocused(false);
          setActive(null);
        }}
        onMouseLeave={() => !focused && setActive(null)}
        className="rounded-lg outline-offset-4"
      >
        <div className="space-y-[3px]">
          {grid.map(({ row, slots }, r) => {
            const summary = rowSummary(slots);
            return (
              <div key={row.id} className="flex items-center gap-1.5 sm:gap-2">
                {!compact && (
                  <div className="flex w-16 shrink-0 items-center gap-1.5 sm:w-36" title={name(row)}>
                    <span className="font-mono text-[0.68rem] font-semibold text-ink-3">{row.id}</span>
                    <span className="truncate text-xs text-ink-2">{row.configName ?? `Guardian ${row.id}`}</span>
                  </div>
                )}
                <div className="relative flex min-w-0 flex-1 gap-px sm:gap-[2px]" aria-hidden="true">
                  {slots.map((slot, c) => {
                    const level = availabilityLevel(slotUptime(slot));
                    const isActive = active?.r === r && active.c === c;
                    return (
                      <div
                        key={slot.start}
                        onMouseEnter={() => setActive({ r, c })}
                        className={`h-5 min-w-px flex-1 rounded-[1px] sm:h-6 sm:rounded-[2px] ${LEVEL_FILL[level]} ${
                          isActive ? 'ring-2 ring-ink ring-offset-1 ring-offset-surface' : ''
                        }`}
                      />
                    );
                  })}
                </div>
                {!compact && (
                  <div className="w-11 shrink-0 text-right text-xs font-medium text-ink-2 tabular sm:w-14">{formatPct(summary)}</div>
                )}
              </div>
            );
          })}
        </div>

        {/* Time axis */}
        <div className="mt-1.5 flex items-center gap-2 text-[0.65rem] text-ink-3">
          {!compact && <div className="w-16 shrink-0 sm:w-36" />}
          <div className="relative h-3.5 min-w-0 flex-1">
            {ticks.map((index, i) => {
              const slot = grid[0]?.slots[index];
              if (!slot) return null;
              const text = history.window === '24h' ? formatUtcDayHour(slot.start) : formatUtcDate(slot.start, { month: 'short', day: 'numeric' });
              const position = i === 0 ? 'left-0' : i === ticks.length - 1 ? 'right-0' : 'left-1/2 -translate-x-1/2';
              return <span key={index} className={`absolute whitespace-nowrap ${position}`}>{i === ticks.length - 1 ? 'Now' : text}</span>;
            })}
          </div>
          {!compact && <div className="w-11 shrink-0 sm:w-14" />}
        </div>

        {/* Tooltip for the active block */}
        <div className="relative">
          {active && activeSlot && (
            <div
              className="pointer-events-none absolute top-1 z-20 w-max max-w-[16rem] rounded-lg border border-line-strong bg-surface px-2.5 py-1.5 text-[0.7rem] leading-snug text-ink-2 shadow-lg"
              style={{ left: `clamp(0px, calc(${tooltipLeft}% - 8rem), calc(100% - 16rem))` }}
            >
              <div className="font-semibold text-ink">{name(grid[active.r].row)}</div>
              <div>{slotRange(activeSlot, history.window)}</div>
              <div className="tabular">
                {slotUptime(activeSlot) === null ? 'Not polled' : `${formatPct(slotUptime(activeSlot))} available · ${activeSlot.samples} polls`}
              </div>
              {activeSlot.latencyMs !== null && <div className="tabular">Mean latency {formatMs(activeSlot.latencyMs)}</div>}
            </div>
          )}
        </div>
      </div>
      <p aria-live="polite" className="sr-only">{focused ? activeText : ''}</p>
    </div>
  );
}

export function HeatmapLegend() {
  return (
    <ul className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[0.7rem] text-ink-3" aria-label="Legend">
      {LEGEND.map(({ level, label }) => (
        <li key={level} className="inline-flex items-center gap-1.5">
          <span className={`inline-block size-2.5 rounded-[2px] ${LEVEL_FILL[level]}`} aria-hidden="true" />
          {label}
        </li>
      ))}
    </ul>
  );
}

interface AvailabilityHeatmapProps {
  federationId: string;
  rows: readonly GuardianRow[];
  initialWindow?: GuardianHealthWindow;
  /** Extra classes for the card, e.g. h-full to match a neighbour's height */
  className?: string;
}

/** Each guardian's share of answered polls over time, with a time-range switch */
export function AvailabilityHeatmap({ federationId, rows, initialWindow = '7d', className }: AvailabilityHeatmapProps) {
  const [window, setWindow] = useState<GuardianHealthWindow>(initialWindow);
  const history = useGuardianHistory(federationId, window);

  return (
    <Card aria-labelledby="availability-title" className={className}>
      <CardHeader
        id="availability-title"
        title="Guardian availability"
        description={`Share of the observer's status polls each guardian answered (${SLOT_DESCRIPTION[window]})`}
        meta={<DataStamp kind="historical" at={history.data?.generated_at} scope={`${window} window`} />}
        actions={<Segmented label="Time range" options={WINDOW_OPTIONS} value={window} onChange={setWindow} />}
      />
      {history.failed ? (
        <ErrorState title="Could not load availability history" message="The observer did not return its health history." onRetry={history.reload} />
      ) : !history.data ? (
        <div role="status" aria-label="Loading availability history" className="space-y-[3px]">
          {Array.from({ length: Math.max(rows.length, 3) }, (_, i) => <Skeleton key={i} className="h-6 w-full" />)}
        </div>
      ) : rows.length === 0 ? (
        <EmptyState title="No guardians to show" />
      ) : history.data.guardians.length === 0 ? (
        <EmptyState icon="clock" title="No health samples in this window">
          The observer has not stored any polls of these guardians yet. Samples appear within a minute of the federation being observed.
        </EmptyState>
      ) : (
        <>
          <HeatmapGrid history={history.data} rows={rows} />
          <div className="mt-3 flex flex-col gap-2 border-t border-line pt-3 sm:flex-row sm:items-center sm:justify-between">
            <HeatmapLegend />
            <p className="text-[0.7rem] text-ink-3">
              Offline means the guardian did not answer the observer; it is not by itself a federation failure.
            </p>
          </div>
        </>
      )}
    </Card>
  );
}
