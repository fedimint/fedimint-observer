import { useId, useState, type ReactNode } from 'react';
import { Icon } from './Icon';

/**
 * An explanation behind a small "?" button: shown on hover and on keyboard
 * focus, dismissed with Escape, and linked to the button for screen readers.
 */
export function InfoTip({ label, children, align = 'center' }: { label: string; children: ReactNode; align?: 'center' | 'start' | 'end' }) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const position = align === 'start' ? 'left-0' : align === 'end' ? 'right-0' : 'left-1/2 -translate-x-1/2';
  return (
    <span className="relative inline-flex align-middle" onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
      <button
        type="button"
        aria-label={label}
        aria-describedby={open ? id : undefined}
        aria-expanded={open}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={() => setOpen((value) => !value)}
        onKeyDown={(event) => event.key === 'Escape' && setOpen(false)}
        className="inline-flex size-4.5 items-center justify-center rounded-full text-ink-3 hover:text-ink"
      >
        <Icon name="help" className="size-3.5" />
      </button>
      {open && (
        <span
          id={id}
          role="tooltip"
          className={`absolute top-full z-40 mt-1.5 w-64 rounded-lg border border-line-strong bg-surface px-3 py-2 text-left text-xs font-normal normal-case leading-relaxed tracking-normal text-ink-2 shadow-lg ${position}`}
        >
          {children}
        </span>
      )}
    </span>
  );
}
