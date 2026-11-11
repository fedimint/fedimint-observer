import ReactEChartsCore from 'echarts-for-react/lib/core';
import * as echarts from 'echarts/core';
import { BarChart, BoxplotChart, LineChart } from 'echarts/charts';
import { GridComponent, MarkLineComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsOption } from 'echarts';

// Only what the reliability charts use, instead of all of echarts
echarts.use([LineChart, BoxplotChart, BarChart, GridComponent, TooltipComponent, MarkLineComponent, CanvasRenderer]);

interface EChartProps {
  option: EChartsOption;
  height: number;
  /** What the chart shows, for screen readers; the page also has the figures as text */
  label: string;
}

export function EChart({ option, height, label }: EChartProps) {
  return (
    <div role="img" aria-label={label}>
      <ReactEChartsCore echarts={echarts} option={option} style={{ height, width: '100%' }} notMerge lazyUpdate />
    </div>
  );
}
