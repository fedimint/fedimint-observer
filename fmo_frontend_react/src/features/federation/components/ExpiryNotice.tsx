import type { FederationSummary } from '../../../types/api';
import { formatUtcDateTime } from '../../../utils/time';
import { isMigrating } from '../../../utils/migration';
import { MigrationNotice } from './MigrationNotice';

interface ExpiryNoticeProps {
  federation: FederationSummary;

  expiry: Date;

  successor: { id: string; name: string } | null;
}

/** Banner on a federation that is shutting down or has shut down, with its successor if it named one */
export function ExpiryNotice({ federation, expiry, successor }: ExpiryNoticeProps) {
  if (federation.successor && isMigrating(federation)) {
    return <MigrationNotice expiry={expiry} successorInvite={federation.successor} successor={successor} />;
  }

  const closed = expiry.getTime() <= Date.now();
  return (
    <div className={`mb-4 rounded-lg border p-3 text-sm ${
      closed
        ? 'border-crit/30 bg-crit-soft text-crit-ink'
        : 'border-warn/30 bg-warn-soft text-warn-ink'
    }`}>
      <strong>{closed ? 'Shut down' : 'Shutting down'}</strong>
      {' '}on {formatUtcDateTime(expiry)}.
      {federation.successor && (
        <>
          {' '}Successor: {/^https?:\/\//.test(federation.successor) ? (
            <a className="underline" href={federation.successor}>{federation.successor}</a>
          ) : (
            <span className="font-mono break-all">{federation.successor}</span>
          )}
        </>
      )}
    </div>
  );
}
