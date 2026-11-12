import type { GatewayV2AvailabilityBucket, GatewayWindow } from '../../../types/api';
import { card, muted } from '../../../components/ui/styles';
import { GatewayAvailabilityChart } from './GatewayAvailabilityChart';

/** Probes of every gateway of the module instance, bucketed on the server */
export function GatewayV2AvailabilityTrend({ trend, timeWindow }: { trend: GatewayV2AvailabilityBucket[]; timeWindow: GatewayWindow }) {
  const hasSamples = trend.some((bucket) => bucket.samples > 0);
  return (
    <section className={`${card} mb-4 p-4 sm:p-5`}>
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-base font-semibold">Gateway availability trend ({timeWindow.toUpperCase()})</h2>
        <span className={`text-xs ${muted}`}>Historical API checks · gaps mean no observation · all times in UTC</span>
      </div>
      {hasSamples ? (
        <GatewayAvailabilityChart buckets={trend} />
      ) : (
        <div className={`flex h-[210px] flex-col items-center justify-center gap-2 text-center text-sm ${muted}`}>
          <span>No probe history yet</span>
          <span className="text-xs">Availability will appear as gateway checks are recorded.</span>
        </div>
      )}
    </section>
  );
}
