import type { GuardianHealth } from '../../../types/api';
import { guardianThreshold, type ConfigGuardian, type FederationConfig } from '../../../utils/config';
import { Badge } from '../../../components/ui/Badge';
import { Panel } from '../../../components/ui/Panel';
import { RetryButton } from '../../../components/ui/RetryButton';
import type { GuardianHealthStatus } from '../hooks/useGuardianHealth';

interface GuardiansPanelProps {
  config: FederationConfig | null;
  health: Record<string, GuardianHealth>;
  status: GuardianHealthStatus;
  onRetry: () => void;
  className?: string;
}

function GuardianStatus({ health, status }: { health: GuardianHealth | undefined; status: GuardianHealthStatus }) {
  if (status === 'loading') {
    return (
      <span className="text-xs sm:text-sm font-medium text-gray-500 dark:text-gray-400">
        Loading...
      </span>
    );
  }
  if (!health) {
    return (
      <Badge
        level="warning"
        tooltip={status === 'failed'
          ? 'The guardian status request failed'
          : 'The observer has no status recorded for this guardian'}
      >
        Status unknown
      </Badge>
    );
  }

  const isOnline = health.latest != null;
  const sessionOutdated = health.latest?.session_outdated || false;
  const blockOutdated = health.latest?.block_outdated || false;
  const block = health.latest?.block_height ?? null;
  return (
    <>
      <Badge level={isOnline ? 'success' : 'error'}>
        {isOnline ? 'Online' : 'Offline'}
      </Badge>
      <Badge level="info">
        {health.software_version || 'Version unknown'}
      </Badge>
      {isOnline && (
        <>
          <Badge
            level={sessionOutdated ? 'warning' : 'info'}
            tooltip={sessionOutdated ? 'Guardian is lacking behind others' : undefined}
          >
            Session {health.latest?.session_count || 0}
          </Badge>
          <Badge
            level={blockOutdated ? 'warning' : 'info'}
            tooltip={blockOutdated ? "Guardian's bitcoind is out of sync" : undefined}
          >
            {block === null ? 'Block height unavailable' : `Block ${block}`}
          </Badge>
        </>
      )}
    </>
  );
}

function GuardianRow({ guardian, health, status }: { guardian: ConfigGuardian; health: GuardianHealth | undefined; status: GuardianHealthStatus }) {
  return (
    <div className="border-b border-gray-200 dark:border-gray-700 pb-3 sm:pb-4 last:border-0">
      <div className="font-medium text-sm sm:text-base text-gray-900 dark:text-white mb-1">
        Guardian {guardian.id}
      </div>
      <div className="text-xs sm:text-sm text-gray-600 dark:text-gray-400 mb-2 break-all">
        {guardian.url}
      </div>
      <div className="flex gap-2 flex-wrap">
        <GuardianStatus health={health} status={status} />
      </div>
    </div>
  );
}

/** Each guardian of the federation with its endpoint, status, version and sync state */
export function GuardiansPanel({ config, health, status, onRetry, className }: GuardiansPanelProps) {
  return (
    <Panel
      className={className}
      title={<>
        Guardians
        {config && (
          <span className="ml-2 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
            Consensus needs {guardianThreshold(config.guardians.length)} of {config.guardians.length} guardians
          </span>
        )}
      </>}
    >
      {status === 'failed' && (
        <div className="mb-3 flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg bg-red-50 p-3 text-xs text-red-800 dark:bg-gray-900/60 dark:text-red-400 sm:text-sm" role="alert">
          <span><span className="font-bold">Error: </span>Could not check the guardians' status.</span>
          <RetryButton onClick={onRetry} />
        </div>
      )}
      {config ? (
        <div className="space-y-3 sm:space-y-4">
          {config.guardians.map((guardian) => (
            <GuardianRow key={guardian.id} guardian={guardian} health={health[guardian.id.toString()]} status={status} />
          ))}
        </div>
      ) : (
        <div className="text-sm text-gray-500 dark:text-gray-400">Loading guardians...</div>
      )}
    </Panel>
  );
}
