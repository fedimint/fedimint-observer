import type { ReactNode } from 'react';
import { InfoTip } from './Tooltip';

interface KpiProps {
  label: string;
  value: ReactNode;

  detail?: ReactNode;

  indicator?: ReactNode;
  help?: ReactNode;
}

/** A compact headline figure; several sit in one row */
export function Kpi({ label, value, detail, indicator, help }: KpiProps) {
  return (
    <div className="min-w-0 rounded-lg border border-line bg-surface-2 px-4 py-3">
      <div className="meta-label flex items-center gap-1">
        <span className="truncate">{label}</span>
        {help && <InfoTip label={`About ${label}`}>{help}</InfoTip>}
      </div>
      <div className="tabular mt-2 flex items-center gap-1.5 text-xl font-medium text-ink">
        {indicator}
        <span className="truncate">{value}</span>
      </div>
      {detail && <div className="mt-0.5 text-xs leading-snug text-ink-3">{detail}</div>}
    </div>
  );
}
