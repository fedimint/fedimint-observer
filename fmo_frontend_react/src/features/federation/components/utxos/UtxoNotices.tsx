import type { ReactNode } from 'react';
import type { FederationOverview } from '../../../../types/api';
import { RetryButton } from '../../../../components/ui/RetryButton';
import type { FederationUtxos } from '../../hooks/useFederationUtxos';

const tones = {
  neutral: 'border-gray-200 text-gray-600 dark:border-gray-700 dark:text-gray-300',
  warning: 'border-amber-200 bg-amber-50 text-amber-900 dark:border-amber-900/60 dark:bg-amber-950/40 dark:text-amber-200',
  problem: 'border-rose-200 bg-rose-50 text-rose-900 dark:border-rose-900/60 dark:bg-rose-950/40 dark:text-rose-200',
};

function Notice({ tone, children }: { tone: keyof typeof tones; children: ReactNode }) {
  return <div className={`border-b px-4 py-3 text-xs sm:px-6 ${tones[tone]}`}>{children}</div>;
}

interface UtxoNoticesProps {
  utxos: FederationUtxos;
  overview: FederationOverview | null;
  /** The federation's indexing progress could not be loaded */
  overviewFailed: boolean;
}

/** Why the comparison may be incomplete or disagree, above the rows */
export function UtxoNotices({ utxos, overview, overviewFailed }: UtxoNoticesProps) {
  const { error, guardians, comparedIds, threshold, findings, laggingGuardians } = utxos;
  const walletSessions = overview?.module_sessions.wallet;
  const walletBehind = overview != null && walletSessions != null && walletSessions < overview.session_count;

  return (
    <>
      {error && (
        <div className="flex flex-wrap items-center gap-x-2 gap-y-1 border-b border-red-200 bg-red-50 px-4 py-3 text-xs text-red-800 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-300 sm:px-6" role="alert">
          <span><span className="font-bold">Error: </span>Could not load the UTXOs: {error.message}</span>
          {error.retryable && <RetryButton onClick={utxos.retry} />}
        </div>
      )}

      {overviewFailed && !error && (
        <Notice tone="neutral">
          Could not check how far the observer's wallet history is indexed, so outputs from recent sessions may be missing.
        </Notice>
      )}

      {utxos.guardianSummaryUnavailable && (
        <Notice tone="warning">
          Guardian wallet data is unavailable right now, so outputs are only checked for having been spent on-chain.
        </Notice>
      )}

      {utxos.tooFewGuardians && (
        <Notice tone="warning">
          Only {comparedIds.length} of {guardians.length} guardians responded, but {threshold} must agree, so outputs cannot be verified right now.
        </Notice>
      )}

      {findings.length > 0 && <Notice tone="problem">{findings.join(' ')}</Notice>}

      {walletBehind && (
        <Notice tone="neutral">
          The observer's wallet history is indexed to session {walletSessions} of {overview.session_count}, so outputs from later sessions are not listed yet.
        </Notice>
      )}

      {laggingGuardians.length > 0 && (
        <Notice tone="neutral">
          Left out of the comparison because they are behind the federation:{' '}
          {laggingGuardians.map((guardian) => `guardian ${guardian.guardian_id} (session ${guardian.session_count})`).join(', ')}.
        </Notice>
      )}
    </>
  );
}
