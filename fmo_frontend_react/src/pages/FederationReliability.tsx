import { useMemo, useState, type ReactNode } from 'react';
import { useParams } from 'react-router-dom';
import type { GuardianHealthWindow } from '../types/api';
import { Card, CardHeader } from '../components/ui/Card';
import { DataStamp } from '../components/ui/DataStamp';
import { Segmented } from '../components/ui/Segmented';
import { EmptyState, ErrorState, Skeleton } from '../components/ui/States';
import { FederationHeader } from '../features/federation/components/FederationHeader';
import { FederationPageError, FederationPageLoading } from '../features/federation/components/FederationPageState';
import { useFederationDetails } from '../features/federation/hooks/useFederationDetails';
import { useGuardianHistory, useGuardianObservability } from '../features/guardians/hooks/useGuardianObservability';
import { offlinePeriods } from '../features/guardians/lib/guardianModel';
import { WINDOW_OPTIONS } from '../features/guardians/lib/heatmap';
import {
  LatencyTrendChart,
  ReachabilityChart,
} from '../features/reliability/components/ReliabilityCharts';
import { EventTimeline, type TimelineEvent } from '../features/reliability/components/EventTimeline';

function ChartSkeleton({ height = 220 }: { height?: number }) {
  return (
    <div role="status" aria-label="Loading chart" style={{ height }}>
      <Skeleton className="h-full w-full rounded-lg" />
    </div>
  );
}

export function FederationReliability() {
  const { id = '' } = useParams<{ id: string }>();
  const details = useFederationDetails(id);
  const config = details.data?.config ?? null;
  const federation = details.data?.federation ?? null;
  const observability = useGuardianObservability(id, config);
  const { rows, resilience: res } = observability;
  const [window, setWindow] = useState<GuardianHealthWindow>('7d');
  const history = useGuardianHistory(id, window);
  const [trendGuardian, setTrendGuardian] = useState<number | null>(null);
  const trendRow = rows.find((row) => row.id === trendGuardian) ?? rows[0] ?? null;

  const events = useMemo<TimelineEvent[]>(() => {
    const list: TimelineEvent[] = [];
    if (history.data) {
      // Hourly windows count runs of 2+ hours; the 30-day window's days count from one
      const minSlots = history.data.window === '30d' ? 1 : 2;
      for (const row of rows) {
        for (const period of offlinePeriods(history.data, row.id, minSlots)) {
          list.push({
            id: `offline-${row.id}-${period.start}`,
            at: period.start,
            end: period.end,
            ongoing: period.ongoing,
            tone: period.ongoing ? 'crit' : 'warn',
            kind: 'historical',
            title: `Guardian ${row.id}${row.configName ? ` (${row.configName})` : ''} did not answer any poll`,
          });
        }
      }
    }
    return list;
  }, [history.data, rows]);

  if (details.loading && !details.data) return <FederationPageLoading />;
  if (details.failed || !federation) {
    return <FederationPageError error={details.error} notFound={!details.failed || (details.error instanceof Error && details.error.message === 'Federation not found')} onRetry={details.reload} />;
  }

  const historyBody = (render: () => ReactNode) =>
    history.failed ? (
      <ErrorState title="Could not load the health history" onRetry={history.reload} />
    ) : !history.data || rows.length === 0 ? (
      <ChartSkeleton />
    ) : history.data.guardians.length === 0 ? (
      <EmptyState icon="clock" title="No health samples in this window" />
    ) : render();

  return (
    <div>
      <FederationHeader
        id={id}
        federation={federation}
        config={config}
        actions={
          <div className="flex flex-col items-start gap-1.5 md:items-end">
            <Segmented label="Time range" options={WINDOW_OPTIONS} value={window} onChange={setWindow} />
            <DataStamp kind="historical" at={history.data?.generated_at} scope={`${window} window`} />
          </div>
        }
      />

      <div className="space-y-4 sm:space-y-5">
        <div className="grid gap-4 sm:gap-5 xl:grid-cols-5">
          <Card className="min-w-0 xl:col-span-3" aria-labelledby="reachability-title">
            <CardHeader
              id="reachability-title"
              title="Historical reachability"
              description="Guardians answering the observer's polls, averaged per period, against the consensus threshold"
            />
            {historyBody(() => <ReachabilityChart history={history.data!} rows={rows} res={res} />)}
          </Card>
          <Card className="min-w-0 xl:col-span-2" aria-labelledby="offline-title">
            <CardHeader
              id="offline-title"
              title="Offline periods"
              description="Guardians that answered none of the observer's polls for an extended period"
            />
            {!history.data && !history.failed ? <ChartSkeleton /> : (
              <div className="max-h-80 overflow-y-auto pr-1 scrollbar-thin">
                <EventTimeline events={events} emptyText={`No guardian was offline for an extended period in the last ${window}`} />
              </div>
            )}
          </Card>
        </div>

        <div>
          <Card className="min-w-0" aria-labelledby="latency-trend-title">
            <CardHeader
              id="latency-trend-title"
              title="Latency over time"
              description="Mean response time of one guardian per period"
              actions={rows.length > 0 && (
                <label className="inline-flex items-center gap-2 text-xs text-ink-3">
                  Guardian
                  <select
                    value={trendRow?.id ?? ''}
                    onChange={(event) => setTrendGuardian(Number(event.target.value))}
                    className="h-8 rounded-lg border border-line bg-surface-2 py-0 pr-8 pl-2 text-xs text-ink focus:border-accent focus:ring-0"
                  >
                    {rows.map((row) => (
                      <option key={row.id} value={row.id}>{row.id}{row.configName ? ` · ${row.configName}` : ''}</option>
                    ))}
                  </select>
                </label>
              )}
            />
            {historyBody(() => (trendRow ? <LatencyTrendChart history={history.data!} row={trendRow} /> : null))}
          </Card>
        </div>

      </div>
    </div>
  );
}
