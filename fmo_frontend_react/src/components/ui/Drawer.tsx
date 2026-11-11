import { useEffect, useId, useRef, type ReactNode } from 'react';
import { Icon } from './Icon';

interface DrawerProps {
  title: ReactNode;
  subtitle?: ReactNode;
  onClose: () => void;
  children: ReactNode;
  /** Width class of the panel on larger screens */
  width?: string;
}

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea, [tabindex]:not([tabindex="-1"])';

/**
 * A detail panel sliding over the page from the right (full screen on
 * phones). Focus moves into it, stays inside while it is open, and returns to
 * whatever opened it; Escape and the backdrop close it.
 */
export function Drawer({ title, subtitle, onClose, children, width = 'sm:max-w-xl' }: DrawerProps) {
  const titleId = useId();
  const panel = useRef<HTMLElement>(null);
  // Parents pass a new callback on every render; the effect below must run
  // once per opening, or each refresh would move focus back into the panel
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  }, [onClose]);

  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    panel.current?.focus();
    const { overflow } = document.body.style;
    document.body.style.overflow = 'hidden';

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.stopPropagation();
        close.current();
        return;
      }
      if (event.key !== 'Tab' || !panel.current) return;
      const focusable = [...panel.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && (document.activeElement === first || document.activeElement === panel.current)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('keydown', onKeyDown);
      document.body.style.overflow = overflow;
      opener?.focus?.();
    };
  }, []);

  return (
    <div role="presentation" className="fixed inset-0 z-50 flex justify-end bg-gray-950/50 backdrop-blur-[2px]" onMouseDown={onClose}>
      <section
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onMouseDown={(event) => event.stopPropagation()}
        className={`flex h-full w-full flex-col border-l border-line bg-canvas shadow-2xl outline-none ${width}`}
      >
        <header className="flex items-start justify-between gap-4 border-b border-line bg-surface px-5 py-4">
          <div className="min-w-0">
            <h2 id={titleId} className="truncate text-base font-semibold text-ink">{title}</h2>
            {subtitle && <div className="mt-1 text-xs text-ink-3">{subtitle}</div>}
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close panel"
            className="inline-flex size-8 shrink-0 items-center justify-center rounded-lg text-ink-3 hover:bg-surface-3 hover:text-ink"
          >
            <Icon name="close" className="size-4" />
          </button>
        </header>
        <div className="flex-1 space-y-4 overflow-y-auto px-5 py-5 scrollbar-thin">{children}</div>
      </section>
    </div>
  );
}
