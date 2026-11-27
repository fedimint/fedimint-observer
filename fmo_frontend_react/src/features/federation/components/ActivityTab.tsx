import { Alert } from '../../../components/ui/Alert';
import { Button } from '../../../components/ui/Button';
import { Panel } from '../../../components/ui/Panel';
import type { FederationActivity } from '../hooks/useFederationActivity';
import { ActivityControls } from './ActivityControls';
import { TransactionChart } from './TransactionChart';

const chartSpace = 'flex h-[400px] items-center justify-center text-center text-xs text-ink-3 sm:text-sm';

function ActivityChart({ activity }: { activity: FederationActivity }) {
  const { histogram, loading, error, zoom, setZoom } = activity;
  const { chartMetric, movingAverageWindow, useLogScale } = activity.settings;

  if (loading && histogram.length === 0) {
    return <div role="status" className={`mt-4 ${chartSpace}`}>Loading chart data…</div>;
  }
  if (error) {
    return (
      <div className="py-12 text-center text-xs text-crit-ink sm:text-sm" role="alert">
        <p>Could not load the activity data: {error}</p>
        <Button onClick={activity.retry} className="mt-3">Try again</Button>
      </div>
    );
  }
  if (histogram.length === 0) {
    return <div className={chartSpace}>No activity data available</div>;
  }
  return (
    <div className={`mt-4 -mx-4 sm:mx-0 transition-opacity ${loading ? 'opacity-60' : ''}`} aria-busy={loading}>
      <h4 className="text-center text-sm sm:text-base font-normal text-ink-3 mb-4 px-4 sm:px-0">
        Daily {chartMetric === 'volume' ? 'Volume' : 'Transactions'}{useLogScale && chartMetric === 'count' ? ' (Log Scale)' : ''}
        <span className="block text-xs text-ink-3">Days in UTC</span>
      </h4>
      <div className="w-full px-4 sm:px-0">
          <TransactionChart
            data={histogram}
            chartMetric={chartMetric}
            movingAverageWindow={movingAverageWindow}
            useLogScale={useLogScale}
            zoomStart={zoom.start}
            zoomEnd={zoom.end}
            onZoomChange={(start, end) => setZoom({ start, end })}
          />
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
      <Alert level="info" message="Some transaction types, like Lightning transactions, cause more than one Fedimint transaction." />

      <Panel>
        <div className="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-4 mb-4 sm:mb-6">
          <div className="flex-1 min-w-0">
            <p className="meta-label">
              {error ? 'Unavailable: activity could not be loaded' : chartMetric === 'volume' ? 'Total volume' : 'Total transactions'}
            </p>
            <h3 className="tabular mt-1 text-xl font-medium text-ink wrap-break-word">
              {error || (loading && histogram.length === 0) ? '—' : chartMetric === 'volume'
                ? totals.volume.toFixed(6) + ' BTC'
                : totals.count.toLocaleString('en-US')}
            </h3>
          </div>
          <ActivityControls activity={activity} />
        </div>

        <ActivityChart activity={activity} />
      </Panel>
    </>
  );
}
