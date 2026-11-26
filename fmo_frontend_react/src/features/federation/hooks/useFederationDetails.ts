import { api } from '../../../services/api';
import { parseConfig } from '../../../utils/config';
import { useApiResource } from '../../../hooks/useApiResource';


export function useFederationDetails(id: string) {
  const summary = useApiResource(async (signal) => {
    const federation = await api.getFederationSummary(id, signal);
    const successor = federation.successor_id && federation.successor_name
      ? { id: federation.successor_id, name: federation.successor_name }
      : null;
    return { federation, successor };
  }, [id], { label: 'federation summary' });
  const storedConfig = useApiResource(
    (signal) => api.getFederationConfig(id, signal).then(parseConfig),
    [id],
    { label: 'stored federation config' },
  );
  const invite = summary.data?.federation.invite;
  const useInvite = storedConfig.failed && Boolean(invite);
  const inviteConfig = useApiResource(
    (signal) => api.getFederationConfigByInvite(invite ?? '', signal).then(parseConfig),
    [invite],
    { enabled: useInvite, label: 'federation config by invite' },
  );
  const config = useInvite ? inviteConfig : storedConfig;
  return { summary, config };
}
