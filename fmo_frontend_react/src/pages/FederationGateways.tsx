import { useParams } from 'react-router-dom';
import { FederationHeader } from '../features/federation/components/FederationHeader';
import { FederationPageError, FederationPageLoading } from '../features/federation/components/FederationPageState';
import { useFederationDetails } from '../features/federation/hooks/useFederationDetails';
import { GatewaysPanel } from '../features/gateways/components/GatewaysPanel';
import { GatewaysV2Panel } from '../features/gateways-v2/components/GatewaysV2Panel';

/** A federation's Lightning v1 or v2 gateways, under the federation's header tabs */
export function FederationGateways({ version }: { version: 1 | 2 }) {
  const { id = '' } = useParams<{ id: string }>();
  const details = useFederationDetails(id);
  const federation = details.data?.federation ?? null;
  const config = details.data?.config ?? null;

  if (details.loading && !details.data) return <FederationPageLoading />;
  if (details.failed || !federation) {
    return <FederationPageError error={details.error} notFound={!details.failed || (details.error instanceof Error && details.error.message === 'Federation not found')} onRetry={details.reload} />;
  }

  return (
    <div>
      <FederationHeader id={id} federation={federation} config={config} />
      {version === 1 ? <GatewaysPanel id={id} /> : <GatewaysV2Panel id={id} modules={config?.raw.modules ?? {}} />}
    </div>
  );
}
