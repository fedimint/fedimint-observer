/** "99.8%"; one decimal below 100, none at 100 */
export function formatPct(value: number | null | undefined, fallback = '—'): string {
  if (value === null || value === undefined || Number.isNaN(value)) return fallback;
  if (value >= 99.95) return '100%';
  return `${value.toFixed(value >= 10 ? 1 : 2)}%`;
}

export function formatMs(value: number | null | undefined, fallback = '—'): string {
  if (value === null || value === undefined) return fallback;
  return value >= 1000 ? `${(value / 1000).toFixed(2)} s` : `${Math.round(value)} ms`;
}

export function guardianLabel(id: number, configName: string | null): string {
  return configName ? `Guardian ${id} · ${configName}` : `Guardian ${id}`;
}
