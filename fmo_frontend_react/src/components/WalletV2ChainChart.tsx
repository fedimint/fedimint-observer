import { useMemo } from 'react';
import ReactEChartsCore from 'echarts-for-react/lib/core';
import * as echarts from 'echarts/core';
import { LineChart } from 'echarts/charts';
import { GridComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import { useTheme } from '../hooks/useTheme';
import type { WalletV2OnChainTransaction } from '../types/api';
import { asBitcoin } from '../utils/format';

echarts.use([LineChart, GridComponent, TooltipComponent, CanvasRenderer]);

interface WalletV2ChainChartProps {
  transactions: WalletV2OnChainTransaction[];
}

export function WalletV2ChainChart({ transactions }: WalletV2ChainChartProps) {
  const { isDark } = useTheme();
  const option = useMemo(() => {
    const ordered = [...transactions].sort((a, b) => a.index - b.index);
    const labels = ordered.map((transaction) => `#${transaction.index}`);
    const values = ordered.map((transaction) => ({
      value: transaction.output_msat,
      itemStyle: {
        color: transaction.output_msat >= transaction.input_msat ? '#10b981' : '#f97316',
      },
    }));
    const labelColor = isDark ? '#9ca3af' : '#6b7280';
    const axisColor = isDark ? '#374151' : '#d1d5db';
    const gridColor = isDark ? '#374151' : '#e5e7eb';
    const tooltipBackground = isDark ? '#1f2937' : '#ffffff';
    const tooltipText = isDark ? '#ffffff' : '#111827';

    return {
      animation: false,
      grid: { left: '3%', right: '4%', top: '12%', bottom: '15%', outerBoundsMode: 'same', outerBoundsContain: 'axisLabel' },
      xAxis: {
        type: 'category',
        data: labels,
        axisLabel: { color: labelColor, fontSize: 10 },
        axisLine: { lineStyle: { color: axisColor } },
      },
      yAxis: {
        type: 'value',
        axisLabel: { color: labelColor, fontSize: 10, formatter: (value: number) => asBitcoin(value, 3) },
        axisLine: { lineStyle: { color: axisColor } },
        splitLine: { lineStyle: { color: gridColor, type: 'dashed' } },
      },
      tooltip: {
        trigger: 'axis',
        backgroundColor: tooltipBackground,
        borderColor: axisColor,
        textStyle: { color: tooltipText },
        formatter: (params: { dataIndex: number }[]) => {
          const transaction = ordered[params[0].dataIndex];
          const deposit = transaction.output_msat >= transaction.input_msat;
          const change = Math.abs(transaction.output_msat - transaction.input_msat);
          return [
            `<strong>Operation #${transaction.index}</strong>`,
            `Net wallet change: ${deposit ? '+' : '-'}${asBitcoin(change, 8)}`,
            `Wallet balance: ${asBitcoin(transaction.output_msat, 8)}`,
            `Fee: ${asBitcoin(transaction.fee_msat, 8)}`,
          ].join('<br/>');
        },
      },
      series: [{
        name: 'Wallet balance',
        type: 'line',
        data: values,
        smooth: false,
        symbol: 'circle',
        symbolSize: 8,
        lineStyle: { color: '#6366f1', width: 2 },
        areaStyle: { color: 'rgba(99, 102, 241, 0.12)' },
      }],
    };
  }, [isDark, transactions]);

  return <ReactEChartsCore echarts={echarts} option={option} notMerge lazyUpdate style={{ height: 320, width: '100%' }} />;
}
