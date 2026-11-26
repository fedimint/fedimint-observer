import type { ReactNode, Ref } from 'react';
import type { GatewayOverviewSummary, GatewayWindow } from '../../../types/api';
import { Segmented } from '../../../components/ui/Segmented';
import { StatusDot } from '../../../components/ui/Status';
import { GATEWAY_WINDOWS } from '../lib/gatewayRows';

const WINDOW_OPTIONS = GATEWAY_WINDOWS.map((value) => ({ value, label: value }));

interface GatewayPageHeaderProps {

  headingRef: Ref<HTMLHeadingElement>;
  timeWindow: GatewayWindow;
  onSelectWindow: (window: GatewayWindow) => void;

  windowStatus: string;

  children?: ReactNode;
}

/** The section title, the time window buttons and the gateway counts */
export function GatewayPageHeader({ headingRef, timeWindow, onSelectWindow, windowStatus, children }: GatewayPageHeaderProps) {
  return (
    <header className="mb-4 rounded-xl border border-line bg-surface p-4 shadow-[0_1px_2px_rgba(10,17,34,0.04)] sm:p-5">
      <div className="flex flex-col justify-between gap-3 sm:flex-row sm:items-start">
        <div>
          <h2 ref={headingRef} tabIndex={-1} className="text-base font-medium text-ink outline-hidden">Lightning gateways</h2>
          <p className="mt-0.5 text-xs text-ink-3 sm:text-sm">Gateways registered with the federation's Lightning module, as the observer polls them</p>
        </div>
        <div className="flex shrink-0 flex-col items-start gap-1 sm:items-end">
          <Segmented label="Time window" options={WINDOW_OPTIONS} value={timeWindow} onChange={onSelectWindow} />
          <span className="text-xs text-ink-3">{windowStatus}</span>
        </div>
      </div>
      {children}
    </header>
  );
}

interface GatewaySummaryBarProps {

  summary: GatewayOverviewSummary | null;

  liveCount: number;

  switchingWindow: boolean;
}

/** Gateways by status, vetted count and average uptime */
export function GatewaySummaryBar({ summary, liveCount, switchingWindow }: GatewaySummaryBarProps) {
  return (
    <div aria-busy={switchingWindow} className={`mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-line pt-3 text-sm transition-opacity ${switchingWindow ? 'opacity-60' : ''}`}>
      {summary ? (
        <>
          <span className="font-semibold text-ink">{summary.total} gateways</span>
          <span className="inline-flex items-center gap-1.5 text-ink-2"><StatusDot tone="ok" />{summary.online} online</span>
          <span className="inline-flex items-center gap-1.5 text-ink-2"><StatusDot tone="warn" />{summary.degraded} degraded</span>
          <span className="inline-flex items-center gap-1.5 text-ink-2"><StatusDot tone="crit" />{summary.offline} offline</span>
          {summary.unknown > 0 && <span className="inline-flex items-center gap-1.5 text-ink-2"><StatusDot tone="neutral" />{summary.unknown} unknown</span>}
          {summary.retired > 0 && <span className="text-ink-3">{summary.retired} retired</span>}
          <span className="text-ink-2">{summary.vetted} vetted</span>
          <span className="sm:ml-auto font-semibold text-ink">
            {summary.avg_uptime_pct !== null
              ? <>{summary.avg_uptime_pct.toFixed(1)}% uptime <span className="font-normal text-xs text-ink-3">({(summary.avg_coverage_pct ?? 0).toFixed(0)}% observed)</span></>
              : 'No uptime data'}
          </span>
        </>
      ) : (
        <span className="text-ink-2">{liveCount} gateways from the live registry · no recorded history</span>
      )}
    </div>
  );
}
