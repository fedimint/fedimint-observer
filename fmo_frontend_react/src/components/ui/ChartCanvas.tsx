import ReactEChartsCore from 'echarts-for-react/lib/core';
import * as echarts from 'echarts/core';
import { LineChart } from 'echarts/charts';
import { GridComponent, TooltipComponent } from 'echarts/components';
import { CanvasRenderer } from 'echarts/renderers';
import type { EChartsReactProps } from 'echarts-for-react/lib/types';

echarts.use([LineChart, GridComponent, TooltipComponent, CanvasRenderer]);

export interface ChartCanvasProps {
  option: EChartsReactProps['option'];
  height: number;
  notMerge?: boolean;
  onEvents?: EChartsReactProps['onEvents'];
}

export function ChartCanvas({ option, height, notMerge = true, onEvents }: ChartCanvasProps) {
  return <ReactEChartsCore echarts={echarts} option={option} style={{ height, width: '100%' }} notMerge={notMerge} lazyUpdate onEvents={onEvents} />;
}
