import { useFederation } from '../features/federation/hooks/useFederation';
import { GatewaysPanel } from '../features/gateways/components/GatewaysPanel';
import { GatewaysV2Panel } from '../features/gateways-v2/components/GatewaysV2Panel';
import { instancesOf } from '../utils/config';

/** A federation's Lightning v1 or v2 gateways, under the federation's header tabs */
export function FederationGateways({ version }: { version: 1 | 2 }) {
  const { id, federation, moduleKinds } = useFederation();
  if (version === 1) return <GatewaysPanel id={id} federation={federation} />;
  return <GatewaysV2Panel id={id} instances={instancesOf(moduleKinds ?? {}, 'lnv2')} />;
}
