import { useCallback, useEffect, useRef, useState } from 'react';
import { api } from '../../../services/api';
import { isInviteCodeQuery, lookupForInput, searchKey, type InviteLookup } from '../lib/federationList';

interface InFlightLookup {
  query: string;
  controller: AbortController;
}

/** Search query plus explicit invite lookup, keyed by query */
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

  useEffect(() => abortInFlight, [abortInFlight]);

  const changeSearchQuery = useCallback(
    (value: string) => {
      setSearchQuery(value);
      if (inFlightRef.current && inFlightRef.current.query !== searchKey(value)) {
        abortInFlight();
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
