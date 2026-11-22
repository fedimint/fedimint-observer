import type { ReactNode } from 'react';
import { useState } from 'react';
import { Icon } from './Icon';
import { StatusPill, type Tone } from './Status';

type BadgeLevel = 'info' | 'warning' | 'error' | 'success';

const TONE: Record<BadgeLevel, Tone> = { info: 'info', warning: 'warn', error: 'crit', success: 'ok' };

interface BadgeProps {
  level: BadgeLevel;
  tooltip?: string;
  children?: ReactNode; // not needed for the icon-only badge (showIcon with a tooltip)
  showIcon?: boolean;
}

/** A status pill, or with `showIcon` a warning icon whose explanation shows on hover and focus */
export function Badge({ level, tooltip, children, showIcon }: BadgeProps) {
  const [open, setOpen] = useState(false);
  if (showIcon && tooltip) {
    return (
      <span
        className="relative inline-flex shrink-0"
        tabIndex={0}
        aria-label={tooltip}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
      >
        <span className={`inline-flex size-5 items-center justify-center rounded-md ${level === 'error' ? 'bg-crit-soft text-crit-ink' : 'bg-warn-soft text-warn-ink'}`}>
          <Icon name="alert" className="size-3.5" strokeWidth={2.2} />
        </span>
        {open && (
          <span role="tooltip" className="absolute bottom-full left-0 z-50 mb-2 w-max max-w-72 rounded-lg border border-line-strong bg-surface px-3 py-2 text-xs font-normal text-ink-2 shadow-lg">
            {tooltip}
          </span>
        )}
      </span>
    );
  }
  return <StatusPill tone={TONE[level]} title={tooltip}>{children}</StatusPill>;
}
