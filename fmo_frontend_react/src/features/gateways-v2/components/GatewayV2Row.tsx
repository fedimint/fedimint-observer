import type { GatewayWindow } from '../../../types/api';
import { shortMiddle } from '../../../utils/format';
import { muted } from '../../../components/ui/styles';
import { formatFee, gatewayName, observedDate, STATUS_COLORS, type GatewayV2 } from '../lib/gatewaysV2';

export function Attribution({ gateway }: { gateway: GatewayV2 }) {
  if (gateway.attribution === 'contested') {
    const keys = gateway.contested_keys === 1 ? 'key is' : `${gateway.contested_keys} keys are`;
    return <p className="mt-2 text-amber-700 dark:text-amber-300">Contested: its module {keys} also claimed by another gateway, so that activity is credited to neither.</p>;
  }
  if (gateway.attribution === 'self_reported') return <p className={`mt-2 ${muted}`}>Attributed by a self-reported key (unverified).</p>;
  return null;
}

/** One check per bucket of the window, oldest to newest, colored by how many succeeded */
export function ProbeStrip({ gateway, timeWindow }: { gateway: GatewayV2; timeWindow: GatewayWindow }) {
  return (
    <div className="mt-2 flex gap-0.5" role="img" aria-label={`${timeWindow} availability: ${gateway.online_samples} successful of ${gateway.samples} probes`}>
      {gateway.availability.map((bucket, index) => (
        <span
          key={index}
          className={`h-5 min-w-0 flex-1 rounded-xs ${!bucket.samples ? 'bg-gray-200 dark:bg-gray-600' : bucket.online_samples === bucket.samples ? 'bg-green-500' : bucket.online_samples ? 'bg-yellow-500' : 'bg-red-500'}`}
          title={`${observedDate(bucket.start)}: ${bucket.online_samples}/${bucket.samples} checks succeeded`}
        />
      ))}
    </div>
  );
}

/** A gateway's identity, availability, fees and contract activity */
export function GatewayV2Row({ gateway, timeWindow, onSelect }: { gateway: GatewayV2; timeWindow: GatewayWindow; onSelect: () => void }) {
  const routing = gateway.routing_info;
  const activity = gateway.activity;
  return (
    <article onClick={onSelect} className="cursor-pointer p-4 transition-colors hover:bg-surface-2 sm:p-5">
      <div className="grid gap-4 lg:grid-cols-4">
        <div>
          <h3 className="truncate font-semibold" title={gatewayName(gateway)}>
            {/* The card opens the details too; the button makes that reachable by keyboard */}
            <button type="button" onClick={(event) => { event.stopPropagation(); onSelect(); }} className="max-w-full truncate text-left hover:underline" aria-label={`Details for ${gatewayName(gateway)}`}>
              {gatewayName(gateway)}
            </button>
          </h3>
          <p className={`mt-1 text-xs ${muted}`}>{gateway.api_endpoint.startsWith('iroh:') ? 'Iroh' : 'HTTPS'} gateway</p>
          <p className={`mt-2 break-all font-mono text-xs ${muted}`} title={routing?.lightning_public_key}>Node: {routing ? shortMiddle(routing.lightning_public_key, 10, 8) : 'Awaiting node details'}</p>
        </div>
        <div>
          <div className="flex items-center justify-between gap-2">
            <span className={`rounded-full px-2.5 py-1 text-xs font-semibold capitalize ${STATUS_COLORS[gateway.status]}`}>{gateway.status}</span>
            <span className={`text-xs ${muted}`}>{gateway.latency_ms === null ? '—' : `${gateway.latency_ms} ms`}</span>
          </div>
          <p className="mt-2 text-sm font-semibold">{gateway.samples ? `${(gateway.online_samples / gateway.samples * 100).toFixed(1)}% successful checks` : 'No probe history'}</p>
          <ProbeStrip gateway={gateway} timeWindow={timeWindow} />
          <p className={`mt-1 text-xs ${muted}`}>{gateway.samples.toLocaleString()} probes · oldest → newest</p>
        </div>
        <div className="text-xs">
          <p className={`mb-2 ${muted}`}>Routing fees</p>
          <p>Send: <span className="font-medium">{formatFee(routing?.send_fee_default)}</span></p>
          <p className="mt-2">Receive: <span className="font-medium">{formatFee(routing?.receive_fee)}</span></p>
          <p className={`mt-2 ${muted}`}>Probed {observedDate(gateway.probed_at)}</p>
        </div>
        <div className="text-xs">
          <p className={`mb-2 ${muted}`}>Contract activity ({timeWindow.toUpperCase()})</p>
          {routing ? (
            <>
              <p><span className="font-semibold">{activity.incoming_contracts.toLocaleString()}</span> incoming · <span className="font-semibold">{activity.outgoing_contracts.toLocaleString()}</span> outgoing</p>
              <p className={`mt-2 ${muted}`}>{activity.outgoing_claimed} claimed · {activity.outgoing_refunded} refunded · {activity.outgoing_cancelled} cancelled</p>
              <Attribution gateway={gateway} />
            </>
          ) : (
            <p className={muted}>Waiting for gateway identity to attribute activity.</p>
          )}
        </div>
      </div>
    </article>
  );
}
