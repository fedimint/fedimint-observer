import { useCallback } from 'react';
import { useApiResource } from '../../hooks/useApiResource';
import { useCursorPages } from '../../hooks/useCursorPages';
import { manifoldApi } from './source';
import type { DirectoryQuery } from './lib/directory';
import type { Sourced } from './types';

const REFRESH_MS = 60_000;
const nextAfter = <T,>(page: Sourced<T>) => page.page?.next_after ?? null;

/** One page of the directory at a time; each Next asks the backend for the following page */
export function useManifoldGuardians(query: DirectoryQuery, limit: number) {
  const fetchPage = useCallback(
    (after: string | undefined, signal: AbortSignal) => manifoldApi.listGuardians(query, limit, after, signal),
    [query, limit],
  );
  return useCursorPages({ fetchPage, nextCursor: nextAfter, refreshMs: REFRESH_MS });
}

export const useManifoldGuardian = (fmanPubkey: string) =>
  useApiResource((signal) => manifoldApi.getGuardian(fmanPubkey, signal), [fmanPubkey], { label: 'Manifold Guardian profile' });

export function useFlipProviders(limit: number) {
  const fetchPage = useCallback(
    (after: string | undefined, signal: AbortSignal) => manifoldApi.listFlipProviders(limit, after, signal),
    [limit],
  );
  return useCursorPages({ fetchPage, nextCursor: nextAfter, refreshMs: REFRESH_MS });
}
