export type GatewayWarningLevel = 'info' | 'warning' | 'error';

export interface GatewayWarningState {
  level: GatewayWarningLevel;
  title: string;
  message: string;
  detail?: string;
}

interface GatewayWarningPageProps {
  warning: GatewayWarningState;
  className?: string;
}

function levelClasses(level: GatewayWarningLevel): string {
  switch (level) {
    case 'info':
      return 'border-blue-200 bg-blue-50 text-blue-900 dark:border-blue-700 dark:bg-blue-900/30 dark:text-blue-200';
    case 'error':
      return 'border-red-200 bg-red-50 text-red-900 dark:border-red-700 dark:bg-red-900/30 dark:text-red-200';
    default:
      return 'border-yellow-200 bg-yellow-50 text-yellow-900 dark:border-yellow-700 dark:bg-yellow-900/30 dark:text-yellow-200';
  }
}

export function GatewayWarningPage({ warning, className = '' }: GatewayWarningPageProps) {
  return (
    <section
      className={`rounded-lg border p-4 sm:p-5 ${levelClasses(warning.level)} ${className}`}
      role="status"
      aria-live="polite"
    >
      <h2 className="text-sm sm:text-base font-semibold">{warning.title}</h2>
      <p className="mt-1 text-sm leading-relaxed">{warning.message}</p>
      {warning.detail && (
        <p className="mt-2 text-xs sm:text-sm opacity-90 break-words">
          {warning.detail}
        </p>
      )}
    </section>
  );
}
