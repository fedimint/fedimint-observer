import { useMemo } from 'react';
import { Chart } from '../../../components/ui/Chart';
import { useTheme } from '../../../hooks/useTheme';
import { chartTheme } from '../../reliability/lib/chartTheme';
import type { HistogramEntry } from '../../../utils/histogram';



interface TransactionChartProps {
  data: HistogramEntry[];
  chartMetric: 'volume' | 'count';
  movingAverageWindow: number;
  useLogScale: boolean;
  zoomStart: number;
  zoomEnd: number;
  onZoomChange: (start: number, end: number) => void;
}

export function TransactionChart({
  data,
  chartMetric,
  movingAverageWindow,
  useLogScale,
  zoomStart,
  zoomEnd,
  onZoomChange,
}: TransactionChartProps) {
  const { isDark } = useTheme();
  const colors = useMemo(() => {
    const theme = chartTheme(isDark);
    return { label: theme.ink3, axis: theme.axis, grid: theme.grid, tooltip: theme.surface, tooltipBorder: theme.axis, tooltipText: theme.ink2, ok: theme.ok };
  }, [isDark]);

  const baseChartOption = useMemo(() => {
    const dates = data.map(d => d.date);
    const values = data.map(d => chartMetric === 'volume' ? d.volume : d.count);
    const avgValues = movingAverageWindow > 0
      ? data.map(d => chartMetric === 'volume' ? d.avgVolume : d.avgCount)
      : [];

    return {
      grid: {
        left: '3%',
        right: '4%',
        bottom: '15%',
        top: '10%',
        outerBoundsMode: 'same',
        outerBoundsContain: 'axisLabel'
      },
      xAxis: {
        type: 'category',
        data: dates,
        axisLabel: {
          rotate: 45,
          fontSize: 10,
          color: colors.label
        },
        axisLine: {
          lineStyle: { color: colors.axis }
        }
      },
      yAxis: {
        type: useLogScale && chartMetric === 'count' ? 'log' : 'value',
        axisLabel: {
          fontSize: 10,
          color: colors.label,
          formatter: (value: number) => {
            if (chartMetric === 'volume') {
              return value < 0.001 ? value.toExponential(1) : value.toFixed(3);
            }
            return value < 1 ? value.toFixed(1) : Math.round(value).toString();
          }
        },
        axisLine: {
          lineStyle: { color: colors.axis }
        },
        splitLine: {
          lineStyle: { color: colors.grid, type: 'dashed' }
        }
      },
      tooltip: {
        trigger: 'axis',
        backgroundColor: colors.tooltip,
        borderColor: colors.tooltipBorder,
        textStyle: { color: colors.tooltipText, fontSize: 12 },
        formatter: (params: { axisValue: string; marker: string; seriesName: string; value: number }[]) => {
          const date = params[0].axisValue;
          let result = `${date}<br/>`;
          params.forEach((param) => {
            const value = param.value;
            if (param.seriesName.includes('Average')) {
              result += `${param.marker} ${param.seriesName}: ${
                chartMetric === 'volume'
                  ? value?.toFixed(8) + ' BTC'
                  : value?.toFixed(1) + ' transactions'
              }<br/>`;
            } else {
              result += `${param.marker} ${param.seriesName}: ${
                chartMetric === 'volume'
                  ? value.toFixed(8) + ' BTC'
                  : Math.round(value) + ' transactions'
              }<br/>`;
            }
          });
          return result;
        }
      },
      series: [
        {
          name: chartMetric === 'volume' ? 'Volume' : 'Transactions',
          type: 'line',
          data: values,
          smooth: true,
          symbol: 'none',
          lineStyle: { color: '#3b82f6', width: 2 },
          areaStyle: {
            color: {
              type: 'linear',
              x: 0,
              y: 0,
              x2: 0,
              y2: 1,
              colorStops: [
                { offset: 0, color: 'rgba(59, 130, 246, 0.8)' },
                { offset: 1, color: 'rgba(59, 130, 246, 0.1)' }
              ]
            }
          }
        },
        ...(movingAverageWindow > 0 && avgValues.length > 0
          ? [
              {
                name: `${movingAverageWindow}-day average`,
                type: 'line',
                data: avgValues,
                smooth: true,
                symbol: 'none',
                lineStyle: {
                  color: colors.ok,
                  width: 3,
                  type: 'dashed'
                }
              }
            ]
          : [])
      ]
    };
  }, [data, chartMetric, movingAverageWindow, useLogScale, colors]);

  const chartOption = useMemo(() => ({
    ...baseChartOption,
    dataZoom: [
      {
        type: 'slider',
        start: zoomStart,
        end: zoomEnd,
        height: 25,
        bottom: 10,
        borderColor: '#3b82f6',
        fillerColor: 'rgba(59, 130, 246, 0.2)',
        handleStyle: {
          color: '#3b82f6'
        },
        moveHandleSize: 10,
        textStyle: { color: colors.label, fontSize: 10 }
      }
    ]
  }), [baseChartOption, zoomStart, zoomEnd, colors]);

  return (
    <Chart
      option={chartOption}
      height={400}
      features={['dataZoom']}
      notMerge={false}
      onEvents={{
        dataZoom: (params: { batch?: { start?: number; end?: number }[]; start?: number; end?: number }) => {
          const range = params.batch?.[0] ?? params;
          if (range.start !== undefined && range.end !== undefined) onZoomChange(range.start, range.end);
        },
      }}
    />
  );
}
