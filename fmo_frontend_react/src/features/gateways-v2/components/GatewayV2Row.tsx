import type { ReactNode } from 'react';
import type { GatewayWindow } from '../../../types/api';
import { asBitcoin, shortMiddle } from '../../../utils/format';
import { Copyable } from '../../../components/ui/Copyable';
import { muted } from '../../../components/ui/styles';
import { formatFee, gatewayName, observedDate, STATUS_COLORS, type GatewayV2 } from '../lib/gatewaysV2';

function Attribution({ gateway }: { gateway: GatewayV2 }) {
  if (gateway.attribution === 'contested') {
    const keys = gateway.contested_keys === 1 ? 'key is' : `${gateway.contested_keys} keys are`;
    return <p className="mt-2 text-amber-700 dark:text-amber-300">Contested: its module {keys} also claimed by another gateway, so that activity is credited to neither.</p>;
  }
  if (gateway.attribution === 'self_reported') return <p className={`mt-2 ${muted}`}>Attributed by a self-reported key (unverified).</p>;
  return null;
}

/** One check per bucket of the window, oldest to newest, colored by how many succeeded */
function ProbeStrip({ gateway, timeWindow }: { gateway: GatewayV2; timeWindow: GatewayWindow }) {
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

function DetailItem({ label, children }: { label: string; children: ReactNode }) {
  return <div><p className={muted}>{label}</p><p className="mt-1">{children}</p></div>;
}

function CopyItem({ label, text }: { label: string; text: string | undefined }) {
  return <div><p className={`mb-1 ${muted}`}>{label}</p>{text !== undefined ? <Copyable text={text} /> : <p>Awaiting node details</p>}</div>;
}

/** Keys, endpoint, registration history and fee details, collapsed until opened */
function GatewayV2Details({ gateway }: { gateway: GatewayV2 }) {
  const routing = gateway.routing_info;
  const activity = gateway.activity;
  return (
    <details className="mt-4 border-t border-gray-100 pt-3 dark:border-gray-700">
      <summary className="cursor-pointer text-xs font-medium text-blue-600 dark:text-blue-400">Gateway details</summary>
      <div className="mt-4 grid gap-4 text-xs sm:grid-cols-2">
        <div><p className={`mb-1 ${muted}`}>Endpoint</p><Copyable text={gateway.api_endpoint} /></div>
        <CopyItem label="Lightning node public key" text={routing?.lightning_public_key} />
        <CopyItem label="Federation module key" text={routing?.module_public_key} />
        <DetailItem label="Last reported by guardians">{gateway.announcing_guardians.map((id) => `#${id}`).join(', ')}</DetailItem>
        <DetailItem label="First seen / last registered">{observedDate(gateway.first_seen)} / {observedDate(gateway.last_seen)}</DetailItem>
        <DetailItem label="Node details observed">{observedDate(gateway.routing_observed_at)}</DetailItem>
        {routing && (
          <>
            <DetailItem label="Minimum send fee / expiry delta">{formatFee(routing.send_fee_minimum)} / {routing.expiration_delta_minimum} blocks</DetailItem>
            <DetailItem label="Funded contract volume">Incoming {asBitcoin(activity.incoming_msat, 8)} · Outgoing {asBitcoin(activity.outgoing_msat, 8)}</DetailItem>
          </>
        )}
      </div>
      <p className={`mt-4 text-xs ${muted}`}>Activity counts contracts funded in this window and their observed outcomes, matched by the module key this gateway reports about itself. The observer cannot verify that the gateway owns that key; a key another gateway also claims is credited to neither. A successful API probe does not verify Lightning liquidity.</p>
    </details>
  );
}

/** A gateway's identity, availability, fees and contract activity */
export function GatewayV2Row({ gateway, timeWindow }: { gateway: GatewayV2; timeWindow: GatewayWindow }) {
  const routing = gateway.routing_info;
  const activity = gateway.activity;
  return (
    <article className="p-4 sm:p-5">
      <div className="grid gap-4 lg:grid-cols-4">
        <div>
          <h3 className="truncate font-semibold" title={gatewayName(gateway)}>{gatewayName(gateway)}</h3>
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
      <GatewayV2Details gateway={gateway} />
    </article>
  );
}
