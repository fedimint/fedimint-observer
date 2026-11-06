import type { FederationSummary } from '../types/api';

export type SortKey = 'assets' | 'volume' | 'transactions' | 'name';
export type SortDirection = 'asc' | 'desc';

/** The home page's long-standing order: assets under management, largest first. */
export const DEFAULT_SORT_KEY: SortKey = 'assets';

/** Options in display order, each with the direction that reads "best first". */
export const SORT_OPTIONS: { key: SortKey; label: string; defaultDirection: SortDirection }[] = [
  { key: 'assets', label: 'Total assets', defaultDirection: 'desc' },
  { key: 'volume', label: 'Volume (7d)', defaultDirection: 'desc' },
  { key: 'transactions', label: 'Transactions (7d)', defaultDirection: 'desc' },
  { key: 'name', label: 'Name', defaultDirection: 'asc' },
];

export function isSortKey(value: string | null): value is SortKey {
  return SORT_OPTIONS.some((option) => option.key === value);
}

export function defaultDirectionFor(key: SortKey): SortDirection {
  return SORT_OPTIONS.find((option) => option.key === key)?.defaultDirection ?? 'desc';
}

/** The value a key sorts by; null when unknown, which always sorts last. */
function metric(fed: FederationSummary, key: Exclude<SortKey, 'name'>): number | null {
  switch (key) {
    case 'assets':
      return fed.deposits;
    case 'volume':
      return fed.last_7d_activity.reduce((sum, day) => sum + day.amount_transferred, 0);
    case 'transactions':
      return fed.last_7d_activity.reduce((sum, day) => sum + day.num_transactions, 0);
  }
}

function compareNullable<T>(a: T | null, b: T | null, compare: (a: T, b: T) => number, dir: number): number {
  if (a === null || b === null) return a === b ? 0 : a === null ? 1 : -1;
  return compare(a, b) * dir;
}

const byName = (a: string, b: string) => a.localeCompare(b, undefined, { sensitivity: 'base' });

export function compareFederations(a: FederationSummary, b: FederationSummary, key: SortKey, direction: SortDirection): number {
  const dir = direction === 'asc' ? 1 : -1;
  const primary = key === 'name'
    ? compareNullable(a.name || null, b.name || null, byName, dir)
    : compareNullable(metric(a, key), metric(b, key), (x, y) => x - y, dir);
  // Ties keep one order across refreshes, whatever order the server sends.
  return primary
    || compareNullable(a.name || null, b.name || null, byName, 1)
    || a.id.localeCompare(b.id);
}

/** A sorted copy; the input is not modified. */
export function sortFederations(feds: FederationSummary[], key: SortKey, direction: SortDirection): FederationSummary[] {
  return [...feds].sort((a, b) => compareFederations(a, b, key, direction));
}
