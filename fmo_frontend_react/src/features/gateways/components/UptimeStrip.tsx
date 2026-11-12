import type { GatewayWindow } from '../../../types/api';
import {
  UPTIME_SEGMENTS,
  getUptimeBucketTooltip,
  getUptimeStripClass,
  uptimeBuckets,
  uptimeTooltipPosition,
  type GatewayWithStatus,
  type UptimeStripStatus,
} from '../lib/gatewayRows';

interface UptimeStripProps {
  gateway: GatewayWithStatus;
  strip: UptimeStripStatus[];
  dataWindow: GatewayWindow;
  windowMinutes: number;
}

/** The window in 30 segments, oldest to newest, each colored by the polls seen in it */
export function UptimeStrip({ gateway, strip, dataWindow, windowMinutes }: UptimeStripProps) {
  const buckets = uptimeBuckets(gateway);
  return (
    // One stop for keyboards and screen readers instead of 30 per row
    <div
      className="mt-2.5 flex gap-0.5"
      role="img"
      aria-label={`${dataWindow.toUpperCase()} availability: ${gateway.coveragePct > 0 ? `${gateway.estimatedUptimePct.toFixed(1)}% uptime` : 'no samples'}, ${gateway.coveragePct.toFixed(0)}% observed`}
    >
      {strip.map((status, index) => {
        const tooltip = getUptimeBucketTooltip(index, UPTIME_SEGMENTS, windowMinutes, buckets?.[index]);
        return (
          <span
            key={`${gateway.gateway_id}-${index}`}
            title={tooltip}
            className="group relative h-3.5 flex-1 cursor-help rounded-[3px]"
          >
            <span className={`block h-full rounded-[3px] transition-transform duration-150 group-hover:scale-y-125 ${getUptimeStripClass(status)}`} />
            <span className={`pointer-events-none absolute bottom-full z-20 mb-2 hidden w-max rounded-md bg-gray-950 px-2 py-1 text-[11px] font-medium text-white shadow-lg group-hover:block dark:bg-black ${uptimeTooltipPosition(index)}`}>
              {tooltip}
            </span>
          </span>
        );
      })}
    </div>
  );
}
