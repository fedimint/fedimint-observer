import type { FederationSummary } from '../../../types/api';
import { formatUtcDateTime } from '../../../utils/time';
import { isMigrating } from '../../../utils/migration';
import { MigrationNotice } from './MigrationNotice';

interface ExpiryNoticeProps {
  federation: FederationSummary;
  /** When the federation's guardians said it shuts down */
  expiry: Date;
  /** The successor, when the observer tracks it */
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
        ? 'border-red-300 bg-red-50 text-red-800 dark:border-red-500/60 dark:bg-red-900/20 dark:text-red-300'
        : 'border-amber-300 bg-amber-50 text-amber-800 dark:border-amber-500/60 dark:bg-amber-900/20 dark:text-amber-300'
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
