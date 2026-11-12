import type { ReactNode } from 'react';
import type { GatewayWindow } from '../../../types/api';
import { Drawer } from '../../../components/ui/Drawer';
import { KeyChip } from '../../../components/ui/KeyChip';
import { formatMsatsCompact, shortMiddle } from '../../../utils/format';
import { formatUtcDateTime } from '../../../utils/time';
import { formatCompactDuration, formatRelative, statusClasses, type GatewayWithStatus, type UptimeStripStatus } from '../lib/gatewayRows';
import { UptimeStrip } from './UptimeStrip';

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
    <div className="grid grid-cols-[8.5rem_minmax(0,1fr)] items-start gap-3 py-1.5 text-xs">
      <dt className="text-ink-3">{label}</dt>
      <dd className="min-w-0 text-ink">{children}</dd>
    </div>
  );
}

interface GatewayDetailProps {
  gateway: GatewayWithStatus;
  strip: UptimeStripStatus[];
  now: number;
  dataWindow: GatewayWindow;
  windowMinutes: number;
  /** The window the activity figures cover, e.g. "7D" */
  metricsWindow: string;
  onClose: () => void;
}

/** Everything the observer knows about one Lightning v1 gateway, in the side panel */
export function GatewayDetail({ gateway, strip, now, dataWindow, windowMinutes, metricsWindow, onClose }: GatewayDetailProps) {
  const name = gateway.lightning_alias || 'Unnamed Gateway';
  return (
    <Drawer
      title={name}
      subtitle={
        <span className="inline-flex items-center gap-2">
          <span className={`rounded-full px-2 py-0.5 text-[0.68rem] font-semibold ${statusClasses(gateway.status)}`}>{gateway.status}</span>
          Lightning v1 gateway
        </span>
      }
      onClose={onClose}
    >
      <Section title="Identity">
        <dl className="divide-y divide-line">
          <Field label="Gateway id"><KeyChip value={gateway.gateway_id} label="gateway id" head={10} tail={8} /></Field>
          <Field label="Lightning node"><KeyChip value={gateway.node_pub_key} label="Lightning node public key" head={10} tail={8} /></Field>
          {/* Gateway APIs (iroh:// or /v1 endpoints) are not pages, so they are text to copy */}
          <Field label="API endpoint"><span className="select-all break-all font-mono text-[0.72rem] text-ink-2">{gateway.api_endpoint}</span></Field>
          <Field label="Vetted">
            {gateway.vetted ? <span className="text-ok-ink">Vetted by the federation</span> : <span className="text-ink-3">Not vetted</span>}
          </Field>
        </dl>
      </Section>

      <Section title={`Availability (${dataWindow.toUpperCase()})`}>
        <div className="flex flex-wrap items-baseline justify-between gap-2 text-xs">
          <span className="text-sm font-semibold text-ink">
            {gateway.coveragePct > 0 ? `${gateway.estimatedUptimePct.toFixed(1)}% uptime` : 'No samples'}
          </span>
          <span className="text-ink-3">{gateway.coveragePct.toFixed(0)}% of the window observed</span>
        </div>
        <UptimeStrip gateway={gateway} strip={strip} dataWindow={dataWindow} windowMinutes={windowMinutes} />
        <dl className="mt-3 divide-y divide-line">
          <Field label="Online">{formatCompactDuration(gateway.estimatedOnlineMinutes)}</Field>
          <Field label="Offline">{formatCompactDuration(gateway.estimatedOfflineMinutes)}</Field>
          <Field label="Unknown">{formatCompactDuration(gateway.estimatedUnknownMinutes)}</Field>
          <Field label="First seen">{formatUtcDateTime(gateway.firstSeenDate)}</Field>
          <Field label="Last seen">
            {gateway.lastSeenDate ? `${formatRelative(gateway.lastSeenDate, now)} · ${formatUtcDateTime(gateway.lastSeenDate)}` : 'Never'}
          </Field>
        </dl>
      </Section>

      <Section title={`Activity (${metricsWindow})`}>
        {gateway.realActivityScore === null ? (
          <p className="text-xs text-ink-3">No activity recorded in {metricsWindow}.</p>
        ) : (
          <dl className="divide-y divide-line">
            <Field label="Activity score">
              <span className="tabular">{gateway.realActivityScore.toLocaleString()}</span>
              <span className="ml-1 text-ink-3">(funds + 3 × settles − 1.5 × cancels, plus a small volume bonus)</span>
            </Field>
            <Field label="Funded contracts"><span className="tabular">{gateway.fundCountWindow.toLocaleString()}</span></Field>
            <Field label="Settled"><span className="tabular">{gateway.settleCountWindow.toLocaleString()}</span></Field>
            <Field label="Cancelled"><span className="tabular">{gateway.cancelCountWindow.toLocaleString()}</span></Field>
            <Field label="Volume"><span className="tabular">{formatMsatsCompact(gateway.totalVolumeMsatWindow)}</span></Field>
          </dl>
        )}
      </Section>

      {gateway.raw && (
        <Section title="Raw announcement">
          <p className="mb-2 text-xs text-ink-3">Exactly as the federation published it for {shortMiddle(gateway.gateway_id, 8, 8, '...')}.</p>
          <pre className="m-0 max-h-96 overflow-auto rounded-lg bg-gray-950 p-3 text-[0.7rem] leading-5 text-gray-100 scrollbar-thin">
            {JSON.stringify(gateway.raw, null, 2)}
          </pre>
        </Section>
      )}
    </Drawer>
  );
}
