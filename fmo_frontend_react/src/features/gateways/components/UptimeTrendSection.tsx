import { lazy, Suspense, useMemo } from 'react';
import type { EChartsOption } from 'echarts';
import type { GatewayUptimeTrendPoint, GatewayWindow } from '../../../types/api';
import { useTheme } from '../../../hooks/useTheme';
import { chartTheme } from '../../reliability/lib/chartTheme';
import { formatUtcDate } from '../../../utils/time';
import { ErrorBoundary, ReloadMessage } from '../../../components/ui/ErrorBoundary';

const UptimeTrendChart = lazy(() => import('./UptimeTrendChart').then((module) => ({ default: module.UptimeTrendChart })));


interface UptimeTrendSectionProps {
  timeWindow: GatewayWindow;
  points: GatewayUptimeTrendPoint[];
  /** Shown in place of the chart when there are no points */
  placeholder: string;
}

/** Daily availability across all of the federation's gateways */
export function UptimeTrendSection({ timeWindow, points, placeholder }: UptimeTrendSectionProps) {
  const { isDark } = useTheme();
  const colors = useMemo(() => {
    const theme = chartTheme(isDark);
    return { label: theme.ink3, axis: theme.axis, grid: theme.grid, line: theme.series, area: theme.seriesSoft };
  }, [isDark]);

  // Each point starts at UTC midnight and covers that UTC day
  const labels = useMemo(
    () => points.map((point) => formatUtcDate(point.day, { month: 'short', day: 'numeric' })),
    [points],
  );
  const option = useMemo<EChartsOption>(() => ({
    animationDuration: 200,
    grid: { top: 18, right: 18, bottom: 30, left: 42 },
    tooltip: { trigger: 'axis', valueFormatter: (value) => `${Number(value).toFixed(1)}%` },
    xAxis: {
      type: 'category', boundaryGap: false,
      data: labels,
      axisLabel: { color: colors.label, fontSize: 11 }, axisLine: { lineStyle: { color: colors.axis } },
    },
    yAxis: { type: 'value', min: 0, max: 100, axisLabel: { formatter: '{value}%', color: colors.label, fontSize: 11 }, splitLine: { lineStyle: { color: colors.grid } } },
    series: [{ name: 'Gateway availability', type: 'line', smooth: true, symbol: points.length > 45 ? 'none' : 'circle', symbolSize: 5, data: points.map((point) => Number(point.uptime_pct.toFixed(2))), lineStyle: { color: colors.line, width: 2.5 }, itemStyle: { color: colors.line }, areaStyle: { color: colors.area } }],
  }), [colors, labels, points]);

  return (
    <section className="mb-4 rounded-xl border border-line bg-surface px-4 pt-4 shadow-xs sm:px-5" aria-labelledby="uptime-trend-heading">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <div>
          <h2 id="uptime-trend-heading" className="text-base font-semibold text-ink">Gateway availability trend ({timeWindow.toUpperCase()})</h2>
          <p className="mt-0.5 text-xs text-ink-3">Each point combines every gateway poll snapshot recorded on that day · days in UTC.</p>
        </div>
        {points.length > 0 && <span className="text-xs text-ink-3">{points.length} day{points.length === 1 ? '' : 's'} with data</span>}
      </div>
      {points.length > 0 ? (
        <ErrorBoundary fallback={<div className="flex h-[210px] items-center justify-center"><ReloadMessage message="The availability chart could not be loaded." /></div>}>
          <Suspense fallback={<div className="flex h-[210px] items-center justify-center text-sm text-ink-3">Loading chart…</div>}>
            <UptimeTrendChart option={option} />
          </Suspense>
        </ErrorBoundary>
      ) : (
        <div className="flex h-[210px] items-center justify-center text-sm text-ink-3">{placeholder}</div>
      )}
    </section>
  );
}
