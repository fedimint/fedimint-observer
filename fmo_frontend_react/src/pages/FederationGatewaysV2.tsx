import { Link, useParams, useSearchParams } from 'react-router-dom';
import type { GatewayWindow } from '../types/api';
import { card, muted } from '../components/ui/styles';
import { GatewayV2Modules } from '../features/gateways-v2/components/GatewayV2Modules';
import { GatewayV2WindowPicker } from '../features/gateways-v2/components/GatewayV2WindowPicker';
import { useGatewayV2Config } from '../features/gateways-v2/hooks/useGatewayV2Config';
import { GATEWAY_V2_WINDOWS } from '../features/gateways-v2/lib/gatewaysV2';

export function FederationGatewaysV2() {
  const { id = '' } = useParams<{ id: string }>();
  const [params, setParams] = useSearchParams();
  const selectedWindow = params.get('window');
  const timeWindow = GATEWAY_V2_WINDOWS.includes(selectedWindow as GatewayWindow) ? selectedWindow as GatewayWindow : '7d';
  const selectWindow = (window: GatewayWindow) => setParams((previous) => {
    const next = new URLSearchParams(previous);
    next.set('window', window);
    return next;
  });
  const { config, name, failed, instances } = useGatewayV2Config(id);

  return (
    <div className="py-6 text-gray-900 dark:text-white">
      <Link to={`/federations/${id}`} className="text-sm text-blue-600 hover:underline dark:text-blue-400">← Back to {name ?? 'federation'}</Link>
      <div className="mb-6 mt-4 flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
        <div>
          <div className="flex flex-wrap items-center gap-3">
            <h1 className="text-2xl font-bold sm:text-3xl">Gateway Details</h1>
            <span className="rounded-full bg-blue-100 px-2.5 py-1 text-xs font-semibold text-blue-700 dark:bg-blue-900/40 dark:text-blue-300">Lightning v2</span>
          </div>
          <p className={`mt-2 text-sm ${muted}`}>Gateway health, routing fees and observed contract activity for {name ?? 'this federation'}.</p>
        </div>
        <GatewayV2WindowPicker value={timeWindow} onChange={selectWindow} />
      </div>
      {failed ? (
        <p role="alert" className={`${card} p-5`}>Could not load federation configuration. Reload to try again.</p>
      ) : !config ? (
        <p className={muted}>Loading gateway details…</p>
      ) : instances.length ? (
        <GatewayV2Modules id={id} instances={instances} timeWindow={timeWindow} />
      ) : (
        <p>This federation has no Lightning v2 module.</p>
      )}
    </div>
  );
}
