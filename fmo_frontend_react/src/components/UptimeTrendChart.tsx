import ReactEChartsCore from 'echarts-for-react/lib/core';
import * as echarts from 'echarts/core';
import { LineChart } from 'echarts/charts';
import { GridComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsOption } from 'echarts';

// Register only what the chart uses instead of loading all of echarts
echarts.use([LineChart, GridComponent, TooltipComponent, CanvasRenderer]);

export function UptimeTrendChart({ option }: { option: EChartsOption }) {
  return <ReactEChartsCore echarts={echarts} option={option} style={{ height: 210 }} notMerge lazyUpdate />;
}
