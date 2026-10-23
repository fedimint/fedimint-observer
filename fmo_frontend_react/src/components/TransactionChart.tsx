import { useMemo } from 'react';
import ReactEChartsCore from 'echarts-for-react/lib/core';
import * as echarts from 'echarts/core';
import { LineChart } from 'echarts/charts';
import {
  GridComponent,
  TooltipComponent,
  DataZoomComponent,
} from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import { useTheme } from '../hooks/useTheme';

// Register only the components we need
echarts.use([
  LineChart,
  GridComponent,
  TooltipComponent,
  DataZoomComponent,
  CanvasRenderer,
]);

// One day of a federation's activity, with volume in BTC for the axis
export interface HistogramEntry {
  date: string;
  timestamp: number;
  volume: number;
  count: number;
  avgVolume?: number;
  avgCount?: number;
}

// Dark is what the chart always used; light matches the app's other charts
const CHART_COLORS = {
  dark: { label: '#9ca3af', axis: '#374151', grid: '#374151', tooltip: '#1f2937', tooltipBorder: '#374151', tooltipText: '#fff' },
  light: { label: '#6b7280', axis: '#d1d5db', grid: '#e5e7eb', tooltip: '#ffffff', tooltipBorder: '#e5e7eb', tooltipText: '#111827' },
};

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
  const colors = isDark ? CHART_COLORS.dark : CHART_COLORS.light;

  // Separate the base chart option from zoom state
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
        // ECharts 6 form of `containLabel: true`, which it lays out the same
        // way but warns about unless the legacy layout is registered
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
                name: `${movingAverageWindow}-Day Average`,
                type: 'line',
                data: avgValues,
                smooth: true,
                symbol: 'none',
                lineStyle: {
                  color: '#059669',
                  width: 3,
                  type: 'dashed'
                }
              }
            ]
          : [])
      ]
    };
  }, [data, chartMetric, movingAverageWindow, useLogScale, colors]);

  // Create the final chart option with zoom state
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
    <ReactEChartsCore
      echarts={echarts}
      option={chartOption}
      notMerge={false}
      lazyUpdate={true}
      style={{ height: '400px', width: '100%' }}
      opts={{ renderer: 'canvas' }}
      onEvents={{
        dataZoom: (params: { batch?: { start: number; end: number }[]; start?: number; end?: number }) => {
          // Handle both batch and direct dataZoom events
          if (params.batch && params.batch[0]) {
            const start = params.batch[0].start;
            const end = params.batch[0].end;
            if (start !== undefined && end !== undefined) {
              onZoomChange(start, end);
            }
          } else if (params.start !== undefined && params.end !== undefined) {
            onZoomChange(params.start, params.end);
          }
        }
      }}
    />
  );
}
