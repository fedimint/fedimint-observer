import { useMemo, useState } from 'react';
import { Link, Navigate, useParams, useSearchParams } from 'react-router-dom';
import type { GatewayWindow } from '../types/api';
import { card, muted } from '../components/ui/styles';
import { RetryButton } from '../components/ui/RetryButton';
import { errorMessage } from '../utils/errors';
import { GatewayV2WindowPicker } from '../features/gateways-v2/components/GatewayV2WindowPicker';
import { useGatewayV2Config } from '../features/gateways-v2/hooks/useGatewayV2Config';
import { GATEWAY_V2_WINDOWS } from '../features/gateways-v2/lib/gatewaysV2';
import { GatewayTabs } from '../features/gateways/components/GatewayTabs';
import { GatewayNodeRow } from '../features/gateway-nodes/components/GatewayNodeRow';
import { GatewayRegistrationItem } from '../features/gateway-nodes/components/GatewayRegistrationItem';
import { useGatewayNodes } from '../features/gateway-nodes/hooks/useGatewayNodes';
import { isMissingRoute, registrationKey, sortNodes } from '../features/gateway-nodes/lib/gatewayNodes';

/** A federation's gateways over both Lightning protocols, one entry per Lightning node */
export function FederationGatewayNodes() {
  const { id = '' } = useParams<{ id: string }>();
  const [params, setParams] = useSearchParams();
  const selectedWindow = params.get('window');
  const timeWindow = GATEWAY_V2_WINDOWS.includes(selectedWindow as GatewayWindow) ? selectedWindow as GatewayWindow : '7d';
  const selectWindow = (window: GatewayWindow) => setParams((previous) => {
    const next = new URLSearchParams(previous);
    next.set('window', window);
    return next;
  });
  const { name, config } = useGatewayV2Config(id);
  const { nodes, lnv1, overview } = useGatewayNodes(id, timeWindow);
  const [now] = useState(() => Date.now());
  const sorted = useMemo(() => sortNodes(nodes.data?.nodes ?? [], lnv1), [nodes.data, lnv1]);

  // Without the Lightning v2 module the observer has no grouped view; the
  // Lightning v1 page covers the rest, including a federation it doesn't observe
  if (nodes.failed && isMissingRoute(nodes.error)) return <Navigate replace to={`/federations/${id}/gateways`} />;

  const data = nodes.data;
  return (
    <div className="py-6 text-gray-900 dark:text-white">
      <Link to={`/federations/${id}`} className="text-sm text-blue-600 hover:underline dark:text-blue-400">← Back to {name ?? 'federation'}</Link>
      <div className="mb-6 mt-4 flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
        <div>
          <h1 className="text-2xl font-bold sm:text-3xl">Gateway Details</h1>
          <p className={`mt-2 text-sm ${muted}`}>
            Lightning gateways of {name ?? 'this federation'} by node, over Lightning v1 and v2. Lightning v1 uptime is how often the guardians listed the gateway; Lightning v2 reachability is how often its endpoint answered a probe.
          </p>
        </div>
        <GatewayV2WindowPicker value={timeWindow} onChange={selectWindow} />
      </div>
      <GatewayTabs id={id} name={name} config={config} />

      {nodes.failed ? (
        <div role="alert" className={`${card} flex flex-wrap items-center justify-between gap-3 p-5`}>
          <p>Could not load gateways: {errorMessage(nodes.error)}</p>
          <RetryButton onClick={nodes.reload} />
        </div>
      ) : !data || overview.loading ? (
        // Node names, order and Lightning v1 numbers come from the v1 overview, so wait for it too
        <p role="status" className={muted}>Loading gateways…</p>
      ) : (
        <>
          {overview.failed && !isMissingRoute(overview.error) && (
            <div role="alert" className={`${card} mb-3 flex flex-wrap items-center justify-between gap-3 p-4 text-sm`}>
              <p>Lightning v1 uptime and activity could not be loaded: {errorMessage(overview.error)}</p>
              <RetryButton onClick={overview.reload} />
            </div>
          )}
          {!data.activity_caught_up && <p role="status" className={`mb-3 text-xs ${muted}`}>Historical contract activity is being indexed. Totals may increase as the backfill catches up.</p>}
          <p className={`mb-3 text-xs ${muted}`}>
            A Lightning v2 endpoint reports its node key itself. It counts towards the node's Lightning v2 numbers only when a Lightning v1 registration of the node announces the same endpoint; otherwise it is marked unverified and its numbers stay under its own registration.
          </p>
          <section className={`${card} divide-y divide-gray-100 dark:divide-gray-700`} aria-label="Gateway nodes">
            {sorted.length ? sorted.map((node) => (
              <GatewayNodeRow key={node.node_pub_key} node={node} lnv1={lnv1} timeWindow={timeWindow} now={now} />
            )) : <p className={`p-5 ${muted}`}>No gateway is registered with this federation.</p>}
          </section>
          {data.unlinked.length > 0 && (
            <section className={`${card} mt-6 p-4 sm:p-5`} aria-labelledby="unlinked-heading">
              <h2 id="unlinked-heading" className="font-semibold">Registrations without a node key</h2>
              <p className={`mt-1 text-xs ${muted}`}>Lightning v2 endpoints that haven't reported routing info yet, and registrations whose node key isn't a valid public key.</p>
              <ul className="divide-y divide-gray-100 dark:divide-gray-700">
                {data.unlinked.map((registration) => (
                  <GatewayRegistrationItem key={registrationKey(registration)} registration={registration} lnv1={lnv1} timeWindow={timeWindow} now={now} />
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  );
}
