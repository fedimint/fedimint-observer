import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { Icon } from './Icon';

interface PageHeaderProps {
  title: ReactNode;
  /** Small label above the title, e.g. the section */
  eyebrow?: ReactNode;
  description?: ReactNode;
  /** Badges after the title */
  badges?: ReactNode;
  actions?: ReactNode;
  /** Where the page sits, e.g. Manifold › Guardian directory › name; the last is this page */
  breadcrumbs?: { label: ReactNode; to?: string }[];
}

export function PageHeader({ title, eyebrow, description, badges, actions, breadcrumbs }: PageHeaderProps) {
  return (
    <header className="mb-5 sm:mb-6">
      {breadcrumbs && breadcrumbs.length > 0 && (
        <nav aria-label="Breadcrumb" className="mb-2">
          <ol className="flex min-w-0 flex-wrap items-center gap-1 text-xs text-ink-3">
            {breadcrumbs.map((crumb, index) => {
              const last = index === breadcrumbs.length - 1;
              return (
                <li key={index} className="inline-flex min-w-0 items-center gap-1">
                  {crumb.to && !last ? (
                    <Link to={crumb.to} className="font-medium hover:text-accent-ink hover:underline">{crumb.label}</Link>
                  ) : (
                    <span aria-current={last ? 'page' : undefined} className={`truncate ${last ? 'text-ink-2' : ''}`}>{crumb.label}</span>
                  )}
                  {!last && <Icon name="chevronRight" className="size-3 shrink-0" />}
                </li>
              );
            })}
          </ol>
        </nav>
      )}
      <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
        <div className="min-w-0">
          {eyebrow && <div className="mb-1 text-[0.7rem] font-semibold uppercase tracking-[0.08em] text-ink-3">{eyebrow}</div>}
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="min-w-0 text-xl font-semibold tracking-tight text-ink wrap-break-word sm:text-2xl">{title}</h1>
            {badges}
          </div>
          {description && <p className="mt-1 max-w-3xl text-sm text-ink-3">{description}</p>}
        </div>
        {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
      </div>
    </header>
  );
}
