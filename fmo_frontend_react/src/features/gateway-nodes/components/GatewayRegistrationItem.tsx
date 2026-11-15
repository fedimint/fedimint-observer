import type { ReactNode } from 'react';
import type { GatewayInfo, GatewayRegistration, GatewayWindow } from '../../../types/api';
import { asBitcoin } from '../../../utils/format';
import { muted } from '../../../components/ui/styles';
import { Copyable } from '../../../components/ui/Copyable';
import { getGatewayStatus } from '../../gateways/lib/gatewayRows';
import { observedDate, STATUS_COLORS } from '../../gateways-v2/lib/gatewaysV2';
import { formatReachability, lnv1Numbers } from '../lib/gatewayNodes';

const LNV1_STATUS_COLORS = { ...STATUS_COLORS, retired: STATUS_COLORS.removed };

function ProtocolBadge({ protocol }: { protocol: GatewayRegistration['protocol'] }) {
  return (
    <span className="rounded-full bg-blue-100 px-2 py-0.5 text-xs font-semibold text-blue-700 dark:bg-blue-900/40 dark:text-blue-300">
      {protocol === 'lnv1' ? 'Lightning v1' : 'Lightning v2'}
    </span>
  );
}

function StatusChip({ status }: { status: keyof typeof LNV1_STATUS_COLORS }) {
  return <span className={`rounded-full px-2 py-0.5 text-xs font-semibold capitalize ${LNV1_STATUS_COLORS[status]}`}>{status}</span>;
}

export function UnverifiedBadge() {
  return (
    <span
      className="rounded-full bg-amber-100 px-2 py-0.5 text-xs font-semibold text-amber-800 dark:bg-amber-900/40 dark:text-amber-300"
      title="This endpoint reports the node key itself, and no Lightning v1 registration of the node announces it. Its numbers are shown here only and left out of the node's totals."
    >
      Unverified
    </span>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <div><p className={muted}>{label}</p><p className="mt-1">{children}</p></div>;
}

/** One Lightning v1 or v2 registration with its own numbers for the window */
export function GatewayRegistrationItem({ registration, lnv1, timeWindow, now }: {
  registration: GatewayRegistration;
  lnv1: Map<string, GatewayInfo>;
  timeWindow: GatewayWindow;
  now: number;
}) {
  const window = timeWindow.toUpperCase();
  return (
    <li className="py-3 text-xs">
      <div className="mb-2 flex flex-wrap items-center gap-2">
        <ProtocolBadge protocol={registration.protocol} />
        {registration.protocol === 'lnv1' ? (
          <StatusChip status={getGatewayStatus(new Date(registration.last_seen), now)} />
        ) : (
          <>
            <StatusChip status={registration.status} />
            <span className={muted}>Module instance {registration.module_instance_id}</span>
            {!registration.url_matches_lnv1 && <UnverifiedBadge />}
          </>
        )}
      </div>
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <div className="sm:col-span-2"><p className={`mb-1 ${muted}`}>Endpoint</p><Copyable text={registration.api_endpoint} /></div>
        <Field label="First seen / last registered">{observedDate(registration.first_seen)} / {observedDate(registration.last_seen)}</Field>
        {registration.protocol === 'lnv1' ? (
          <Lnv1Fields registration={registration} lnv1={lnv1} window={window} />
        ) : (
          <>
            <Field label={`Reachability (${window})`}>
              {formatReachability(registration.samples, registration.online_samples)}
            </Field>
            <Field label={`Contract activity (${window})`}>
              {registration.activity.incoming_contracts.toLocaleString()} incoming · {registration.activity.outgoing_contracts.toLocaleString()} outgoing
              {' · '}{asBitcoin(registration.activity.incoming_msat + registration.activity.outgoing_msat, 8)}
              {registration.attribution === 'contested' && <span className="block text-amber-700 dark:text-amber-300">Contested module key: credited to no endpoint.</span>}
              {registration.attribution === 'self_reported' && <span className={`block ${muted}`}>Attributed by a self-reported key (unverified).</span>}
            </Field>
          </>
        )}
      </div>
    </li>
  );
}

function Lnv1Fields({ registration, lnv1, window }: {
  registration: Extract<GatewayRegistration, { protocol: 'lnv1' }>;
  lnv1: Map<string, GatewayInfo>;
  window: string;
}) {
  const { uptimePct, coveragePct, activity } = lnv1Numbers(registration, lnv1);
  return (
    <>
      <Field label={`Registry uptime (${window})`}>{uptimePct === null ? 'No poll history' : `${uptimePct.toFixed(1)}% listed by guardians (${(coveragePct ?? 0).toFixed(0)}% observed)`}</Field>
      <Field label={`Contract activity (${window})`}>
        {activity
          ? <>{activity.fund_count.toLocaleString()} funded · {activity.settle_count.toLocaleString()} settled · {asBitcoin(activity.total_volume_msat, 8)}</>
          : 'Not available'}
      </Field>
    </>
  );
}
