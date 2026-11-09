import { useState } from 'react';
import { chartSegments } from '../utils/chartSegments';
import { formatUtcDayHour } from '../utils/time';

import type { GatewayV2AvailabilityBucket } from '../types/api';

export function GatewayAvailabilityChart({ buckets }: { buckets: GatewayV2AvailabilityBucket[] }) {
  const [active, setActive] = useState<number | null>(null);
  const values = buckets.map((bucket) => bucket.samples ? bucket.online_samples / bucket.samples * 100 : null);
  const segments = chartSegments(values);
  const x = (index: number) => index / Math.max(1, buckets.length - 1) * 100;
  // The section names the zone once: all times in UTC
  const label = (index: number) => formatUtcDayHour(buckets[index]?.start);
  const selected = active === null ? null : buckets[active];
  return <div className="h-[210px] pt-3 text-[10px] text-gray-600 dark:text-gray-300">
    <div className="flex h-40 gap-2">
      <div className="flex w-9 shrink-0 flex-col justify-between text-right" aria-hidden="true">{[100, 75, 50, 25, 0].map((value) => <span key={value} className="leading-none">{value}%</span>)}</div>
      <div className="relative mr-2 min-w-0 flex-1" role="img" aria-label="Recorded gateway check success from 0 to 100 percent. Gaps indicate no observations."
        onMouseLeave={() => setActive(null)}>
        {[0, 25, 50, 75, 100].map((value) => <div key={value} className="absolute inset-x-0 border-t border-dashed border-gray-200 dark:border-gray-700" style={{ top: `${value}%` }} />)}
        <svg width="100%" height="100%" viewBox="0 0 100 100" preserveAspectRatio="none" className="overflow-visible" aria-hidden="true">
          {segments.map((segment) => {
            const first = segment[0];
            const last = segment[segment.length - 1];
            const line = segment.map((point, index) => `${index ? 'L' : 'M'}${x(point.index)},${100 - point.value}`).join(' ');
            return <g key={first.index}>
              <path d={`${line} L${x(last.index)},100 L${x(first.index)},100 Z`} fill="rgba(99,102,241,0.10)" />
              <path d={line} fill="none" stroke="#6366f1" strokeWidth="2" vectorEffect="non-scaling-stroke" />
            </g>;
          })}
        </svg>
        {values.map((value, index) => value !== null && <span key={index} aria-hidden="true" className="pointer-events-none absolute h-1.5 w-1.5 -translate-x-1/2 -translate-y-1/2 rounded-full" style={{ backgroundColor: '#6366f1', left: `${x(index)}%`, top: `${100 - value}%` }} />)}
        <div className="absolute inset-0 flex" aria-hidden="true">{buckets.map((bucket, index) => <span key={index} className="h-full flex-1" onMouseEnter={() => setActive(index)}
          title={`${label(index)}: ${bucket.samples ? `${(bucket.online_samples / bucket.samples * 100).toFixed(1)}% (${bucket.online_samples}/${bucket.samples} checks)` : 'No observations'}`} />)}</div>
        {selected && <div aria-hidden="true" className="pointer-events-none absolute left-1/2 top-2 z-10 -translate-x-1/2 whitespace-nowrap rounded-md border border-gray-200 bg-white px-3 py-2 text-xs text-gray-900 shadow-sm dark:border-gray-600 dark:bg-gray-800 dark:text-gray-100">
          {label(active!)}<br />{selected.samples ? `${(selected.online_samples / selected.samples * 100).toFixed(1)}% · ${selected.online_samples}/${selected.samples} checks` : 'No observations'}
        </div>}
      </div>
    </div>
    <div className="ml-11 mt-3 flex justify-between gap-2" aria-hidden="true">{[0, Math.floor((buckets.length - 1) / 2), buckets.length - 1].map((index) => <span key={index} className="truncate">{label(index)}</span>)}</div>
  </div>;
}
