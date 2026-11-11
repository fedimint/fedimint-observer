import { api } from '../../../services/api';
import { parseConfig } from '../../../utils/config';
import { useApiResource } from '../../../hooks/useApiResource';

/**
 * The federation's summary from the observer's list, its config, and the
 * successor its guardians named, when the observer tracks that one.
 */
export function useFederationDetails(id: string) {
  return useApiResource(async (signal) => {
    // Every request needs only the id, so none waits for another; only the
    // invite fallback waits for the summary.
    // Only this federation's summary, not the whole list; the backend names
    // the successor when it tracks that one too
    const federationRequest = api.getFederationSummary(id, signal).then((fed) => {
      const successor = fed.successor_id && fed.successor_name
        ? { id: fed.successor_id, name: fed.successor_name }
        : null;
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
