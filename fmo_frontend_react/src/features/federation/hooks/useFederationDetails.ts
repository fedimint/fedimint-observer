import { api } from '../../../services/api';
import { parseConfig } from '../../../utils/config';
import { federationNames } from '../../../utils/migration';
import { useApiResource } from '../../../hooks/useApiResource';

/**
 * The federation's summary from the observer's list, its config, and the
 * successor its guardians named, when the observer tracks that one.
 */
export function useFederationDetails(id: string) {
  return useApiResource(async (signal) => {
    // Every request needs only the id, so none waits for another; only the
    // invite fallback waits for the federation list.
    const federationRequest = api.getFederations(signal).then((federations) => {
      const fed = federations.find((f) => f.id === id);
      if (!fed) {
        throw new Error('Federation not found');
      }
      // The successor gets a link and a name only when the observer tracks it
      const successorName = fed.successor_id ? federationNames(federations).get(fed.successor_id) : undefined;
      const successor = fed.successor_id && successorName !== undefined ? { id: fed.successor_id, name: successorName } : null;
      return { fed, successor };
    });
    // The config by federation ID works for actively observed federations; the
    // invite code works for any federation with a valid invite
    const configRequest = api.getFederationConfig(id, signal)
      .catch(async () => api.getFederationConfigByInvite((await federationRequest).fed.invite, signal))
      .then(parseConfig);

    const [{ fed, successor }, config] = await Promise.all([federationRequest, configRequest]);
    return { federation: fed, successor, config };
  }, [id], { label: 'federation details' });
}
