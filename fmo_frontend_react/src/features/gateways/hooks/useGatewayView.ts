import { useCallback, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import type { GatewayWindow } from '../../../types/api';
import { readStorage, writeStorage } from '../../../utils/storage';
import {
  DEFAULT_WINDOW,
  GATEWAY_FILTERS,
  GATEWAY_SORTS,
  GATEWAY_WINDOWS,
  SORT_DIRECTIONS,
  defaultDirection,
  parseOption,
  type GatewayFilter,
  type GatewaySort,
  type SortDirection,
} from '../lib/gatewayRows';

const WINDOW_STORAGE_KEY = 'gatewayWindow';

type ViewChanges = Partial<Record<'window' | 'status' | 'sort' | 'dir', string | null>>;

/** Keeps gateway filters and ordering in the URL. */
export function useGatewayView() {
  const [searchParams, setSearchParams] = useSearchParams();
  // Reads the stored window once when the URL omits it.
  const [storedWindow] = useState(() => parseOption(GATEWAY_WINDOWS, readStorage(WINDOW_STORAGE_KEY)));
  const timeWindow = parseOption(GATEWAY_WINDOWS, searchParams.get('window')) ?? storedWindow ?? DEFAULT_WINDOW;
  const filter: GatewayFilter = parseOption(GATEWAY_FILTERS, searchParams.get('status')) ?? 'all';
  const sort: GatewaySort = parseOption(GATEWAY_SORTS, searchParams.get('sort')) ?? 'freshness';
  const direction: SortDirection = parseOption(SORT_DIRECTIONS, searchParams.get('dir')) ?? defaultDirection(sort);

  // Builds from the address bar so rapid updates cannot overwrite each other.
  const updateView = useCallback((changes: ViewChanges) => {
    const next = new URLSearchParams(window.location.search);
    for (const [key, value] of Object.entries(changes)) {
      if (value == null) next.delete(key);
      else next.set(key, value);
    }
    setSearchParams(next, { replace: true });
  }, [setSearchParams]);

  return {
    timeWindow,
    filter,
    sort,
    direction,
    selectWindow: (next: GatewayWindow) => {
      writeStorage(WINDOW_STORAGE_KEY, next);
      updateView({ window: next });
    },
    selectFilter: (next: GatewayFilter) => updateView({ status: next === 'all' ? null : next }),

    selectSort: (next: GatewaySort) => updateView({ sort: next === 'freshness' ? null : next, dir: null }),
    reverseDirection: () => {
      const next = direction === 'asc' ? 'desc' : 'asc';
      updateView({ dir: next === defaultDirection(sort) ? null : next });
    },
  };
}
