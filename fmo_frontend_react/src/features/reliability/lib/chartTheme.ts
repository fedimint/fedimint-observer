
export interface ChartTheme {
  surface: string;
  ink: string;
  ink2: string;
  ink3: string;
  grid: string;
  axis: string;

  series: string;
  seriesSoft: string;

  reference: string;
  crit: string;
  warn: string;

  ok: string;
  serious: string;
}

export function chartTheme(isDark: boolean): ChartTheme {
  return isDark
    ? {
        surface: '#111a30', ink: '#eef2fa', ink2: '#c3cbdc', ink3: '#8d99b2', grid: '#1d2741', axis: '#2f3b5c',
        series: '#4f9ff0', seriesSoft: 'rgba(79, 159, 240, 0.16)', reference: '#c3cbdc', crit: '#c92a5a', warn: '#f2c14e',
        ok: '#14b39b', serious: '#e8743b',
      }
    : {
        surface: '#ffffff', ink: '#0b1220', ink2: '#3a4660', ink3: '#5b6781', grid: '#e6eaf1', axis: '#c9d1df',
        series: '#1d6fd8', seriesSoft: 'rgba(29, 111, 216, 0.12)', reference: '#3a4660', crit: '#b3123f', warn: '#f2c14e',
        ok: '#0f9f8a', serious: '#e8743b',
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
