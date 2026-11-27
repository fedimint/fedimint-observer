import type { GatewayWindow } from '../../../types/api';
import { readStorage } from '../../../utils/storage.ts';

export const GATEWAY_WINDOWS: readonly GatewayWindow[] = ['24h', '7d', '30d', '90d'];
export const GATEWAY_FILTERS = ['all', 'online', 'degraded', 'offline', 'unknown', 'retired'] as const;
export type GatewayFilter = (typeof GATEWAY_FILTERS)[number];
export const GATEWAY_SORTS = ['freshness', 'status', 'uptime', 'activity'] as const;
export type GatewaySort = (typeof GATEWAY_SORTS)[number];
export const SORT_DIRECTIONS = ['asc', 'desc'] as const;
export type SortDirection = (typeof SORT_DIRECTIONS)[number];
export const DEFAULT_WINDOW: GatewayWindow = '7d';

export const WINDOW_STORAGE_KEY = 'gatewayWindow';
export const GATEWAY_LIST = 'gateways-v1';

export interface GatewayView {
  timeWindow: GatewayWindow;
  filter: GatewayFilter;
  sort: GatewaySort;
  direction: SortDirection;
}

/** One of `options` matching `value` regardless of case, or null */
export function parseOption<T extends string>(options: readonly T[], value: string | null): T | null {
  const lower = value?.toLowerCase();
  return options.find((option) => option === lower) ?? null;
}

export function defaultDirection(sort: GatewaySort): SortDirection {
  return sort === 'status' ? 'asc' : 'desc';
}

export function storedGatewayWindow(): GatewayWindow | null {
  return parseOption(GATEWAY_WINDOWS, readStorage(WINDOW_STORAGE_KEY));
}

/** The gateway list's window, filter and order from the address bar; the window falls back to the one stored on this device */
export function gatewayViewFrom(params: URLSearchParams, storedWindow: GatewayWindow | null): GatewayView {
  const sort = parseOption(GATEWAY_SORTS, params.get('sort')) ?? 'freshness';
  return {
    timeWindow: parseOption(GATEWAY_WINDOWS, params.get('window')) ?? storedWindow ?? DEFAULT_WINDOW,
    filter: parseOption(GATEWAY_FILTERS, params.get('status')) ?? 'all',
    sort,
    direction: parseOption(SORT_DIRECTIONS, params.get('dir')) ?? defaultDirection(sort),
  };
}
