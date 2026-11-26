import { useFederation } from '../features/federation/hooks/useFederation';
import { GatewaysPanel } from '../features/gateways/components/GatewaysPanel';
import { GatewaysV2Panel } from '../features/gateways-v2/components/GatewaysV2Panel';

/** A federation's Lightning v1 or v2 gateways, under the federation's header tabs */
export function FederationGateways({ version }: { version: 1 | 2 }) {
  const { id, config, federation } = useFederation();
  return version === 1 ? <GatewaysPanel id={id} federation={federation} /> : <GatewaysV2Panel id={id} modules={config?.raw.modules ?? {}} />;
}
