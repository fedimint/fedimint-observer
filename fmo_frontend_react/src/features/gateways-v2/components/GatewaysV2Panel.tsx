import { useSearchParams } from 'react-router-dom';
import type { ClientConfig, GatewayWindow } from '../../../types/api';
import { muted } from '../../../components/ui/styles';
import { GATEWAY_V2_WINDOWS } from '../lib/gatewaysV2';
import { GatewayV2Modules } from './GatewayV2Modules';
import { GatewayV2WindowPicker } from './GatewayV2WindowPicker';

/** The federation's Lightning v2 gateways, shown in a tab of the federation page */
export function GatewaysV2Panel({ id, modules }: { id: string; modules: ClientConfig['modules'] }) {
  const [params, setParams] = useSearchParams();
  const selectedWindow = params.get('window');
  const timeWindow = GATEWAY_V2_WINDOWS.includes(selectedWindow as GatewayWindow) ? selectedWindow as GatewayWindow : '7d';
  const selectWindow = (window: GatewayWindow) => setParams((previous) => {
    const next = new URLSearchParams(previous);
    next.set('window', window);
    return next;
  }, { replace: true });
  // The federation's Lightning v2 module instances
  const instances = Object.entries(modules ?? {}).filter(([, module]) => module.kind === 'lnv2').map(([instance]) => Number(instance));

  return (
    <div className="text-gray-900 dark:text-white">
      <div className="mb-4 flex flex-col justify-between gap-3 sm:flex-row sm:items-end">
        <div>
          <h2 className="text-lg font-semibold">Lightning v2 gateways</h2>
          <p className={`mt-1 text-xs ${muted}`}>Gateway health, routing fees and observed contract activity</p>
        </div>
        <GatewayV2WindowPicker value={timeWindow} onChange={selectWindow} />
      </div>
      {instances.length ? (
        <GatewayV2Modules id={id} instances={instances} timeWindow={timeWindow} />
      ) : (
        <p className={muted}>This federation has no Lightning v2 module.</p>
      )}
    </div>
  );
}
