/**
 * Chart colours per mode. Canvas charts can't read CSS variables, so these
 * mirror the tokens in index.css. Series steps were validated with the
 * dataviz palette checker against each mode's surface (lightness band,
 * chroma, CVD separation, contrast >= 3:1).
 */
export interface ChartTheme {
  surface: string;
  ink: string;
  ink2: string;
  ink3: string;
  grid: string;
  axis: string;
  /** Operational series */
  series: string;
  seriesSoft: string;
  /** Reference lines such as the consensus threshold */
  reference: string;
  crit: string;
  warn: string;
}

export function chartTheme(isDark: boolean): ChartTheme {
  return isDark
    ? {
        surface: '#111a30', ink: '#eef2fa', ink2: '#c3cbdc', ink3: '#8d99b2', grid: '#1d2741', axis: '#2f3b5c',
        series: '#1ea7b0', seriesSoft: 'rgba(30, 167, 176, 0.16)', reference: '#c3cbdc', crit: '#d03b3b', warn: '#fab219',
      }
    : {
        surface: '#ffffff', ink: '#0b1220', ink2: '#3a4660', ink3: '#5b6781', grid: '#e6eaf1', axis: '#c9d1df',
        series: '#0e9aa7', seriesSoft: 'rgba(14, 154, 167, 0.12)', reference: '#3a4660', crit: '#d03b3b', warn: '#fab219',
      };
}

/** Tooltip box shared by every chart */
export function tooltipStyle(theme: ChartTheme) {
  return {
    backgroundColor: theme.surface,
    borderColor: theme.axis,
    borderWidth: 1,
    padding: [6, 10],
    textStyle: { color: theme.ink2, fontSize: 11 },
    extraCssText: 'border-radius:8px;box-shadow:0 6px 20px rgba(10,17,34,0.18);',
  };
}

export function axisStyle(theme: ChartTheme) {
  return {
    axisLine: { lineStyle: { color: theme.axis } },
    axisTick: { show: false },
    axisLabel: { color: theme.ink3, fontSize: 10 },
    splitLine: { lineStyle: { color: theme.grid } },
  };
}
