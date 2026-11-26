export type SortKey = 'assets' | 'volume' | 'transactions' | 'name';
export type SortDirection = 'asc' | 'desc';


export const DEFAULT_SORT_KEY: SortKey = 'assets';


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
