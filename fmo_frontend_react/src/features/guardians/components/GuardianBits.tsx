import { Link } from 'react-router-dom';
import type { ManifoldSeatBindingsReport } from '../../../types/api';
import { seatBindingsSummary } from '../lib/seatBindings';
import { StatusPill, StatusDot, type Tone } from '../../../components/ui/Status';
import { Icon } from '../../../components/ui/Icon';
import type { GuardianRow, GuardianState } from '../lib/guardianModel';
import { formatPct } from '../lib/format';
import { availabilityLevel } from '../lib/guardianModel';

const STATE: Record<GuardianState, { tone: Tone; label: string; title: string }> = {
  online: { tone: 'ok', label: 'Online', title: 'Answered the observer within the last 5 minutes' },
  offline: { tone: 'crit', label: 'Offline', title: 'Polled recently, but has not answered for more than 5 minutes' },
  unknown: { tone: 'neutral', label: 'Unknown', title: 'The observer has no recent poll of this guardian' },
};

export function GuardianStatePill({ state, size }: { state: GuardianState; size?: 'sm' | 'md' }) {
  const { tone, label, title } = STATE[state];
  return <StatusPill tone={tone} title={title} size={size}>{label}</StatusPill>;
}

interface GuardianIdentityProps {
  row: GuardianRow;
  /** Makes the name a button, e.g. to open the guardian's details */
  onSelect?: () => void;
  /** Link the Manifold name to its profile; off inside other interactive elements */
  linkManifold?: boolean;
}

/** Peer id and config name; the Manifold identity only when a verified binding names one */
export function GuardianIdentity({ row, onSelect, linkManifold = true }: GuardianIdentityProps) {
  const name = <span className="truncate text-sm font-medium text-ink">{row.configName ?? `Guardian ${row.id}`}</span>;
  const manifold = row.binding && (
    <>
      <Icon name="key" className="size-3" />
      <span className="truncate font-mono">{row.binding.display_name}</span>
    </>
  );
  return (
    <div className="min-w-0">
      <div className="flex items-center gap-1.5">
        <span className="inline-flex h-5 min-w-5 items-center justify-center rounded bg-surface-3 px-1 font-mono text-[0.68rem] font-semibold text-ink-2" title="Fedimint peer id">
          {row.id}
        </span>
        {onSelect ? (
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              onSelect();
            }}
            className="min-w-0 truncate rounded text-left hover:underline"
            aria-label={`Details for guardian ${row.id}${row.configName ? `, ${row.configName}` : ''}`}
          >
            {name}
          </button>
        ) : name}
      </div>
      {manifold && (linkManifold ? (
        <Link
          to={`/manifold/guardians/${row.binding!.fman_pubkey}`}
          onClick={(event) => event.stopPropagation()}
          className="mt-0.5 inline-flex max-w-full items-center gap-1 text-xs text-manifold-ink hover:underline"
          title="Manifold Guardian that signed this seat's binding"
        >
          {manifold}
        </Link>
      ) : (
        <span className="mt-0.5 inline-flex max-w-full items-center gap-1 text-xs text-manifold-ink">{manifold}</span>
      ))}
    </div>
  );
}

/** Uptime as a coloured dot and figure; the dot never carries the meaning alone */
export function UptimeValue({ value }: { value: number | null }) {
  const level = availabilityLevel(value);
  const tone: Tone = level === 'ok' ? 'ok' : level === 'minor' ? 'warn' : level === 'none' ? 'neutral' : 'crit';
  return (
    <span className="inline-flex items-center gap-1.5 tabular">
      {value !== null && <StatusDot tone={tone} />}
      <span className={value === null ? 'text-ink-3' : 'text-ink'}>{formatPct(value)}</span>
    </span>
  );
}

export function SeatBindingsPill({ report }: { report: ManifoldSeatBindingsReport | null }) {
  const { tone, label } = seatBindingsSummary(report);
  return <StatusPill tone={tone === 'ok' ? 'manifold' : tone} icon={tone === 'ok' ? 'checkCircle' : undefined}>{label}</StatusPill>;
}
