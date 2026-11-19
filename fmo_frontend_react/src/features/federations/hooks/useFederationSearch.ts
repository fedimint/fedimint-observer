import { useCallback, useRef, useState } from 'react';
import { api } from '../../../services/api';
import { isInviteCodeQuery, normalizeSearchInput } from '../lib/federationList';

interface QueryResolution {
  query: string;
  federationId: string;
}

/**
 * Manages search query state and explicit on-demand resolution of Fedimint invite codes
 * to federation IDs. Keying results by query prevents stale race conditions.
 */
export function useFederationSearch() {
  const [searchQuery, setSearchQuery] = useState('');
  const [resolution, setResolution] = useState<QueryResolution | null>(null);
  const [isResolvingInvite, setIsResolvingInvite] = useState(false);
  const [lookupError, setLookupError] = useState<string | null>(null);
  const abortControllerRef = useRef<AbortController | null>(null);

  const cleanQuery = normalizeSearchInput(searchQuery).toLowerCase();
  const isInvite = isInviteCodeQuery(searchQuery);

  // A result belongs strictly to the normalized query that produced it.
  // Consumed only if the key still matches the current input.
  const resolvedFederationId =
    resolution && resolution.query === cleanQuery ? resolution.federationId : null;

  const resolveInvite = useCallback(async () => {
    const targetQuery = normalizeSearchInput(searchQuery).toLowerCase();
    if (!isInviteCodeQuery(targetQuery)) {
      return;
    }

    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }

    const controller = new AbortController();
    abortControllerRef.current = controller;
    setIsResolvingInvite(true);
    setLookupError(null);

    try {
      const fedId = await api.getFederationIdByInvite(targetQuery, controller.signal);
      if (!controller.signal.aborted) {
        setResolution({ query: targetQuery, federationId: fedId });
      }
    } catch (err: unknown) {
      if (!controller.signal.aborted) {
        setResolution(null);
        setLookupError(
          err instanceof Error ? err.message : 'Could not resolve federation ID for this invite code',
        );
      }
    } finally {
      if (!controller.signal.aborted) {
        setIsResolvingInvite(false);
      }
    }
  }, [searchQuery]);

  const clearSearch = useCallback(() => {
    if (abortControllerRef.current) {
      abortControllerRef.current.abort();
    }
    setSearchQuery('');
    setResolution(null);
    setLookupError(null);
    setIsResolvingInvite(false);
  }, []);

  return {
    searchQuery,
    setSearchQuery,
    clearSearch,
    isInvite,
    resolveInvite,
    resolvedFederationId,
    isResolvingInvite,
    lookupError,
  };
}
