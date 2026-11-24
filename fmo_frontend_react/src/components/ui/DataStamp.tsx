import { formatRelative, formatUtcDateTime } from '../../utils/time';
import { Icon } from './Icon';
import { StatusDot } from './Status';

type Kind = 'live' | 'historical' | 'demo';

interface DataStampProps {
  kind: Kind;

  at: string | number | Date | null | undefined;

  staleAfterSeconds?: number;

  scope?: string;
  now?: number;
}


export function DataStamp({ kind, at, staleAfterSeconds = 300, scope, now = Date.now() }: DataStampProps) {
  const time = at === null || at === undefined ? null : new Date(at);
  const ageSeconds = time ? (now - time.getTime()) / 1000 : null;
  const stale = kind === 'live' && (ageSeconds === null || ageSeconds > staleAfterSeconds);
  const exact = formatUtcDateTime(time, 'no observation yet');

  const label = kind === 'demo' ? 'Demo data' : kind === 'historical' ? 'Historical' : stale ? 'Stale' : 'Live';
  return (
    <span className="inline-flex flex-wrap items-center gap-x-1.5 gap-y-0.5 font-mono text-xs text-ink-3" title={exact}>
      {kind === 'live' && <StatusDot tone={stale ? 'warn' : 'ok'} pulse={!stale} />}
      {kind === 'historical' && <Icon name="clock" className="size-3" />}
      {kind === 'demo' && <Icon name="flask" className="size-3 text-demo" />}
      <span className={`font-medium ${stale ? 'text-warn-ink' : kind === 'demo' ? 'text-demo-ink' : 'text-ink-2'}`}>
        {label}
      </span>
      {scope && <span>· {scope}</span>}
      <span>
        · {time ? (kind === 'historical' ? `to ${exact}` : `as of ${formatRelative(time, now)}`) : 'no observation yet'}
      </span>
    </span>
  );
}
