import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../../../services/api';
import { isInviteCodeQuery, lookupForInput, searchKey, type InviteLookup } from '../lib/federationList';

interface InFlightLookup {
  query: string;
  controller: AbortController;
}

/**
 * Search query state plus explicit lookup of an invite code's federation ID.
 * Lookup state is keyed by query, so it never applies to a different input.
 */
export function useFederationSearch() {
  const [searchQuery, setSearchQuery] = useState('');
  const [lookup, setLookup] = useState<InviteLookup | null>(null);
  const inFlightRef = useRef<InFlightLookup | null>(null);

  const current = lookupForInput(lookup, searchQuery);
  const resolvedFederationId = current?.status === 'done' ? current.federationId : null;
  const isResolvingInvite = current?.status === 'loading';
  const lookupError = current?.status === 'error' ? current.message : null;

  const abortInFlight = useCallback(() => {
    inFlightRef.current?.controller.abort();
    inFlightRef.current = null;
  }, []);

  // Abort on unmount so a late response never sets state
  useEffect(() => abortInFlight, [abortInFlight]);

  // Every edit, including the clear button, goes through here
  const changeSearchQuery = useCallback(
    (value: string) => {
      setSearchQuery(value);
      if (inFlightRef.current && inFlightRef.current.query !== searchKey(value)) {
        abortInFlight();
        // Drop the aborted lookup's loading state, keep finished results
        setLookup((prev) => (prev?.status === 'loading' ? null : prev));
      }
    },
    [abortInFlight],
  );

  const resolveInvite = useCallback(async () => {
    const query = searchKey(searchQuery);
    if (!isInviteCodeQuery(query)) return;

    abortInFlight();
    const controller = new AbortController();
    inFlightRef.current = { query, controller };
    setLookup({ query, status: 'loading' });

    try {
      const federationId = await api.getFederationIdByInvite(query, controller.signal);
      if (!controller.signal.aborted) setLookup({ query, status: 'done', federationId });
    } catch (err: unknown) {
      if (!controller.signal.aborted) {
        const message = err instanceof Error ? err.message : 'Could not resolve federation ID for this invite code';
        setLookup({ query, status: 'error', message });
      }
    } finally {
      if (inFlightRef.current?.controller === controller) inFlightRef.current = null;
    }
  }, [searchQuery, abortInFlight]);

  return {
    searchQuery,
    setSearchQuery: changeSearchQuery,
    isInvite: isInviteCodeQuery(searchQuery),
    resolveInvite,
    resolvedFederationId,
    isResolvingInvite,
    lookupError,
  };
}
