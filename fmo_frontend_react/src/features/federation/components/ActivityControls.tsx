import type { HistogramAverage } from '../../../types/api';
import type { ChartMetric, FederationActivity } from '../hooks/useFederationActivity';

const select = 'px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white text-xs sm:text-sm min-w-[140px] appearance-none cursor-pointer';
const checkboxLabel = 'flex items-center text-xs sm:text-sm text-gray-600 dark:text-gray-400 cursor-pointer whitespace-nowrap';
const checkbox = 'mr-2 w-4 h-4 text-blue-600 bg-gray-100 border-gray-300 rounded-sm focus:ring-blue-500 shrink-0';

interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  title: string;
  label: string;
  /** Shorter label for phones */
  shortLabel: string;
}

function Checkbox({ checked, onChange, title, label, shortLabel }: CheckboxProps) {
  return (
    <label className={checkboxLabel} title={title}>
      <input type="checkbox" className={checkbox} checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="hidden sm:inline">{label}</span>
      <span className="sm:hidden">{shortLabel}</span>
    </label>
  );
}

/** Moving average, metric, and the metric's scale or outlier option */
export function ActivityControls({ activity }: { activity: FederationActivity }) {
  const { chartMetric, filterOutliers, movingAverageWindow, useLogScale } = activity.settings;
  return (
    <div className="flex flex-col sm:flex-row items-start sm:items-center gap-2 sm:gap-4 shrink-0 relative z-10">
      <div className="relative">
        <select
          className={select}
          value={movingAverageWindow}
          onChange={(e) => activity.setMovingAverageWindow(Number(e.target.value) as HistogramAverage)}
          aria-label="Moving average"
        >
          <option value="0">No Average</option>
          <option value="7">7-Day Avg</option>
          <option value="30">30-Day Avg</option>
        </select>
      </div>
      {chartMetric === 'count' && (
        <Checkbox
          checked={useLogScale}
          onChange={activity.setUseLogScale}
          title="Use logarithmic scale (base 10) for Y-axis. Zeros are replaced with a small value."
          label="Log Scale"
          shortLabel="Log"
        />
      )}
      {chartMetric === 'volume' && (
        <Checkbox
          checked={filterOutliers}
          onChange={activity.setFilterOutliers}
          title="Filter out values that are more than 10 times the 95th percentile"
          label="Filter Extreme Outliers"
          shortLabel="Filter Outliers"
        />
      )}
      <div className="relative">
        <select
          className={select}
          value={chartMetric}
          onChange={(e) => activity.setChartMetric(e.target.value as ChartMetric)}
          aria-label="Chart metric"
        >
          <option value="volume">Volume</option>
          <option value="count">Transactions</option>
        </select>
      </div>
    </div>
  );
}
