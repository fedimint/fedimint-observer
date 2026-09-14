
import React, { Suspense } from 'react';
import { Link } from 'react-router-dom';
import type { FederationHealth } from '../types/api';
import { Badge } from './Badge';
const CombinedMiniChart = React.lazy(() => import('./MiniChart').then((m) => ({ default: m.CombinedMiniChart })));
import { asBitcoin } from '../utils/format';

interface ActivityData {
  num_transactions: number;
  amount_transferred: number;
}

interface FederationRowProps {
  id: string;
  name: string;
  totalAssets: number;
  health: FederationHealth;
  activityData: ActivityData[];
  maxTransaction?: number;  // global max for consistent chart scale
  maxVolume?: number;        // global max for consistent chart scale
}

export function FederationRow({
  id,
  name,
  totalAssets,
  health,
  activityData,
  maxTransaction,
  maxVolume,
}: FederationRowProps) {
  // Extract data for mini charts
  const transactionData = activityData.map(d => d.num_transactions);
  const volumeData = activityData.map(d => d.amount_transferred / 100000000000); // millisats to BTC
  
  // Generate date labels (most recent data last, so we count backwards)
  const dates = activityData.map((_, index) => {
    const daysAgo = activityData.length - 1 - index;
    if (daysAgo === 0) return 'Today';
    if (daysAgo === 1) return 'Yesterday';
    return `${daysAgo} days ago`;
  });

  // Health status messages
  const HEALTH_MESSAGES = {
    degraded: 'Some guardians are offline or out of sync',
    offline: 'All guardians are offline',
  } as const;

  const showWarning = health === 'degraded' || health === 'offline';
  const badgeLevel = health === 'degraded' ? 'warning' : 'error';
  const healthMessage = showWarning ? HEALTH_MESSAGES[health] : '';

  return (
    <Link 
      to={`/federations/${id}`}
      className="block bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 px-3 sm:px-6 py-4 text-xs sm:text-sm"
    >
      {/* Mobile Layout (3 rows) */}
      <div className="md:hidden space-y-3">
        {/* Row 1: Name, Total Assets */}
        <div className="grid grid-cols-[1.5fr,1fr] gap-3 items-start">
          {/* Name */}
          <div className="font-medium text-gray-900 dark:text-white min-w-0">
            <span className="text-[10px] uppercase text-gray-600 dark:text-gray-400 block mb-1">Name</span>
            <div className="flex items-center gap-1.5">
              <span className="font-medium text-gray-900 dark:text-white break-words">
                {name}
              </span>
              {showWarning && <Badge level={badgeLevel} tooltip={healthMessage} showIcon>{''}</Badge>}
            </div>
          </div>


          {/* Total Assets */}
          <div className="text-right flex-shrink-0">
            <span className="text-[10px] uppercase text-gray-600 dark:text-gray-400 block mb-1">Total Assets</span>
            <span className="text-gray-900 dark:text-white whitespace-nowrap">{asBitcoin(totalAssets, 6)}</span>
          </div>
        </div>

        {/* Row 2: Activity Chart */}
        <div>
          <span className="text-[10px] uppercase text-gray-600 dark:text-gray-400 block mb-1">Activity (7d)</span>
          <Suspense fallback={<div className="w-full h-12 bg-transparent" /> }>
            <CombinedMiniChart
              transactionData={transactionData}
              volumeData={volumeData}
              dates={dates}
              formatTransaction={(val) => Math.round(val).toString()}
              formatVolume={(val) => `${val.toFixed(8)} BTC`}
              maxTransaction={maxTransaction}
              maxVolume={maxVolume}
            />
          </Suspense>
        </div>
      </div>

      {/* Desktop Layout (3 columns) */}
      <div className="hidden md:grid md:grid-cols-3 md:gap-3">
        {/* Name */}
        <div className="font-medium text-gray-900 dark:text-white">
          <div className="flex items-center gap-1.5">
            <span className="font-medium text-gray-900 dark:text-white break-words">
              {name}
            </span>
            {showWarning && <Badge level={badgeLevel} tooltip={healthMessage} showIcon>{''}</Badge>}
          </div>
        </div>


        {/* Total Assets */}
        <div>
          <span className="text-gray-900 dark:text-white">{asBitcoin(totalAssets, 6)}</span>
        </div>

        {/* Activity Charts (7d) */}
        <div>
          <Suspense fallback={<div className="w-full h-12 bg-transparent" /> }>
            <CombinedMiniChart
              transactionData={transactionData}
              volumeData={volumeData}
              dates={dates}
              formatTransaction={(val) => Math.round(val).toString()}
              formatVolume={(val) => `${val.toFixed(8)} BTC`}
              maxTransaction={maxTransaction}
              maxVolume={maxVolume}
            />
          </Suspense>
        </div>
      </div>
    </Link>
  );
}
