import { useMemo } from 'react';
import type { EChartsOption } from 'echarts';
import type { GuardianHealthHistory } from '../../../types/api';
import { useTheme } from '../../../hooks/useTheme';
import { EmptyState } from '../../../components/ui/States';
import { formatUtcDate, formatUtcDateTime, formatUtcDayHour } from '../../../utils/time';
import {
  guardianSlots,
  reachabilitySeries,
  type GuardianRow,
  type Resilience,
} from '../../guardians/lib/guardianModel';
import { formatMs } from '../../guardians/lib/format';
import { axisStyle, chartTheme, tooltipStyle } from '../lib/chartTheme';
import { EChart } from './EChart';

const timeLabel = (history: GuardianHealthHistory, time: number) =>
  history.window === '30d' ? formatUtcDate(time, { month: 'short', day: 'numeric' }) : formatUtcDayHour(time);

/** Guardians answering per bucket, against the consensus threshold */
export function ReachabilityChart({ history, rows, res }: { history: GuardianHealthHistory; rows: readonly GuardianRow[]; res: Resilience }) {
  const { isDark } = useTheme();
  const theme = chartTheme(isDark);
  const series = useMemo(() => reachabilitySeries(history, rows.map((row) => row.id)), [history, rows]);
  const polled = series.filter((point) => point.answering !== null);
  const belowThreshold = polled.filter((point) => (point.answering ?? 0) < res.threshold - 0.001).length;

  const option: EChartsOption = {
    grid: { left: 36, right: 64, top: 16, bottom: 28 },
    tooltip: {
      trigger: 'axis',
      ...tooltipStyle(theme),
      formatter: (params) => {
        const point = series[(params as { dataIndex: number }[])[0]?.dataIndex ?? 0];
        if (!point) return '';
        const range = `${formatUtcDateTime(point.start)}`;
        return point.answering === null
          ? `${range}<br/>Not polled`
          : `${range}<br/><b>${point.answering.toFixed(2)}</b> of ${res.total} guardians answering on average<br/>Threshold ${res.threshold}`;
      },
    },
    xAxis: {
      type: 'category',
      data: series.map((point) => timeLabel(history, point.start)),
      boundaryGap: false,
      ...axisStyle(theme),
      splitLine: { show: false },
      axisLabel: { color: theme.ink3, fontSize: 10, hideOverlap: true },
    },
    yAxis: { type: 'value', min: 0, max: res.total, interval: res.total <= 7 ? 1 : undefined, ...axisStyle(theme) },
    series: [
      {
        type: 'line',
        name: 'Guardians answering',
        data: series.map((point) => (point.answering === null ? null : Number(point.answering.toFixed(3)))),
        step: 'middle',
        showSymbol: false,
        connectNulls: false,
        lineStyle: { width: 2, color: theme.series },
        itemStyle: { color: theme.series },
        markLine: {
          silent: true,
          symbol: 'none',
          lineStyle: { color: theme.reference, type: 'dashed', width: 1 },
          label: { formatter: `Threshold ${res.threshold}`, color: theme.ink2, fontSize: 10, position: 'end' },
          data: [{ yAxis: res.threshold }],
        },
      },
    ],
  };

  if (polled.length === 0) {
    return <EmptyState icon="clock" title="No polls in this window" />;
  }
  return (
    <>
      <EChart
        option={option}
        height={220}
        label={`Guardians answering the observer over the ${history.window} window, against the consensus threshold of ${res.threshold}. ${belowThreshold} of ${polled.length} periods fell below it.`}
      />
      <p className="mt-1 text-xs text-ink-3">
        {belowThreshold === 0
          ? `The average stayed at or above the threshold of ${res.threshold} throughout the window.`
          : `${belowThreshold} of ${polled.length} periods averaged below the threshold of ${res.threshold}.`}
      </p>
    </>
  );
}

/** Five-number summary of each guardian's per-bucket mean latency */
export function LatencyTrendChart({ history, row }: { history: GuardianHealthHistory; row: GuardianRow }) {
  const { isDark } = useTheme();
  const theme = chartTheme(isDark);
  const slots = useMemo(() => guardianSlots(history, row.id), [history, row.id]);
  const timed = slots.filter((slot) => slot.latencyMs !== null);
  if (timed.length === 0) return <EmptyState icon="signal" title="No timed answers from this guardian in this window" />;

  const option: EChartsOption = {
    grid: { left: 52, right: 16, top: 12, bottom: 28 },
    tooltip: {
      trigger: 'axis',
      ...tooltipStyle(theme),
      formatter: (params) => {
        const slot = slots[(params as { dataIndex: number }[])[0]?.dataIndex ?? 0];
        return slot ? `${formatUtcDateTime(slot.start)}<br/>${slot.latencyMs === null ? 'No timed answer' : `Mean latency <b>${formatMs(slot.latencyMs)}</b>`}` : '';
      },
    },
    xAxis: {
      type: 'category',
      data: slots.map((slot) => timeLabel(history, slot.start)),
      boundaryGap: false,
      ...axisStyle(theme),
      splitLine: { show: false },
      axisLabel: { color: theme.ink3, fontSize: 10, hideOverlap: true },
    },
    yAxis: { type: 'value', name: 'ms', nameTextStyle: { color: theme.ink3, fontSize: 10 }, min: 0, ...axisStyle(theme) },
    series: [
      {
        type: 'line',
        name: `Guardian ${row.id}`,
        data: slots.map((slot) => (slot.latencyMs === null ? null : Math.round(slot.latencyMs))),
        connectNulls: false,
        showSymbol: false,
        lineStyle: { width: 2, color: theme.series },
        itemStyle: { color: theme.series },
      },
    ],
  };
  const values = timed.map((slot) => slot.latencyMs!);
  return (
    <EChart
      option={option}
      height={200}
      label={`Mean API latency of guardian ${row.id} over the ${history.window} window, from ${formatMs(Math.min(...values))} to ${formatMs(Math.max(...values))}.`}
    />
  );
}

/** Latest session per answering guardian and how far each trails the furthest one */
