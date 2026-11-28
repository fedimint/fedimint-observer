import { useMemo } from 'react';
import { Chart } from '../../../components/ui/Chart';
import { useTheme } from '../../../hooks/useTheme';
import { chartTheme } from '../../reliability/lib/chartTheme';
import type { WalletV2OnChainTransaction } from '../../../types/api';
import { asBitcoin } from '../../../utils/format';

interface WalletV2ChainChartProps {
  transactions: WalletV2OnChainTransaction[];
}

export function WalletV2ChainChart({ transactions }: WalletV2ChainChartProps) {
  const { isDark } = useTheme();
  const option = useMemo(() => {
    const theme = chartTheme(isDark);
    const ordered = [...transactions].sort((a, b) => a.index - b.index);
    const labels = ordered.map((transaction) => `#${transaction.index}`);
    const values = ordered.map((transaction) => ({
      value: transaction.output_msat,
      itemStyle: {
        color: transaction.output_msat >= transaction.input_msat ? theme.ok : theme.serious,
      },
    }));
    const labelColor = theme.ink3;
    const axisColor = theme.axis;
    const gridColor = theme.grid;
    const tooltipBackground = theme.surface;
    const tooltipText = theme.ink2;

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
        lineStyle: { color: theme.series, width: 2 },
        areaStyle: { color: theme.seriesSoft },
      }],
    };
  }, [isDark, transactions]);

  return <Chart option={option} height={320} />;
}
