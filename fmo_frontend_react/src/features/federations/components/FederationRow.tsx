import { memo, useMemo } from 'react';
import { Link } from 'react-router-dom';
import type { FederationActivity, FederationHealth } from '../../../types/api';
import { Badge } from '../../../components/ui/Badge';
import { asBitcoin } from '../../../utils/format';
import { formatUtcDate, formatUtcDateTime } from '../../../utils/time';
import { CombinedMiniChart } from './MiniChart';
import { MIGRATION_HELP_URL, showMigrationHelp } from '../../../utils/migration';

interface FederationRowProps {
  id: string;
  name: string;
  totalAssets: number | null;
  health: FederationHealth;
  expiryTimestamp?: number | null;
  /** Federation its guardians named to migrate to, with its name when the observer tracks it */
  successorId?: string | null;
  successorName?: string | null;
  activityData: FederationActivity[];
  maxTransaction?: number;  // global max for consistent chart scale
  maxVolume?: number;        // global max for consistent chart scale, in msats
}

// Health status messages
const HEALTH_MESSAGES = {
  degraded: 'Some guardians are offline or out of sync',
  offline: 'Too few guardians are answering to reach consensus',
} as const;

// Stable formatter references across row renders.
const formatTransaction = (value: number) => Math.round(value).toString();
const formatVolume = (msats: number) => asBitcoin(msats, 8);

export const FederationRow = memo(function FederationRow({
  id,
  name,
  totalAssets,
  health,
  expiryTimestamp,
  successorId,
  successorName,
  activityData,
  maxTransaction,
  maxVolume,
}: FederationRowProps) {
  // Extract data for mini charts
  const transactionData = useMemo(() => activityData.map(d => d.num_transactions), [activityData]);
  const volumeData = useMemo(() => activityData.map(d => d.amount_transferred), [activityData]);

  // The server counts UTC days, the last being the current one (most recent data
  // last, so we count backwards)
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

  return (
    <Link
      to={`/federations/${id}`}
      className="block bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 px-3 sm:px-6 py-4 text-xs sm:text-sm"
    >
      {/* Mobile: name and total assets, with the chart in a second row below. Desktop: 3 columns. */}
      <div className="grid grid-cols-[1.5fr_1fr] gap-3 items-start md:grid-cols-3">
        {/* Name */}
        <div className="font-medium text-gray-900 dark:text-white min-w-0">
          <span className="text-[10px] uppercase text-gray-600 dark:text-gray-300 block mb-1 md:hidden">Name</span>
          <div className="flex items-center gap-1.5 min-w-0">
            <span className="font-medium text-gray-900 dark:text-white wrap-break-word min-w-0">
              {name}
            </span>
            {showWarning && <Badge level={badgeLevel} tooltip={healthMessage} showIcon />}
          </div>
          {/* Always on its own line under the name, never broken, so every row and screen width shows it the same way */}
          {expiryTimestamp && successorId ? (
            <span className="mt-1.5 flex items-center gap-1 min-w-0">
              <span
                title={`${isShutDown ? 'Moved' : 'Migrating'} to ${successorName ?? 'a new federation'}; ${isShutDown ? 'shut down' : 'shuts down'} on ${formatUtcDateTime(expiryTimestamp * 1000)}`}
                className={`inline-flex min-w-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium leading-4 ring-1 ring-inset ${
                  isShutDown
                    ? 'bg-gray-100 text-gray-700 ring-gray-500/25 dark:bg-gray-500/10 dark:text-gray-300 dark:ring-gray-400/30'
                    : 'bg-blue-50 text-blue-800 ring-blue-600/20 dark:bg-blue-500/10 dark:text-blue-300 dark:ring-blue-500/30'
                }`}
              >
                <svg className="size-3 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                  <path d="M5 12h14M13 6l6 6-6 6" />
                </svg>
                <span className="truncate">
                  {isShutDown ? 'Moved' : 'Migrating'}{successorName ? ` to ${successorName}` : ''}
                </span>
              </span>
              {/* The row is itself a link, so the explainer opens from a button rather than a nested link */}
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
                  className="inline-flex size-5 shrink-0 items-center justify-center rounded-full border border-gray-300 text-[11px] font-semibold text-gray-600 hover:bg-gray-100 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-600"
                >
                  ?
                </button>
              )}
            </span>
          ) : expiryTimestamp ? (
            <span
              title={`${isShutDown ? 'Shut down' : 'Shuts down'} on ${formatUtcDateTime(expiryTimestamp * 1000)}`}
              className={`mt-1.5 inline-flex items-center gap-1.5 whitespace-nowrap rounded-full px-2 py-0.5 text-[11px] font-medium leading-4 ring-1 ring-inset ${
                isShutDown
                  ? 'bg-red-50 text-red-700 ring-red-600/20 dark:bg-red-500/10 dark:text-red-400 dark:ring-red-500/30'
                  : 'bg-amber-50 text-amber-800 ring-amber-600/20 dark:bg-amber-500/10 dark:text-amber-300 dark:ring-amber-500/30'
              }`}
            >
              <span aria-hidden="true" className={`size-1.5 shrink-0 rounded-full ${isShutDown ? 'bg-red-500' : 'bg-amber-500'}`} />
              {isShutDown ? 'Shut down' : `Shuts down ${formatUtcDate(expiryTimestamp * 1000)}`}
            </span>
          ) : null}
        </div>

        {/* Total Assets */}
        <div className="text-right md:text-left">
          <span className="text-[10px] uppercase text-gray-600 dark:text-gray-300 block mb-1 md:hidden">Total Assets</span>
          <span className="text-gray-900 dark:text-white whitespace-nowrap md:whitespace-normal">{totalAssets === null ? 'Awaiting balance' : asBitcoin(totalAssets, 6)}</span>
        </div>

        {/* Activity Charts (7d) */}
        <div className="col-span-2 md:col-span-1">
          <span className="text-[10px] uppercase text-gray-600 dark:text-gray-300 block mb-1 md:hidden">Activity (7d)</span>
          <CombinedMiniChart
            transactionData={transactionData}
            volumeData={volumeData}
            dates={dates}
            formatTransaction={formatTransaction}
            formatVolume={formatVolume}
            maxTransaction={maxTransaction}
            maxVolume={maxVolume}
          />
        </div>
      </div>
    </Link>
  );
});
