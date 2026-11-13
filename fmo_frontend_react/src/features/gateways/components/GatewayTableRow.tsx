import type { GatewayWindow } from '../../../types/api';
import { formatMsatsCompact, shortMiddle } from '../../../utils/format';
import { formatUtcDateTime } from '../../../utils/time';
import { formatCompactDuration, formatRelative, statusClasses, type GatewayWithStatus, type UptimeStripStatus } from '../lib/gatewayRows';
import { UptimeStrip } from './UptimeStrip';

interface GatewayTableRowProps {
  gateway: GatewayWithStatus;
  strip: UptimeStripStatus[];
  now: number;
  dataWindow: GatewayWindow;
  windowMinutes: number;
  /** The window the activity figures cover, e.g. "7D" */
  metricsWindow: string;
  onViewRaw: (gatewayName: string, raw: Record<string, unknown>) => void;
}

/** Labels each cell on phones, where the row is a stacked card without column headings */
function MobileLabel({ children }: { children: string }) {
  return <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-gray-500 dark:text-gray-400 sm:hidden">{children}</div>;
}

const cell = 'block px-4 py-2 sm:table-cell sm:px-5 sm:py-4';

export function GatewayTableRow({ gateway, strip, now, dataWindow, windowMinutes, metricsWindow, onViewRaw }: GatewayTableRowProps) {
  return (
    <tr className="block py-2 align-top border-b border-gray-200 bg-white transition-colors last:border-b-0 hover:bg-blue-50/40 dark:border-gray-700 dark:bg-gray-800 dark:hover:bg-blue-950/20 sm:table-row sm:py-0">
      <td className={cell}>
        <div className="truncate font-semibold text-gray-950 dark:text-white" title={gateway.lightning_alias || 'Unnamed Gateway'}>
          {gateway.lightning_alias || 'Unnamed Gateway'}
        </div>
        <div className="text-xs font-mono text-gray-600 dark:text-gray-400 mt-1" title={gateway.gateway_id}>
          {shortMiddle(gateway.gateway_id, 8, 8, '...')}
        </div>
        <div className="text-xs font-mono text-gray-500 dark:text-gray-500 mt-1" title={gateway.node_pub_key}>
          Node: {shortMiddle(gateway.node_pub_key, 8, 8, '...')}
        </div>
      </td>
      <td className={cell}>
        <div className="flex items-center justify-between gap-3">
          <span className={`rounded-full px-2.5 py-1 text-xs font-semibold ${statusClasses(gateway.status)}`}>
            {gateway.status}
          </span>
          <span className="whitespace-nowrap text-xs text-gray-500 dark:text-gray-400">{gateway.lastSeenDate ? `seen ${formatRelative(gateway.lastSeenDate, now)}` : 'never seen'}</span>
        </div>
        <div className="mt-2 flex items-baseline justify-between gap-3">
          <span className="font-semibold text-gray-950 dark:text-white">{gateway.coveragePct > 0 ? `${gateway.estimatedUptimePct.toFixed(1)}% uptime` : 'No samples'}</span>
          <span className="text-xs text-gray-500 dark:text-gray-400">{gateway.coveragePct.toFixed(0)}% observed</span>
        </div>
        <UptimeStrip gateway={gateway} strip={strip} dataWindow={dataWindow} windowMinutes={windowMinutes} />
        <div className="mt-2 flex flex-wrap gap-x-2 text-xs text-gray-500 dark:text-gray-400">
          <span>On {formatCompactDuration(gateway.estimatedOnlineMinutes)}</span>
          <span>Off {formatCompactDuration(gateway.estimatedOfflineMinutes)}</span>
          <span>Unknown {formatCompactDuration(gateway.estimatedUnknownMinutes)}</span>
        </div>
      </td>
      <td className={`${cell} text-gray-700 dark:text-gray-300`}>
        <MobileLabel>Activity</MobileLabel>
        {gateway.realActivityScore !== null ? (
          <>
            <div className="font-semibold text-gray-950 dark:text-white">
              {gateway.realActivityScore.toLocaleString()} <span className="cursor-help text-xs font-normal text-gray-500 dark:text-gray-400" title="Activity score: funds + 3 × settles − 1.5 × cancels, plus a small bonus for volume">score</span>
            </div>
            <div className="mt-1 text-xs leading-5 text-gray-500 dark:text-gray-400">
              {metricsWindow} · {gateway.fundCountWindow} funds · {gateway.settleCountWindow} settles · {gateway.cancelCountWindow} cancels<br />
              {formatMsatsCompact(gateway.totalVolumeMsatWindow)} volume
            </div>
          </>
        ) : (
          <div className="text-xs text-gray-500 dark:text-gray-400">
            No activity recorded in {metricsWindow}
          </div>
        )}
      </td>
      <td className={cell}>
        <MobileLabel>Trust & history</MobileLabel>
        <span className={`text-sm font-medium ${gateway.vetted ? 'text-green-600 dark:text-green-400' : 'text-gray-500 dark:text-gray-400'}`}>
          {gateway.vetted ? '✓ Vetted' : 'Not vetted'}
        </span>
        <div className="mt-2 text-xs text-gray-500 dark:text-gray-400">First seen<br /><span className="text-gray-700 dark:text-gray-300">{formatUtcDateTime(gateway.firstSeenDate)}</span></div>
        <div className="mt-2 text-xs text-gray-500 dark:text-gray-400">Last seen<br /><span className="text-gray-700 dark:text-gray-300">{formatUtcDateTime(gateway.lastSeenDate)}</span></div>
      </td>
      <td className={cell}>
        <MobileLabel>Endpoint</MobileLabel>
        {/* Gateway APIs (iroh:// or /v1 endpoints) are not pages, so show them as text to copy */}
        <span title={gateway.api_endpoint} className="block select-all truncate font-mono text-xs text-gray-700 dark:text-gray-300">
          {gateway.api_endpoint}
        </span>
        {gateway.raw && (
          <button
            type="button"
            onClick={() => onViewRaw(gateway.lightning_alias || shortMiddle(gateway.gateway_id, 8, 8, '...'), gateway.raw!)}
            className="mt-2 text-xs text-gray-600 hover:text-blue-700 hover:underline dark:text-gray-400 dark:hover:text-blue-300"
          >
            View raw announcement
          </button>
        )}
      </td>
    </tr>
  );
}
