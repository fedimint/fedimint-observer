import type { FederationHistogram } from '../types/api';
import { MSATS_PER_BTC } from './format.ts';
import { formatUtcDate } from './time.ts';

// One UTC day of a federation's activity, with volume in BTC for the axis
export interface HistogramEntry {
  /** The UTC day as sent, e.g. "May 31, 2024" for "2024-05-31" */
  date: string;
  /** UTC midnight of that day */
  timestamp: number;
  volume: number;
  count: number;
  avgVolume?: number;
  avgCount?: number;
}

/**
 * The server's days as chart points. The server fills in quiet days, computes the
 * averages and flags outliers; with `excludeOutliers` the flagged days are left out
 * of the chart, so the axis skips them.
 */
export function toHistogramEntries(histogram: FederationHistogram, excludeOutliers: boolean): HistogramEntry[] {
  return histogram.days
    .filter((day) => !(excludeOutliers && day.outlier))
    .map((day) => ({
      date: formatUtcDate(day.date),
      timestamp: Date.parse(day.date),
      volume: day.volume_msat / MSATS_PER_BTC,
      count: day.count,
      avgVolume: day.avg_volume_msat === null ? undefined : day.avg_volume_msat / MSATS_PER_BTC,
      avgCount: day.avg_count ?? undefined,
    }));
}
