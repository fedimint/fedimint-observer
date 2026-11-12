import type { ReactNode, Ref } from 'react';
import type { GatewayOverviewSummary, GatewayWindow } from '../../../types/api';
import { GATEWAY_WINDOWS } from '../lib/gatewayRows';

interface GatewayPageHeaderProps {
  /** Takes keyboard focus after a successful retry removes the focused button */
  headingRef: Ref<HTMLHeadingElement>;
  timeWindow: GatewayWindow;
  onSelectWindow: (window: GatewayWindow) => void;
  /** Loading, freshness or fallback note under the window buttons */
  windowStatus: string;
  /** The summary line, once there is data */
  children?: ReactNode;
}

/** The section title, the time window buttons and the gateway counts */
export function GatewayPageHeader({ headingRef, timeWindow, onSelectWindow, windowStatus, children }: GatewayPageHeaderProps) {
  return (
    <header className="mb-4 rounded-2xl border border-slate-200 bg-linear-to-br from-white to-blue-50 p-5 shadow-xs dark:border-gray-700 dark:from-gray-800 dark:to-blue-950/30 sm:p-6">
      <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
        <div>
          <h2 ref={headingRef} tabIndex={-1} className="text-lg font-semibold text-gray-950 outline-hidden dark:text-white">Lightning gateways</h2>
          <p className="mt-1 text-xs text-gray-600 dark:text-gray-400">Gateways registered with the federation's Lightning module, as the observer polls them</p>
        </div>
        <div className="rounded-xl bg-slate-100 p-1 dark:bg-gray-900">
          <div className="flex gap-1" role="group" aria-label="Time window">
            {GATEWAY_WINDOWS.map((option) => (
              <button
                key={option}
                type="button"
                aria-pressed={timeWindow === option}
                onClick={() => onSelectWindow(option)}
                className={`rounded-lg px-3 py-2 text-sm font-semibold ${timeWindow === option ? 'bg-white text-blue-700 shadow-xs dark:bg-gray-700 dark:text-blue-300' : 'text-gray-600 dark:text-gray-400'}`}
              >
                {option.toUpperCase()}
              </button>
            ))}
          </div>
          <div className="px-2 pt-1 text-right text-[11px] text-gray-600 dark:text-gray-400">{windowStatus}</div>
        </div>
      </div>
      {children}
    </header>
  );
}

interface GatewaySummaryBarProps {
  /** Counted on the server over all recorded gateways; null for live registry rows alone */
  summary: GatewayOverviewSummary | null;
  /** How many gateways the live registry listed, when there is no summary */
  liveCount: number;
  /** Dimmed while another window loads */
  switchingWindow: boolean;
}

/** Gateways by status, vetted count and average uptime */
export function GatewaySummaryBar({ summary, liveCount, switchingWindow }: GatewaySummaryBarProps) {
  return (
    <div aria-busy={switchingWindow} className={`mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-slate-200 pt-3 text-sm transition-opacity dark:border-gray-700 ${switchingWindow ? 'opacity-60' : ''}`}>
      {summary ? (
        <>
          <span className="font-semibold text-gray-950 dark:text-white">{summary.total} gateways</span>
          <span className="text-green-700 dark:text-green-300">● {summary.online} online</span>
          <span className="text-yellow-700 dark:text-yellow-300">● {summary.degraded} degraded</span>
          <span className="text-red-700 dark:text-red-300">● {summary.offline} offline</span>
          {summary.unknown > 0 && <span className="text-gray-600 dark:text-gray-300">● {summary.unknown} unknown</span>}
          {summary.retired > 0 && <span className="text-gray-500 dark:text-gray-400">{summary.retired} retired</span>}
          <span className="text-gray-600 dark:text-gray-300">{summary.vetted} vetted</span>
          <span className="sm:ml-auto font-semibold text-indigo-700 dark:text-indigo-300">
            {summary.avg_uptime_pct !== null
              ? <>{summary.avg_uptime_pct.toFixed(1)}% uptime <span className="font-normal text-xs text-gray-500 dark:text-gray-400">({(summary.avg_coverage_pct ?? 0).toFixed(0)}% observed)</span></>
              : 'No uptime data'}
          </span>
        </>
      ) : (
        <span className="text-gray-600 dark:text-gray-300">{liveCount} gateways from the live registry · no recorded history</span>
      )}
    </div>
  );
}
