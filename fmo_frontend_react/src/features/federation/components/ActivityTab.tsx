import { lazy, Suspense } from 'react';
import { Panel } from '../../../components/ui/Panel';
import type { FederationActivity } from '../hooks/useFederationActivity';
import { ActivityControls } from './ActivityControls';

// echarts is large, so the chart loads only when shown
const TransactionChart = lazy(() => import('./TransactionChart').then((module) => ({ default: module.TransactionChart })));

const message = 'text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 py-12';

function ActivityChart({ activity }: { activity: FederationActivity }) {
  const { histogram, loading, error, zoom, setZoom } = activity;
  const { chartMetric, movingAverageWindow, useLogScale } = activity.settings;

  if (loading && histogram.length === 0) {
    return <div className={message}>Loading chart data...</div>;
  }
  if (error) {
    return (
      <div className="py-12 text-center text-xs text-red-700 dark:text-red-400 sm:text-sm" role="alert">
        <p>Could not load the activity data: {error}</p>
        <button
          type="button"
          onClick={activity.retry}
          className="mt-3 rounded-lg border border-gray-300 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-700"
        >
          Try again
        </button>
      </div>
    );
  }
  if (histogram.length === 0) {
    return <div className={message}>No activity data available</div>;
  }
  return (
    <div className={`mt-4 -mx-4 sm:mx-0 transition-opacity ${loading ? 'opacity-60' : ''}`} aria-busy={loading}>
      <h4 className="text-center text-sm sm:text-base font-normal text-gray-500 dark:text-gray-400 mb-4 px-4 sm:px-0">
        Daily {chartMetric === 'volume' ? 'Volume' : 'Transactions'}{useLogScale && chartMetric === 'count' ? ' (Log Scale)' : ''}
        <span className="block text-xs text-gray-400 dark:text-gray-500">Days in UTC</span>
      </h4>
      <div className="w-full px-4 sm:px-0">
        <Suspense fallback={
          <div className="flex items-center justify-center h-[400px] text-gray-500 dark:text-gray-400">
            Loading chart...
          </div>
        }>
          <TransactionChart
            data={histogram}
            chartMetric={chartMetric}
            movingAverageWindow={movingAverageWindow}
            useLogScale={useLogScale}
            zoomStart={zoom.start}
            zoomEnd={zoom.end}
            onZoomChange={(start, end) => setZoom({ start, end })}
          />
        </Suspense>
      </div>
    </div>
  );
}

/** The federation's total and daily transaction volume or count */
export function ActivityTab({ activity }: { activity: FederationActivity }) {
  const { histogram, loading, error, totals } = activity;
  const { chartMetric } = activity.settings;

  return (
    <>
      <div className="bg-blue-50 border border-blue-200 dark:bg-blue-900/20 dark:border-blue-800 rounded-lg p-3 sm:p-4 mb-4 sm:mb-6">
        <span className="text-xs sm:text-sm text-blue-800 dark:text-blue-300 font-semibold">Info:</span>
        <span className="text-xs sm:text-sm text-blue-800 dark:text-blue-300 ml-2">
          Some transaction types, like Lightning transactions, cause more than one Fedimint transaction.
        </span>
      </div>

      <Panel>
        <div className="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-4 mb-4 sm:mb-6">
          <div className="flex-1 min-w-0">
            <h3 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white wrap-break-word">
              {error || (loading && histogram.length === 0) ? '—' : chartMetric === 'volume'
                ? totals.volume.toFixed(6) + ' BTC'
                : totals.count.toString()}
            </h3>
            <p className="text-xs sm:text-sm text-gray-500 dark:text-gray-400">
              {error ? 'Unavailable: activity could not be loaded' : chartMetric === 'volume' ? 'Total Volume' : 'Total Transactions'}
            </p>
          </div>
          <ActivityControls activity={activity} />
        </div>

        <ActivityChart activity={activity} />
      </Panel>
    </>
  );
}
