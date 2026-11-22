import { Icon, type IconName } from './Icon';

type AlertLevel = 'info' | 'warning' | 'error' | 'success';

interface AlertProps {
  message: string;
  level: AlertLevel;
}

const STYLE: Record<AlertLevel, { box: string; icon: IconName }> = {
  info: { box: 'border-accent/25 bg-accent-soft text-accent-ink', icon: 'info' },
  warning: { box: 'border-warn/40 bg-warn-soft text-warn-ink', icon: 'alert' },
  error: { box: 'border-crit/25 bg-crit-soft text-crit-ink', icon: 'xCircle' },
  success: { box: 'border-ok/25 bg-ok-soft text-ok-ink', icon: 'checkCircle' },
};

/** A one-message notice above a block: tone border, icon, text */
export function Alert({ message, level }: AlertProps) {
  const { box, icon } = STYLE[level];
  return (
    <div role={level === 'error' ? 'alert' : 'status'} className={`mb-4 flex items-start gap-2.5 rounded-xl border px-4 py-3 text-sm ${box}`}>
      <Icon name={icon} className="mt-0.5 size-4" />
      <p className="min-w-0">{message}</p>
    </div>
  );
}
