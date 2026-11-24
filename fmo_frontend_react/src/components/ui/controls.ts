

export type ControlSize = 'sm' | 'md';


export const CONTROL_SIZE: Record<ControlSize, string> = {
  sm: 'h-8 text-xs px-3',
  md: 'h-10 text-sm px-4',
};


export const ICON_CONTROL_SIZE: Record<ControlSize, string> = {
  sm: 'size-8',
  md: 'size-10',
};


export const CONTROL_RADIUS = 'rounded-lg';


export const FIELD =
  `${CONTROL_RADIUS} border border-line-strong bg-surface text-ink placeholder:text-ink-3 transition-colors focus:border-accent focus:ring-2 focus:ring-accent/25 focus:outline-none disabled:cursor-not-allowed disabled:opacity-50`;

export type ButtonVariant = 'primary' | 'secondary' | 'ghost';

export const BUTTON_VARIANT: Record<ButtonVariant, string> = {
  primary: 'bg-accent text-on-accent hover:brightness-110 border border-transparent',
  secondary: 'border border-line-strong bg-surface text-ink-2 hover:bg-surface-2 hover:text-ink',
  ghost: 'border border-transparent text-ink-2 hover:bg-surface-3 hover:text-ink',
};

export const BUTTON_BASE =
  `inline-flex shrink-0 items-center justify-center gap-1.5 whitespace-nowrap font-medium ${CONTROL_RADIUS} transition-colors disabled:cursor-not-allowed disabled:opacity-40`;


export const TOGGLE_ON = 'border-accent/40 bg-accent-soft text-accent-ink';
export const TOGGLE_OFF = 'border-line-strong bg-surface text-ink-2 hover:bg-surface-2 hover:text-ink';

/** Classes of a link styled as a button, for router and external links */
export function buttonClass(variant: ButtonVariant = 'secondary', size: ControlSize = 'sm') {
  return `${BUTTON_BASE} ${BUTTON_VARIANT[variant]} ${CONTROL_SIZE[size]}`;
}
