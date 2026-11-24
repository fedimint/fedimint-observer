import type { FederationHistogram } from '../types/api';
import { MSATS_PER_BTC } from './format.ts';
import { formatUtcDate } from './time.ts';

export interface HistogramEntry {

  date: string;

  timestamp: number;
  volume: number;
  count: number;
  avgVolume?: number;
  avgCount?: number;
}


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
