import { useCallback } from 'react';
import { api } from '../../../services/api';
import type { FederationGroup, FederationPage, FederationSortKey, SortDir } from '../../../types/api';
import { useCursorPages } from '../../../hooks/useCursorPages';
import type { ActivityScale } from '../lib/federationList';

const REFRESH_MS = 60_000;
const nextAfter = (page: FederationPage) => page.next_after;
const NO_SCALE: ActivityScale = { maxTransaction: 0, maxVolume: 0 };

/**
 * One group of the home page's list (active, offline or shut down), searched,
 * ordered and paged by the backend. Page 1 refreshes each minute; each Next
 * asks the backend for the following page.
 */
export function useFederationGroup(group: FederationGroup, search: string, sort: FederationSortKey, dir: SortDir, limit: number) {
  const fetchPage = useCallback(
    (after: string | undefined, signal: AbortSignal) => api.getFederationsPage({ group, search, sort, dir, limit, after }, signal),
    [group, search, sort, dir, limit],
  );
  const pages = useCursorPages({ fetchPage, nextCursor: nextAfter, refreshMs: REFRESH_MS });
  const scale = pages.page?.activity_scale;
  return {
    ...pages,
    federations: pages.page?.federations ?? [],
    total: pages.page?.total ?? null,
    /** Over every federation, so charts on every page and group share one scale */
    scale: scale ? { maxTransaction: scale.max_transactions, maxVolume: scale.max_volume_msat } : NO_SCALE,
    loading: pages.page === null && !pages.error,
    failed: pages.page === null && pages.error !== null,
  };
}
