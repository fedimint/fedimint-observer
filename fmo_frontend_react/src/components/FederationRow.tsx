import { memo, useMemo } from 'react';
import { Link } from 'react-router-dom';
import type { FederationActivity, FederationHealth } from '../types/api';
import { Badge } from './Badge';
import { asBitcoin } from '../utils/format';
import { formatUtcDate } from '../utils/time';
import { CombinedMiniChart } from './MiniChart';

interface FederationRowProps {
  id: string;
  name: string;
  totalAssets: number | null;
  health: FederationHealth;
  expiryTimestamp?: number | null;
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
  const expiryDate = expiryTimestamp ? `${formatUtcDate(expiryTimestamp * 1000)} UTC` : null;

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
          <div className="flex items-center gap-1.5">
            <span className="font-medium text-gray-900 dark:text-white wrap-break-word">
              {name}
            </span>
            {showWarning && <Badge level={badgeLevel} tooltip={healthMessage} showIcon />}
            {expiryDate && (
              <Badge level={expiryTimestamp! * 1000 <= Date.now() ? 'error' : 'warning'}>
                {expiryTimestamp! * 1000 <= Date.now() ? 'Shut down' : `Shutting down ${expiryDate}`}
              </Badge>
            )}
          </div>
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
