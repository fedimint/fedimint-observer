import { useParams } from 'react-router-dom';
import { api } from '../services/api';
import { useApiResource } from '../hooks/useApiResource';
import { WalletV2Panel } from '../features/wallet-v2/components/WalletV2Panel';
import { ActivityTab } from '../features/federation/components/ActivityTab';
import { ConfigTab } from '../features/federation/components/ConfigTab';
import { UtxoInventory } from '../features/federation/components/utxos/UtxoInventory';
import { FederationHeader } from '../features/federation/components/FederationHeader';
import { FederationPageError, FederationPageLoading } from '../features/federation/components/FederationPageState';
import { useFederationDetails } from '../features/federation/hooks/useFederationDetails';
import { useFederationActivity } from '../features/federation/hooks/useFederationActivity';
import { useFederationUtxos } from '../features/federation/hooks/useFederationUtxos';
import type { FederationConfig } from '../utils/config';

export type FederationSectionName = 'activity' | 'utxos' | 'assets' | 'config';

function ActivitySection({ id }: { id: string }) {
  return <ActivityTab activity={useFederationActivity(id)} />;
}

function UtxoSection({ id, config }: { id: string; config: FederationConfig | null }) {
  const hasWalletV1 = config?.modules.includes('wallet') ?? false;
  const utxos = useFederationUtxos(id, hasWalletV1);
  const overview = useApiResource((signal) => api.getFederationOverview(id, signal), [id], { label: 'federation overview' });
  if (config && !hasWalletV1) return <p className="text-sm text-ink-3">This federation has no on-chain wallet module.</p>;
  return <UtxoInventory utxos={utxos} overview={overview.data} overviewFailed={overview.failed} network={config?.network} />;
}

/** One of a federation's data sections (activity, UTXOs, wallet assets, config), under its header tabs */
export function FederationSection({ section }: { section: FederationSectionName }) {
  // The router remounts this page per federation (App.tsx), so id never changes here
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
      {section === 'activity' && <ActivitySection id={id} />}
      {section === 'utxos' && <UtxoSection id={id} config={config} />}
      {section === 'assets' && (
        config && !config.modules.includes('walletv2')
          ? <p className="text-sm text-ink-3">This federation has no Wallet v2 module.</p>
          : <WalletV2Panel id={id} modules={config?.raw.modules ?? {}} active />
      )}
      {section === 'config' && <ConfigTab config={config?.raw} />}
    </div>
  );
}
