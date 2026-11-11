import { Icon, type IconName } from '../../../components/ui/Icon';
import { EmptyState } from '../../../components/ui/States';
import { formatUtcDateTime } from '../../../utils/time';

export interface TimelineEvent {
  id: string;
  /** Epoch ms; live events carry the snapshot time */
  at: number;
  end?: number;
  ongoing?: boolean;
  tone: 'crit' | 'warn' | 'info' | 'ok' | 'manifold';
  title: string;
  detail?: string;
  /** Live observation or derived from stored history */
  kind: 'live' | 'historical';
}

const TONE: Record<TimelineEvent['tone'], { icon: IconName; dot: string }> = {
  crit: { icon: 'xCircle', dot: 'bg-crit-soft text-crit-ink ring-crit/30' },
  warn: { icon: 'alert', dot: 'bg-warn-soft text-warn-ink ring-warn/30' },
  info: { icon: 'info', dot: 'bg-accent-soft text-accent-ink ring-accent/25' },
  ok: { icon: 'checkCircle', dot: 'bg-ok-soft text-ok-ink ring-ok/25' },
  manifold: { icon: 'key', dot: 'bg-manifold-soft text-manifold-ink ring-manifold/25' },
};

function duration(ms: number): string {
  const hours = Math.round(ms / 3_600_000);
  return hours >= 48 ? `${Math.round(hours / 24)} days` : `${hours} ${hours === 1 ? 'hour' : 'hours'}`;
}

export function EventTimeline({ events, emptyText }: { events: readonly TimelineEvent[]; emptyText: string }) {
  if (events.length === 0) return <EmptyState icon="checkCircle" title={emptyText} />;
  const sorted = [...events].sort((a, b) => b.at - a.at);
  return (
    <ol className="relative space-y-3 before:absolute before:top-2 before:bottom-2 before:left-[11px] before:w-px before:bg-line">
      {sorted.map((event) => {
        const tone = TONE[event.tone];
        return (
          <li key={event.id} className="relative flex gap-3">
            <span className={`relative z-10 inline-flex size-6 shrink-0 items-center justify-center rounded-full ring-1 ring-inset ${tone.dot}`}>
              <Icon name={tone.icon} className="size-3" strokeWidth={2.2} />
            </span>
            <div className="min-w-0 pb-0.5">
              <p className="text-sm text-ink">{event.title}</p>
              <p className="mt-0.5 text-xs text-ink-3">
                <span className="font-semibold uppercase tracking-wide">{event.kind === 'live' ? 'Now' : 'History'}</span>
                {' · '}
                {event.end
                  ? `${formatUtcDateTime(event.at)} → ${event.ongoing ? 'still ongoing' : formatUtcDateTime(event.end)} (${duration(event.end - event.at)}${event.ongoing ? ' so far' : ''})`
                  : formatUtcDateTime(event.at)}
              </p>
              {event.detail && <p className="mt-0.5 text-xs text-ink-3">{event.detail}</p>}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
