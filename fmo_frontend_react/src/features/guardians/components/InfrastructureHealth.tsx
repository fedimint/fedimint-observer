import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { Card, CardHeader } from '../../../components/ui/Card';
import { DataStamp } from '../../../components/ui/DataStamp';
import { Kpi } from '../../../components/ui/Kpi';
import { Icon } from '../../../components/ui/Icon';
import { Skeleton } from '../../../components/ui/States';
import { RetryButton } from '../../../components/ui/RetryButton';
import { StatusPill } from '../../../components/ui/Status';
import { formatRelative, formatUtcDateTime } from '../../../utils/time';
import type { GuardianObservability } from '../hooks/useGuardianObservability';
import { RESILIENCE, resilienceDetail } from '../lib/resilienceText';
import { attention } from '../lib/attention';
import { OperationalWarnings } from './OperationalWarnings';
import { seatBindingsExplanation, seatBindingsSummary } from '../lib/seatBindings';

interface InfrastructureHealthProps {
  federationId: string;
  observability: GuardianObservability;
  onSelectGuardian?: (id: number) => void;
}

/** Compact health of the federation's guardian infrastructure, for the overview */
export function InfrastructureHealth({ federationId, observability, onSelectGuardian }: InfrastructureHealthProps) {
  const { resilience: res, live, seatBindings, warnings, rows } = observability;
  // Ages the live status even when no new answer arrives, so stale data stops reading as current
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const clock = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(clock);
  }, []);
  const loading = live.loading && !live.data;
  const binding = seatBindingsSummary(seatBindings.data);
  const lastAnswer = rows.map((row) => row.live?.answered_at).filter((t): t is string => !!t).sort().at(-1) ?? null;
  const state = RESILIENCE[res.level];
  const attentionState = attention({
    warnings,
    res,
    liveGeneratedAt: live.data?.generated_at ?? null,
    liveFailed: live.failed,
    now,
  });

  return (
    <Card aria-labelledby="infra-health-title">
      <CardHeader
        id="infra-health-title"
        title="Infrastructure health"
        description="How the federation's guardians are doing right now, from the observer's own polls"
        meta={<DataStamp kind="live" at={live.data?.generated_at} staleAfterSeconds={180} now={now} />}
        actions={
          <Link to={`/federations/${encodeURIComponent(federationId)}/guardians`} className="inline-flex items-center gap-1 text-xs font-medium text-accent-ink hover:underline">
            All guardians <Icon name="chevronRight" className="size-3.5" />
          </Link>
        }
      />

      {live.failed && !live.data && (
        <div role="alert" className="mb-3 flex flex-wrap items-center gap-2 rounded-lg bg-crit-soft px-3 py-2 text-xs text-crit-ink">
          Could not load the live guardian status. <RetryButton onClick={live.reload} />
        </div>
      )}

      <div className="grid grid-cols-2 gap-2.5 xl:grid-cols-4">
        {loading ? (
          Array.from({ length: 4 }, (_, i) => <Skeleton key={i} className="h-[86px] w-full rounded-lg" />)
        ) : (
          <>
            <Kpi
              label="Reachable"
              value={<span className="tabular">{res.online}<span className="text-ink-3"> / {res.total}</span></span>}
              detail={res.offline > 0 ? `${res.offline} not answering` : res.unknown > 0 ? `${res.unknown} not recently polled` : 'All guardians answering the observer'}
              help="Guardians that answered the observer's status request in the last 5 minutes."
            />
            <Kpi
              label="Reachability vs threshold"
              value={<StatusPill tone={state.tone} size="md">{state.label}</StatusPill>}
              detail={`Consensus needs ${res.threshold} of ${res.total} · ${resilienceDetail(res)}`}
              help={`How many guardians answer the observer, against the ${res.threshold} of ${res.total} that consensus needs. It is the observer's view only: guardians the observer cannot reach may still reach each other, and guardians that answer it may still fail to make progress.`}
            />
            <Kpi
              label="Manifold seat bindings"
              value={seatBindings.data?.status === 'verified'
                ? <span className="tabular text-manifold-ink">{seatBindings.data.distinct_guardians}<span className="text-ink-3"> MG {seatBindings.data.distinct_guardians === 1 ? 'identity' : 'identities'}</span></span>
                : <StatusPill tone={binding.tone} size="md">{binding.tone === 'warn' ? 'Failed' : 'None'}</StatusPill>}
              detail={seatBindings.data?.status === 'verified' ? `Verified for all ${res.total} seats` : binding.label}
              help={`${seatBindingsExplanation(seatBindings.data)} This counts distinct Manifold Guardian (MG) identities, not independent operators: one MG identity may run several seats, and one operator may control several MG identities.`}
            />
            <Kpi
              label="Last answer"
              value={<span title={formatUtcDateTime(lastAnswer)}>{formatRelative(lastAnswer)}</span>}
              detail={lastAnswer ? formatUtcDateTime(lastAnswer) : 'No answer in the last hour'}
              help="The latest status request any guardian answered. The observer polls every guardian once a minute."
            />
          </>
        )}
      </div>

      <div className="mt-4">
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-3">Attention</h3>
        {loading ? <Skeleton className="h-8 w-full" /> : (
          attentionState.kind === 'clear' ? (
            <p className="flex items-center gap-2 text-xs text-ink-3">
              <Icon name="checkCircle" className="size-3.5 text-ok" />
              {attentionState.message}
            </p>
          ) : attentionState.kind === 'unknown' ? (
            <p className="flex items-center gap-2 text-xs text-ink-3">
              <Icon name="info" className="size-3.5" />
              {attentionState.message}
            </p>
          ) : (
            <>
              <OperationalWarnings warnings={attentionState.warnings} onSelectGuardian={onSelectGuardian} />
              {attentionState.note && <p className="mt-2 text-xs text-ink-3">{attentionState.note}</p>}
            </>
          )
        )}
      </div>
    </Card>
  );
}
