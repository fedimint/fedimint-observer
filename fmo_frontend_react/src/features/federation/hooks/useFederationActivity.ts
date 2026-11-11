import { useEffect, useMemo, useRef, useState } from 'react';
import { api } from '../../../services/api';
import type { HistogramAverage } from '../../../types/api';
import { MSATS_PER_BTC } from '../../../utils/format';
import { toHistogramEntries, type HistogramEntry } from '../../../utils/histogram';
import { errorMessage } from '../../../utils/errors';
import { useApiResource } from '../../../hooks/useApiResource';

export type ChartMetric = 'volume' | 'count';

const THREE_MONTHS_MS = 90 * 24 * 60 * 60 * 1000;
const NO_ENTRIES: HistogramEntry[] = [];

/** The zoom that shows the last three months of `histogram`, in percent of its length */
function lastThreeMonths(histogram: HistogramEntry[]): { start: number; end: number } {
  if (histogram.length === 0) return { start: 0, end: 100 };
  const cutoff = histogram[histogram.length - 1].timestamp - THREE_MONTHS_MS;
  // With nothing in the last 90 days, show all
  const startIndex = Math.max(0, histogram.findIndex((entry) => entry.timestamp >= cutoff));
  return { start: (startIndex / histogram.length) * 100, end: 100 };
}

/**
 * The federation's daily activity and the chart settings it is shown with. It is
 * kept by the page rather than the Activity tab, so switching tabs keeps both.
 */
export function useFederationActivity(id: string) {
  const [chartMetric, setChartMetric] = useState<ChartMetric>('volume');
  const [filterOutliers, setFilterOutliers] = useState(true);
  const [movingAverageWindow, setMovingAverageWindow] = useState<HistogramAverage>(0);
  const [useLogScale, setUseLogScale] = useState(false);
  // Outliers only distort the volume chart, so the transaction chart keeps them
  const excludeOutliers = chartMetric === 'volume' && filterOutliers;

  const resource = useApiResource(async (signal) => {
    const data = await api.getFederationHistogram(id, { average: movingAverageWindow, excludeOutliers }, signal);
    return {
      id,
      entries: toHistogramEntries(data, excludeOutliers),
      totals: { volume: data.total_volume_msat / MSATS_PER_BTC, count: data.total_count },
    };
  }, [id, movingAverageWindow, excludeOutliers], { label: 'histogram' });
  // An answer for another federation never shows under this one
  const loaded = resource.data?.id === id ? resource.data : null;
  const histogram = loaded?.entries ?? NO_ENTRIES;

  // Zoom to the last three months when a federation's activity first loads; later
  // reloads for another average or outlier setting keep the viewer's zoom
  const initialZoom = useMemo(() => lastThreeMonths(histogram), [histogram]);
  const [zoom, setZoom] = useState(initialZoom);
  const zoomedFederation = useRef<string | null>(null);
  useEffect(() => {
    if (histogram.length > 0 && zoomedFederation.current !== id) {
      zoomedFederation.current = id;
      setZoom(initialZoom);
    }
  }, [histogram.length, initialZoom, id]);

  return {
    histogram,
    totals: loaded?.totals ?? { volume: 0, count: 0 },
    loading: resource.loading,
    error: resource.failed ? errorMessage(resource.error) : null,
    retry: resource.reload,
    zoom,
    setZoom,
    settings: { chartMetric, filterOutliers, movingAverageWindow, useLogScale },
    setChartMetric,
    setFilterOutliers,
    setMovingAverageWindow,
    setUseLogScale,
  };
}

export type FederationActivity = ReturnType<typeof useFederationActivity>;
