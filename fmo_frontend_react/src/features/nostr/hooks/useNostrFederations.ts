import { useEffect, useMemo, useState } from 'react';
import { api } from '../../../services/api';
import { errorMessage } from '../../../utils/errors';
import { useApiResource } from '../../../hooks/useApiResource';
import { lookUpNames, type NostrFederation } from '../lib/federationNames';

/**
 * Federations announced on Nostr, named and with health where the observer
 * knows them. The announced list and the observer's own list load
 * independently: the observer's list only adds names and health. Names it
 * lacks are then looked up from each invite code in the background.
 */
export function useNostrFederations() {
  const nostr = useApiResource((signal) => api.getNostrFederations(signal), [], { label: 'nostr federations' });
  const observed = useApiResource((signal) => api.getFederations(signal), [], { label: 'observed federations' });
  const [lookedUpNames, setLookedUpNames] = useState<Map<string, string>>(() => new Map());
  const nostrList = nostr.data;

  const observedById = useMemo(
    () => (observed.data ? new Map(observed.data.map((fed) => [fed.id, fed])) : null),
    [observed.data]
  );

  // Names the observer's list lacks are fetched from /config/{invite}/meta in the
  // background, once that list has loaded or failed
  const observedSettled = observed.data !== null || observed.failed;
  useEffect(() => {
    if (!nostrList || !observedSettled) return;
    const controller = new AbortController();
    const unnamed = Object.entries(nostrList)
      .filter(([id]) => !observedById?.get(id)?.name)
      .map(([id, invite]) => ({ id, name: null, invite }));
    void lookUpNames(unnamed, controller.signal, (names) => {
      setLookedUpNames((previous) => new Map([...previous, ...names]));
    });
    return () => controller.abort();
  }, [nostrList, observedById, observedSettled]);

  const federations = useMemo<NostrFederation[]>(
    () => Object.entries(nostrList ?? {}).map(([id, invite]) => {
      const federation = observedById?.get(id);
      return { id, invite, name: federation?.name || lookedUpNames.get(id) || null, health: federation?.health };
    }),
    [nostrList, observedById, lookedUpNames]
  );

  const online = useMemo(() => federations.filter((fed) => fed.name && fed.health !== 'offline'), [federations]);
  const offline = useMemo(() => federations.filter((fed) => !fed.name || fed.health === 'offline'), [federations]);

  return {
    online,
    offline,
    loading: nostrList === null && !nostr.failed,
    error: nostr.failed ? errorMessage(nostr.error) : null,
    retry: nostr.reload,
    /** The observer's federations by id; null until they load */
    observedById,
    observedError: observed.failed ? errorMessage(observed.error) : null,
    retryObserved: observed.reload,
  };
}
