import { useState } from 'react';
import { useParams, Link } from 'react-router-dom';
import { api } from '../services/api';
import { errorMessage } from '../utils/errors';
import { useApiResource } from '../hooks/useApiResource';
import { Tabs, type TabItem } from '../components/ui/Tabs';
import { WalletV2Panel } from '../features/wallet-v2/components/WalletV2Panel';
import { ExpiryNotice } from '../features/federation/components/ExpiryNotice';
import { GuardiansPanel } from '../features/federation/components/GuardiansPanel';
import { FederationInfoPanel } from '../features/federation/components/FederationInfoPanel';
import { RecommendPanel } from '../features/federation/components/RecommendPanel';
import { ActivityTab } from '../features/federation/components/ActivityTab';
import { ConfigTab } from '../features/federation/components/ConfigTab';
import { UtxoInventory } from '../features/federation/components/utxos/UtxoInventory';
import { useFederationDetails } from '../features/federation/hooks/useFederationDetails';
import { useGuardianHealth } from '../features/federation/hooks/useGuardianHealth';
import { useFederationActivity } from '../features/federation/hooks/useFederationActivity';
import { useFederationUtxos } from '../features/federation/hooks/useFederationUtxos';

type Tab = 'activity' | 'utxos' | 'assets' | 'config';

export function FederationDetail() {
  // The router remounts this page per federation (App.tsx), so id never changes here
  const { id = '' } = useParams<{ id: string }>();
  const details = useFederationDetails(id);
  const guardians = useGuardianHealth(id);
  const activity = useFederationActivity(id);
  const overview = useApiResource((signal) => api.getFederationOverview(id, signal), [id], { label: 'federation overview' });
  const [activeTab, setActiveTab] = useState<Tab>('activity');
  // UTXOs load the first time their tab opens, and are only ever shown there
  const [utxosRequested, setUtxosRequested] = useState(false);
  // Once opened, the wallet panel stays mounted so switching tabs keeps its
  // history instead of fetching it again.
  const [assetsOpened, setAssetsOpened] = useState(false);

  const federation = details.data?.federation ?? null;
  const config = details.data?.config ?? null;
  const hasWalletV1 = config?.modules.includes('wallet') ?? false;
  const hasWalletV2 = config?.modules.includes('walletv2') ?? false;
  const utxos = useFederationUtxos(id, utxosRequested && hasWalletV1);

  if (details.loading) {
    return (
      <div className="flex justify-center items-center min-h-[400px]">
        <div className="text-gray-500 dark:text-gray-400">Loading...</div>
      </div>
    );
  }

  if (details.failed || !federation) {
    return (
      <div className="flex justify-center items-center min-h-[400px]">
        <div className="text-red-500">Error: {errorMessage(details.error, '') || 'Federation not found'}</div>
      </div>
    );
  }

  const expiry = federation.expiry_timestamp ? new Date(federation.expiry_timestamp * 1000) : null;
  // The invite is hidden while every guardian is known to be offline, since joining
  // would fail. When the guardians could not be checked, the federation list's own
  // health decides: an unknown status is no reason to withhold a working invite.
  const showInvite = guardians.status === 'loaded'
    ? guardians.hasOnlineGuardian
    : guardians.status === 'failed' && federation.health !== 'offline';

  const tabs: TabItem<Tab>[] = [
    { id: 'activity', label: 'Activity' },
    ...(hasWalletV1 ? [{ id: 'utxos' as const, label: 'UTXOs' }] : []),
    ...(hasWalletV2 ? [{ id: 'assets' as const, label: 'Wallet Assets' }] : []),
    { id: 'config', label: 'Config' },
  ];
  const selectTab = (tab: Tab) => {
    setActiveTab(tab);
    if (tab === 'utxos') setUtxosRequested(true);
    if (tab === 'assets') setAssetsOpened(true);
  };

  return (
    <div className="py-4 sm:py-8 px-4 sm:px-0">
      <div className="mb-4 sm:mb-6">
        <Link
          to="/"
          className="text-sm sm:text-base text-blue-600 dark:text-blue-400 hover:underline"
        >
          ← Back to Federations
        </Link>
      </div>

      <h1 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white mb-6 sm:mb-8 wrap-break-word">
        {federation.name || 'Unnamed Federation'}
      </h1>
      {expiry && <ExpiryNotice federation={federation} expiry={expiry} successor={details.data?.successor ?? null} />}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 sm:gap-6">
        <GuardiansPanel
          className="lg:col-span-2"
          config={config}
          health={guardians.health}
          status={guardians.status}
          onRetry={guardians.retry}
        />

        <div className="space-y-4 sm:space-y-6">
          <FederationInfoPanel
            id={id}
            federation={federation}
            config={config}
            showInvite={showInvite}
            guardiansUnchecked={guardians.status === 'failed'}
          />
          <RecommendPanel id={id} network={config?.network} />
        </div>
      </div>

      <div className="mt-4 sm:mt-6">
        <Tabs items={tabs} active={activeTab} onSelect={selectTab} />

        <div className="mt-4 sm:mt-6">
          {activeTab === 'activity' && <ActivityTab activity={activity} />}
          {assetsOpened && hasWalletV2 && (
            <div hidden={activeTab !== 'assets'}>
              <WalletV2Panel id={id} modules={config?.raw.modules ?? {}} active={activeTab === 'assets'} />
            </div>
          )}
          {activeTab === 'utxos' && (
            <UtxoInventory utxos={utxos} overview={overview.data} overviewFailed={overview.failed} network={config?.network} />
          )}
          {activeTab === 'config' && <ConfigTab config={config?.raw} />}
        </div>
      </div>
    </div>
  );
}
