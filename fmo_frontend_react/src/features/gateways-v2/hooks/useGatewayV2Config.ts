import { useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';
import { api } from '../../../services/api';
import type { ClientConfig } from '../../../types/api';
import type { GatewayPageHandoff } from '../../../utils/gateways';

/**
 * The federation's config and name. Coming from the federation page, both are
 * handed over in the router state, so neither is fetched again.
 */
export function useGatewayV2Config(id: string) {
  const handoff = useLocation().state as GatewayPageHandoff | null;
  const fromFederationPage = handoff?.id === id && handoff.config ? handoff : null;
  const [config, setConfig] = useState<ClientConfig | null>(fromFederationPage?.config ?? null);
  // Null while unknown, or when the federation has no name: never a made-up one
  const [name, setName] = useState<string | null>(fromFederationPage?.name || null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (fromFederationPage?.config) {
      setConfig(fromFederationPage.config);
      setName(fromFederationPage.name || null);
      setFailed(false);
      return;
    }
    // A direct link: the name is only in the full federation list
    const controller = new AbortController();
    Promise.all([api.getFederationConfig(id, controller.signal), api.getFederations(controller.signal).catch(() => [])])
      .then(([config, federations]) => {
        if (controller.signal.aborted) return;
        setConfig(config);
        setName(federations.find((fed) => fed.id === id)?.name || null);
      }).catch(() => { if (!controller.signal.aborted) setFailed(true); });
    return () => controller.abort();
  }, [id, fromFederationPage]);

  // The federation's Lightning v2 module instances
  const instances = Object.entries(config?.modules ?? {}).filter(([, module]) => module.kind === 'lnv2').map(([instance]) => Number(instance));
  return { config, name, failed, instances };
}
