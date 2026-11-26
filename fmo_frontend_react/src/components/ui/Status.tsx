import type { ReactNode } from 'react';
import { Icon, type IconName } from './Icon';


export type Tone = 'ok' | 'warn' | 'crit' | 'neutral' | 'info' | 'manifold' | 'demo';

const TONE_CLASSES: Record<Tone, string> = {
  ok: 'bg-ok-soft text-ok-ink ring-ok/25',
  warn: 'bg-warn-soft text-warn-ink ring-warn/30',
  crit: 'bg-crit-soft text-crit-ink ring-crit/25',
  neutral: 'bg-surface-3 text-ink-2 ring-line-strong/60',
  info: 'bg-accent-soft text-accent-ink ring-accent/25',
  manifold: 'bg-manifold-soft text-manifold-ink ring-manifold/25',
  demo: 'bg-demo-soft text-demo-ink ring-demo/30',
};

const TONE_ICONS: Record<Tone, IconName> = {
  ok: 'checkCircle',
  warn: 'alert',
  crit: 'xCircle',
  neutral: 'minusCircle',
  info: 'info',
  manifold: 'key',
  demo: 'flask',
};

const TONE_DOT: Record<Tone, string> = {
  ok: 'bg-ok',
  warn: 'bg-warn',
  crit: 'bg-crit',
  neutral: 'bg-ink-3',
  info: 'bg-accent',
  manifold: 'bg-manifold',
  demo: 'bg-demo',
};

interface StatusPillProps {
  tone: Tone;
  children: ReactNode;

  icon?: IconName | null;
  title?: string;
  size?: 'sm' | 'md';
}

export function StatusPill({ tone, children, icon, title, size = 'sm' }: StatusPillProps) {
  const iconName = icon === undefined ? TONE_ICONS[tone] : icon;
  return (
    <span
      title={title}
      className={`inline-flex max-w-full items-center gap-1 whitespace-nowrap rounded-md ring-1 ring-inset font-medium ${
        size === 'sm' ? 'px-1.5 py-0.5 text-xs' : 'px-2 py-1 text-xs'
      } ${TONE_CLASSES[tone]}`}
    >
      {iconName && <Icon name={iconName} className={size === 'sm' ? 'size-3' : 'size-3.5'} strokeWidth={2.2} />}
      <span className="truncate">{children}</span>
    </span>
  );
}

/** A small round marker for dense rows, always next to a text label */
export function StatusDot({ tone, pulse = false }: { tone: Tone; pulse?: boolean }) {
  return (
    <span className="relative inline-flex size-2 shrink-0" aria-hidden="true">
      {pulse && <span className={`fmo-live-halo absolute inset-0 rounded-full ${TONE_DOT[tone]}`} />}
      <span className={`relative inline-flex size-2 rounded-full ${TONE_DOT[tone]}`} />
    </span>
  );
}
