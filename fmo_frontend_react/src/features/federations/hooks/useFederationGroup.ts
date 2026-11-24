import { useCallback } from 'react';
import { api } from '../../../services/api';
import type { FederationGroup, FederationPage, FederationSortKey, SortDir } from '../../../types/api';
import { useCursorPages } from '../../../hooks/useCursorPages';
import type { ActivityScale } from '../lib/federationList';

const REFRESH_MS = 60_000;
const nextAfter = (page: FederationPage) => page.next_after;
const NO_SCALE: ActivityScale = { maxTransaction: 0, maxVolume: 0 };


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

    scale: scale ? { maxTransaction: scale.max_transactions, maxVolume: scale.max_volume_msat } : NO_SCALE,
    loading: pages.page === null && !pages.error,
    failed: pages.page === null && pages.error !== null,
  };
}
