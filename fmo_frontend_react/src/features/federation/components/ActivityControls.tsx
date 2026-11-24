import { Select } from '../../../components/ui/Select';
import type { HistogramAverage } from '../../../types/api';
import type { ChartMetric, FederationActivity } from '../hooks/useFederationActivity';

const checkboxLabel = 'inline-flex h-8 items-center text-xs text-ink-2 cursor-pointer whitespace-nowrap';
const checkbox = 'mr-2 size-4 shrink-0 rounded-sm border-line-strong bg-surface text-accent focus:ring-2 focus:ring-accent/25';

interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  title: string;
  label: string;

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
    <div className="relative z-10 flex shrink-0 flex-wrap items-center gap-2 sm:gap-3">
      <Select className="min-w-36"
          value={movingAverageWindow}
          onChange={(e) => activity.setMovingAverageWindow(Number(e.target.value) as HistogramAverage)}
          aria-label="Moving average"
        >
          <option value="0">No average</option>
          <option value="7">7-Day Avg</option>
          <option value="30">30-Day Avg</option>
        </Select>
      {chartMetric === 'count' && (
        <Checkbox
          checked={useLogScale}
          onChange={activity.setUseLogScale}
          title="Use logarithmic scale (base 10) for Y-axis. Zeros are replaced with a small value."
          label="Log scale"
          shortLabel="Log"
        />
      )}
      {chartMetric === 'volume' && (
        <Checkbox
          checked={filterOutliers}
          onChange={activity.setFilterOutliers}
          title="Filter out values that are more than 10 times the 95th percentile"
          label="Filter extreme outliers"
          shortLabel="Filter Outliers"
        />
      )}
      <Select className="min-w-36"
          value={chartMetric}
          onChange={(e) => activity.setChartMetric(e.target.value as ChartMetric)}
          aria-label="Chart metric"
        >
          <option value="volume">Volume</option>
          <option value="count">Transactions</option>
        </Select>
    </div>
  );
}
