import { api } from '../../../services/api';
import { parseConfig } from '../../../utils/config';
import { useApiResource } from '../../../hooks/useApiResource';


export function useFederationDetails(id: string) {
  return useApiResource(async (signal) => {
    const federationRequest = api.getFederationSummary(id, signal).then((fed) => {
      const successor = fed.successor_id && fed.successor_name
        ? { id: fed.successor_id, name: fed.successor_name }
        : null;
      return { fed, successor };
    });
    const configRequest = api.getFederationConfig(id, signal)
      .catch(async () => api.getFederationConfigByInvite((await federationRequest).fed.invite, signal))
      .then(parseConfig);

    const [{ fed, successor }, config] = await Promise.all([federationRequest, configRequest]);
    return { federation: fed, successor, config };
  }, [id], { label: 'federation details' });
}
