import type { ReactNode } from 'react';
import { Icon, type IconName } from './Icon';
import { RetryButton } from './RetryButton';

export function Skeleton({ className = 'h-4 w-full' }: { className?: string }) {
  return <div aria-hidden="true" className={`skeleton ${className}`} />;
}

/** A loading block announced once to screen readers */
export function LoadingBlock({ label, rows = 4, className = '' }: { label: string; rows?: number; className?: string }) {
  return (
    <div role="status" aria-label={label} className={`space-y-2.5 ${className}`}>
      {Array.from({ length: rows }, (_, i) => (
        <Skeleton key={i} className={`h-9 ${i % 3 === 2 ? 'w-4/5' : 'w-full'}`} />
      ))}
    </div>
  );
}

export function EmptyState({ icon = 'info', title, children, action }: { icon?: IconName; title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-line-strong px-6 py-8 text-center">
      <span className="mb-3 inline-flex size-9 items-center justify-center rounded-full bg-surface-3 text-ink-3">
        <Icon name={icon} className="size-4.5" />
      </span>
      <p className="text-sm font-medium text-ink">{title}</p>
      {children && <div className="mt-1 max-w-md text-xs leading-relaxed text-ink-3">{children}</div>}
      {action && <div className="mt-3">{action}</div>}
    </div>
  );
}

export function ErrorState({ title, message, onRetry }: { title: string; message?: string; onRetry?: () => void }) {
  return (
    <div role="alert" className="flex flex-wrap items-start gap-3 rounded-lg border border-crit/30 bg-crit-soft px-4 py-3 text-sm text-crit-ink">
      <Icon name="xCircle" className="mt-0.5 size-4" />
      <div className="min-w-0 flex-1">
        <p className="font-semibold">{title}</p>
        {message && <p className="mt-0.5 text-xs opacity-90">{message}</p>}
      </div>
      {onRetry && <RetryButton onClick={onRetry} />}
    </div>
  );
}
