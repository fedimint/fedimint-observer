import type { ReactNode } from 'react';

interface CardProps {
  children: ReactNode;
  className?: string;

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

  description?: ReactNode;

  actions?: ReactNode;

  meta?: ReactNode;
}

export function CardHeader({ title, id, description, actions, meta }: CardHeaderProps) {
  return (
    <div className="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div className="min-w-0">
        <h2 id={id} className="text-base font-medium text-ink">{title}</h2>
        {description && <p className="mt-0.5 text-xs text-ink-3 sm:text-sm">{description}</p>}
        {meta && <div className="mt-2">{meta}</div>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}
