import type { ReactNode } from 'react';
import type { GatewayWindow } from '../../../types/api';
import { Drawer } from '../../../components/ui/Drawer';
import { KeyChip } from '../../../components/ui/KeyChip';
import { asBitcoin } from '../../../utils/format';
import { formatFee, gatewayName, observedDate, STATUS_COLORS, type GatewayV2 } from '../lib/gatewaysV2';
import { Attribution, ProbeStrip } from './GatewayV2Row';

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="rounded-xl border border-line bg-surface p-4">
      <h3 className="mb-3 text-sm font-semibold text-ink">{title}</h3>
      {children}
    </section>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[9.5rem_minmax(0,1fr)] items-start gap-3 py-1.5 text-xs">
      <dt className="text-ink-3">{label}</dt>
      <dd className="min-w-0 text-ink">{children}</dd>
    </div>
  );
}

const awaiting = <span className="text-ink-3">Awaiting node details</span>;

/** Everything the observer knows about one Lightning v2 gateway, in the side panel */
export function GatewayV2Detail({ gateway, timeWindow, onClose }: { gateway: GatewayV2; timeWindow: GatewayWindow; onClose: () => void }) {
  const routing = gateway.routing_info;
  const activity = gateway.activity;
  return (
    <Drawer
      title={gatewayName(gateway)}
      subtitle={
        <span className="inline-flex items-center gap-2">
          <span className={`rounded-full px-2 py-0.5 text-[0.68rem] font-semibold capitalize ${STATUS_COLORS[gateway.status]}`}>{gateway.status}</span>
          Lightning v2 · {gateway.api_endpoint.startsWith('iroh:') ? 'Iroh' : 'HTTPS'} gateway
        </span>
      }
      onClose={onClose}
    >
      <Section title="Identity">
        <dl className="divide-y divide-line">
          <Field label="API endpoint"><span className="select-all break-all font-mono text-[0.72rem] text-ink-2">{gateway.api_endpoint}</span></Field>
          <Field label="Lightning node">{routing ? <KeyChip value={routing.lightning_public_key} label="Lightning node public key" head={10} tail={8} /> : awaiting}</Field>
          <Field label="Federation module key">{routing ? <KeyChip value={routing.module_public_key} label="federation module key" head={10} tail={8} /> : awaiting}</Field>
          <Field label="Reported by guardians">{gateway.announcing_guardians.map((id) => `#${id}`).join(', ') || '—'}</Field>
          <Field label="First seen">{observedDate(gateway.first_seen)}</Field>
          <Field label="Last registered">{observedDate(gateway.last_seen)}</Field>
        </dl>
      </Section>

      <Section title={`Availability (${timeWindow.toUpperCase()})`}>
        <p className="text-sm font-semibold text-ink">
          {gateway.samples ? `${(gateway.online_samples / gateway.samples * 100).toFixed(1)}% successful checks` : 'No probe history'}
        </p>
        <ProbeStrip gateway={gateway} timeWindow={timeWindow} />
        <dl className="mt-3 divide-y divide-line">
          <Field label="Probes"><span className="tabular">{gateway.online_samples.toLocaleString()} of {gateway.samples.toLocaleString()} succeeded</span></Field>
          <Field label="Latest response time">{gateway.latency_ms === null ? '—' : <span className="tabular">{gateway.latency_ms} ms</span>}</Field>
          <Field label="Last probed">{observedDate(gateway.probed_at)}</Field>
        </dl>
      </Section>

      <Section title="Routing fees">
        <dl className="divide-y divide-line">
          <Field label="Send fee">{formatFee(routing?.send_fee_default)}</Field>
          <Field label="Receive fee">{formatFee(routing?.receive_fee)}</Field>
          {routing && <Field label="Minimum send fee">{formatFee(routing.send_fee_minimum)}</Field>}
          {routing && <Field label="Minimum expiry delta">{routing.expiration_delta_minimum} blocks</Field>}
          <Field label="Node details observed">{observedDate(gateway.routing_observed_at)}</Field>
        </dl>
      </Section>

      <Section title={`Contract activity (${timeWindow.toUpperCase()})`}>
        {routing ? (
          <>
            <dl className="divide-y divide-line">
              <Field label="Incoming contracts"><span className="tabular">{activity.incoming_contracts.toLocaleString()}</span></Field>
              <Field label="Outgoing contracts"><span className="tabular">{activity.outgoing_contracts.toLocaleString()}</span></Field>
              <Field label="Outgoing outcomes">{activity.outgoing_claimed} claimed · {activity.outgoing_refunded} refunded · {activity.outgoing_cancelled} cancelled</Field>
              <Field label="Funded volume">Incoming {asBitcoin(activity.incoming_msat, 8)} · Outgoing {asBitcoin(activity.outgoing_msat, 8)}</Field>
            </dl>
            <div className="text-xs"><Attribution gateway={gateway} /></div>
          </>
        ) : (
          <p className="text-xs text-ink-3">Waiting for the gateway's identity to attribute activity.</p>
        )}
        <p className="mt-3 text-[0.7rem] leading-relaxed text-ink-3">
          Activity is matched by the module key this gateway reports about itself, which the observer cannot verify; a key another gateway also claims is credited to neither. A successful API probe does not verify Lightning liquidity.
        </p>
      </Section>
    </Drawer>
  );
}
