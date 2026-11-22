import type { GatewayWarningLevel, GatewayWarningState } from '../lib/gatewaySelection';

interface GatewayWarningPageProps {
  warning: GatewayWarningState;
  className?: string;
  action?: { label: string; onClick: () => void; busy?: boolean };
}

function levelClasses(level: GatewayWarningLevel): string {
  switch (level) {
    case 'info':
      return 'border-accent/30 bg-accent-soft text-accent-ink';
    case 'error':
      return 'border-crit/30 bg-crit-soft text-crit-ink';
    default:
      return 'border-warn/30 bg-warn-soft text-warn-ink';
  }
}

export function GatewayWarningPage({ warning, className = '', action }: GatewayWarningPageProps) {
  return (
    <section
      className={`rounded-lg border p-4 sm:p-5 ${levelClasses(warning.level)} ${className}`}
      role="status"
      aria-live="polite"
    >
      <h2 className="text-sm sm:text-base font-semibold">{warning.title}</h2>
      <p className="mt-1 text-sm leading-relaxed">{warning.message}</p>
      {warning.detail && (
        <p className="mt-2 text-xs sm:text-sm opacity-90 wrap-break-word">
          {warning.detail}
        </p>
      )}
      {action && (
        <button
          type="button"
          onClick={action.onClick}
          aria-disabled={action.busy}
          className={`mt-3 rounded-md border border-current px-3 py-1.5 text-xs font-semibold ${action.busy ? 'cursor-wait opacity-70' : 'hover:opacity-80'}`}
        >
          {action.label}
        </button>
      )}
    </section>
  );
}
