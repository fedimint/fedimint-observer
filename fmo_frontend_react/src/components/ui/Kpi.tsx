import type { ReactNode } from 'react';
import { InfoTip } from './Tooltip';

interface KpiProps {
  label: string;
  value: ReactNode;
  /** One short line under the value */
  detail?: ReactNode;
  /** Marker before the value, e.g. a status icon */
  indicator?: ReactNode;
  help?: ReactNode;
}

/** A compact headline figure; several sit in one row */
export function Kpi({ label, value, detail, indicator, help }: KpiProps) {
  return (
    <div className="min-w-0 rounded-lg border border-line bg-surface-2 px-3.5 py-3">
      <div className="flex items-center gap-1 text-[0.7rem] font-medium uppercase tracking-wide text-ink-3">
        <span className="truncate">{label}</span>
        {help && <InfoTip label={`About ${label}`}>{help}</InfoTip>}
      </div>
      <div className="mt-1.5 flex items-center gap-1.5 text-xl font-semibold tracking-tight text-ink">
        {indicator}
        <span className="truncate">{value}</span>
      </div>
      {detail && <div className="mt-0.5 text-xs leading-snug text-ink-3">{detail}</div>}
    </div>
  );
}
