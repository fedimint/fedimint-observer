import { useState } from 'react';
import { Icon, type IconName } from '../../../components/ui/Icon';
import type { GuardianWarning, WarningTone } from '../lib/guardianModel';

const STYLE: Record<WarningTone, { icon: IconName; classes: string; label: string }> = {
  crit: { icon: 'xCircle', classes: 'border-crit/30 bg-crit-soft text-crit-ink', label: 'Critical' },
  warn: { icon: 'alert', classes: 'border-warn/35 bg-warn-soft text-warn-ink', label: 'Warning' },
  info: { icon: 'info', classes: 'border-accent/25 bg-accent-soft text-accent-ink', label: 'Note' },
};

interface OperationalWarningsProps {
  warnings: readonly GuardianWarning[];
  /** Shown when there is nothing to warn about; omit to render nothing */
  allClear?: string;
  /** Warnings shown before "Show all" */
  limit?: number;
  onSelectGuardian?: (id: number) => void;
}

/** Short operational messages, most severe first; each names what it is about */
export function OperationalWarnings({ warnings, allClear, limit = 4, onSelectGuardian }: OperationalWarningsProps) {
  const [expanded, setExpanded] = useState(false);
  if (warnings.length === 0) {
    return allClear ? (
      <p className="flex items-center gap-2 text-xs text-ink-3">
        <Icon name="checkCircle" className="size-3.5 text-ok" />
        {allClear}
      </p>
    ) : null;
  }
  const shown = expanded ? warnings : warnings.slice(0, limit);
  return (
    <div>
      <ul className="space-y-1.5" aria-label="Operational warnings">
        {shown.map((warning) => {
          const style = STYLE[warning.tone];
          const content = (
            <>
              <Icon name={style.icon} className="mt-0.5 size-3.5" strokeWidth={2.2} />
              <span className="sr-only">{style.label}: </span>
              <span className="min-w-0 flex-1 text-left">{warning.message}</span>
              {onSelectGuardian && warning.guardianId !== null && <Icon name="chevronRight" className="mt-0.5 size-3.5 opacity-70" />}
            </>
          );
          return (
            <li key={warning.id}>
              {onSelectGuardian && warning.guardianId !== null ? (
                <button
                  type="button"
                  onClick={() => onSelectGuardian(warning.guardianId!)}
                  className={`flex w-full items-start gap-2 rounded-lg border px-3 py-2 text-xs hover:brightness-[0.98] ${style.classes}`}
                >
                  {content}
                </button>
              ) : (
                <div className={`flex items-start gap-2 rounded-lg border px-3 py-2 text-xs ${style.classes}`}>{content}</div>
              )}
            </li>
          );
        })}
      </ul>
      {warnings.length > limit && (
        <button type="button" onClick={() => setExpanded((value) => !value)} className="mt-2 text-xs font-medium text-accent-ink hover:underline">
          {expanded ? 'Show fewer' : `Show all ${warnings.length} messages`}
        </button>
      )}
    </div>
  );
}
