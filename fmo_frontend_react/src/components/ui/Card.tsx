import type { ReactNode } from 'react';

interface CardProps {
  children: ReactNode;
  className?: string;
  /** Padding classes, replacing the default */
  padding?: string;
  as?: 'section' | 'div' | 'article';
  'aria-labelledby'?: string;
}

/** The surface every dashboard block sits on: hairline border, soft corners, no glow */
export function Card({ children, className = '', padding = 'p-4 sm:p-5', as: Tag = 'section', ...rest }: CardProps) {
  return (
    <Tag className={`rounded-xl border border-line bg-surface shadow-[0_1px_2px_rgba(10,17,34,0.04)] ${padding} ${className}`} {...rest}>
      {children}
    </Tag>
  );
}

interface CardHeaderProps {
  title: ReactNode;
  id?: string;
  /** One line under the title saying what the block shows */
  description?: ReactNode;
  /** Controls such as a time-range selector */
  actions?: ReactNode;
  /** Data provenance, e.g. a DataStamp */
  meta?: ReactNode;
}

export function CardHeader({ title, id, description, actions, meta }: CardHeaderProps) {
  return (
    <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div className="min-w-0">
        <h2 id={id} className="text-[0.95rem] font-semibold tracking-tight text-ink">{title}</h2>
        {description && <p className="mt-0.5 text-xs text-ink-3 sm:text-[0.8rem]">{description}</p>}
        {meta && <div className="mt-1.5">{meta}</div>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}
