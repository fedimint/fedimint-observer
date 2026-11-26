import { memo, useMemo } from 'react';
import { Link } from 'react-router-dom';
import type { FederationActivity, FederationHealth } from '../../../types/api';
import { Badge } from '../../../components/ui/Badge';
import { Icon } from '../../../components/ui/Icon';
import { asBitcoin } from '../../../utils/format';
import { formatUtcDate, formatUtcDateTime } from '../../../utils/time';
import { observedSessionSpan } from '../lib/federationList';
import { CombinedMiniChart } from './MiniChart';
import { MIGRATION_HELP_URL, showMigrationHelp } from '../../../utils/migration';

interface FederationRowProps {
  id: string;
  name: string;
  totalAssets: number | null;
  firstSessionTimestamp?: number | null;
  lastSessionTimestamp?: number | null;
  secondaryColumn: 'assets' | 'operational';
  health: FederationHealth;
  expiryTimestamp?: number | null;

  successorId?: string | null;
  successorName?: string | null;
  activityData: FederationActivity[];
  maxTransaction?: number;  // global max for consistent chart scale
  maxVolume?: number;        // global max for consistent chart scale, in msats
}

const HEALTH_MESSAGES = {
  degraded: 'Some guardians are offline or out of sync',
  offline: 'Fewer guardians answer the observer than consensus needs',
} as const;

// Stable formatter references across row renders.
const formatTransaction = (value: number) => Math.round(value).toString();
const formatVolume = (msats: number) => asBitcoin(msats, 8);

export const FederationRow = memo(function FederationRow({
  id,
  name,
  totalAssets,
  firstSessionTimestamp,
  lastSessionTimestamp,
  secondaryColumn,
  health,
  expiryTimestamp,
  successorId,
  successorName,
  activityData,
  maxTransaction,
  maxVolume,
}: FederationRowProps) {
  const transactionData = useMemo(() => activityData.map(d => d.num_transactions), [activityData]);
  const volumeData = useMemo(() => activityData.map(d => d.amount_transferred), [activityData]);

  const dates = useMemo(() => activityData.map((_, index) => {
    const daysAgo = activityData.length - 1 - index;
    if (daysAgo === 0) return 'Today (UTC)';
    if (daysAgo === 1) return 'Yesterday (UTC)';
    return `${daysAgo} days ago (UTC)`;
  }), [activityData]);

  const showWarning = health === 'degraded' || health === 'offline';
  const badgeLevel = health === 'degraded' ? 'warning' : 'error';
  const healthMessage = showWarning ? HEALTH_MESSAGES[health] : '';
  const isShutDown = !!expiryTimestamp && expiryTimestamp * 1000 <= Date.now();
  const sessionSpan = observedSessionSpan(firstSessionTimestamp, lastSessionTimestamp);
  const shutDown = secondaryColumn === 'operational';

  return (
    <Link
      to={`/federations/${id}`}
      className="block bg-surface hover:bg-surface-2 px-3 sm:px-6 py-4 text-xs sm:text-sm"
    >
      {/* Shut-down rows end after the operating range; live rows add the chart below/on the right. */}
      <div className={`grid grid-cols-[1.5fr_1fr] gap-3 items-start ${shutDown ? 'md:grid-cols-2' : 'md:grid-cols-3'}`}>
        <div className="font-medium text-ink min-w-0">
          <span className="meta-label block mb-1 md:hidden">Name</span>
          <div className="flex items-center gap-1.5 min-w-0">
            <span className="font-medium text-ink wrap-break-word min-w-0">
              {name}
            </span>
            {showWarning && <Badge level={badgeLevel} tooltip={healthMessage} showIcon />}
          </div>
          {/* Always on its own line under the name, never broken, so every row and screen width shows it the same way */}
          {expiryTimestamp && successorId ? (
            <span className="mt-2 flex items-center gap-1 min-w-0">
              <span
                title={`${isShutDown ? 'Moved' : 'Migrating'} to ${successorName ?? 'a new federation'}; ${isShutDown ? 'shut down' : 'shuts down'} on ${formatUtcDateTime(expiryTimestamp * 1000)}`}
                className={`inline-flex min-w-0 items-center gap-1.5 rounded-md px-2 py-0.5 text-xs font-medium leading-4 ring-1 ring-inset ${
                  isShutDown
                    ? 'bg-surface-3 text-ink-2 ring-line-strong/60'
                    : 'bg-accent-soft text-accent-ink ring-accent/25'
                }`}
              >
                <Icon name="arrowRight" className="size-3" strokeWidth={2.2} />
                <span className="truncate">
                  {isShutDown ? 'Moved' : 'Migrating'}{successorName ? ` to ${successorName}` : ''}
                </span>
              </span>

              {!isShutDown && showMigrationHelp() && (
                <button
                  type="button"
                  aria-label="Why is this federation migrating?"
                  title="Why is this federation migrating?"
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    window.open(MIGRATION_HELP_URL, '_blank', 'noopener,noreferrer');
                  }}
                  className="inline-flex size-5 shrink-0 items-center justify-center rounded-full border border-line-strong text-xs font-semibold text-ink-2 hover:bg-surface-3"
                >
                  ?
                </button>
              )}
            </span>
          ) : expiryTimestamp ? (
            <span
              title={`${isShutDown ? 'Shut down' : 'Shuts down'} on ${formatUtcDateTime(expiryTimestamp * 1000)}`}
              className={`mt-2 inline-flex items-center gap-1.5 whitespace-nowrap rounded-md px-2 py-0.5 text-xs font-medium leading-4 ring-1 ring-inset ${
                isShutDown
                  ? 'bg-crit-soft text-crit-ink ring-crit/25'
                  : 'bg-warn-soft text-warn-ink ring-warn/30'
              }`}
            >
              <span aria-hidden="true" className={`size-1.5 shrink-0 rounded-full ${isShutDown ? 'bg-crit' : 'bg-warn'}`} />
              {isShutDown ? 'Shut down' : `Shuts down ${formatUtcDate(expiryTimestamp * 1000)}`}
            </span>
          ) : null}
        </div>

        {/* Assets for live lists; observed operating range for shut-down federations */}
        <div className="text-right md:text-left">
          <span className="meta-label block mb-1 md:hidden">
            {shutDown ? 'Observed session dates (UTC)' : 'Total assets'}
          </span>
          <span
            className="tabular text-ink whitespace-nowrap md:whitespace-normal"
            title={shutDown ? sessionSpan.title : undefined}
          >
            {shutDown
              ? sessionSpan.text
              : totalAssets === null ? 'Awaiting balance' : asBitcoin(totalAssets, 6)}
          </span>
        </div>

        {!shutDown && <div className="col-span-2 md:col-span-1">
          <span className="meta-label block mb-1 md:hidden">Activity, last 7 days</span>
          <CombinedMiniChart
            transactionData={transactionData}
            volumeData={volumeData}
            dates={dates}
            formatTransaction={formatTransaction}
            formatVolume={formatVolume}
            maxTransaction={maxTransaction}
            maxVolume={maxVolume}
          />
        </div>}
      </div>
    </Link>
  );
});
