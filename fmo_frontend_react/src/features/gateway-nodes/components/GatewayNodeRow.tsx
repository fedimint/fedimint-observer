import type { GatewayInfo, GatewayNode, GatewayWindow } from '../../../types/api';
import { asBitcoin } from '../../../utils/format';
import { muted } from '../../../components/ui/styles';
import { Copyable } from '../../../components/ui/Copyable';
import { formatReachability, hasUnverified, lnv1Numbers, lnv1Registrations, lnv2Registrations, nodeName, registrationKey } from '../lib/gatewayNodes';
import { GatewayRegistrationItem, UnverifiedBadge } from './GatewayRegistrationItem';

/**
 * One Lightning node: its Lightning v1 numbers beside its verified Lightning
 * v2 numbers, never added together, and every registration underneath.
 */
export function GatewayNodeRow({ node, lnv1, timeWindow, now }: {
  node: GatewayNode;
  lnv1: Map<string, GatewayInfo>;
  timeWindow: GatewayWindow;
  now: number;
}) {
  const window = timeWindow.toUpperCase();
  const v1 = lnv1Registrations(node);
  const v2 = lnv2Registrations(node);
  const v1Numbers = v1.map((registration) => lnv1Numbers(registration, lnv1));
  const totals = node.totals;

  return (
    <article className="p-4 sm:p-5">
      <div className="grid gap-4 lg:grid-cols-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="truncate font-semibold" title={nodeName(node, lnv1)}>{nodeName(node, lnv1)}</h3>
            {hasUnverified(node) && <UnverifiedBadge />}
          </div>
          <p className={`mt-1 text-xs ${muted}`}>
            {node.registrations.length} registration{node.registrations.length === 1 ? '' : 's'}: {v1.length} Lightning v1 · {v2.length} Lightning v2
          </p>
          <div className="mt-2 text-xs"><Copyable text={node.node_pub_key} /></div>
        </div>
        <div className="text-xs">
          <p className={`mb-1 ${muted}`}>Lightning v1 registry uptime ({window})</p>
          {v1.length ? (
            <ul>
              {v1Numbers.map(({ uptimePct, coveragePct }, index) => (
                <li key={v1[index].gateway_id} className="font-semibold">
                  {uptimePct === null ? 'No poll history' : <>{uptimePct.toFixed(1)}% <span className={`font-normal ${muted}`}>({(coveragePct ?? 0).toFixed(0)}% observed)</span></>}
                </li>
              ))}
            </ul>
          ) : <p className={muted}>No Lightning v1 registration</p>}
          <p className={`mb-1 mt-3 ${muted}`}>Lightning v2 reachability, verified ({window})</p>
          {totals ? <p className="font-semibold">{formatReachability(totals.samples, totals.online_samples)}</p> : <p className={muted}>No verified Lightning v2 registration</p>}
        </div>
        <div className="text-xs">
          <p className={`mb-1 ${muted}`}>Lightning v1 activity ({window})</p>
          {v1Numbers.some(({ activity }) => activity) ? (
            <ul>
              {v1Numbers.map(({ activity }, index) => (
                <li key={v1[index].gateway_id}>
                  {activity ? <><span className="font-semibold">{activity.fund_count.toLocaleString()}</span> funded · {asBitcoin(activity.total_volume_msat, 8)}</> : 'Not available'}
                </li>
              ))}
            </ul>
          ) : <p className={muted}>{v1.length ? 'Not available' : '—'}</p>}
          <p className={`mb-1 mt-3 ${muted}`}>Lightning v2 activity, verified ({window})</p>
          {totals ? (
            <p>
              <span className="font-semibold">{totals.activity.incoming_contracts.toLocaleString()}</span> incoming · <span className="font-semibold">{totals.activity.outgoing_contracts.toLocaleString()}</span> outgoing · {asBitcoin(totals.activity.incoming_msat + totals.activity.outgoing_msat, 8)}
            </p>
          ) : <p className={muted}>—</p>}
        </div>
      </div>
      <details className="mt-4 border-t border-gray-100 pt-3 dark:border-gray-700">
        <summary className="cursor-pointer text-xs font-medium text-blue-600 dark:text-blue-400">Registrations ({node.registrations.length})</summary>
        <ul className="divide-y divide-gray-100 dark:divide-gray-700">
          {node.registrations.map((registration) => (
            <GatewayRegistrationItem
              key={registrationKey(registration)}
              registration={registration}
              lnv1={lnv1}
              timeWindow={timeWindow}
              now={now}
            />
          ))}
        </ul>
      </details>
    </article>
  );
}
