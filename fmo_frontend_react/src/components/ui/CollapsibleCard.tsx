import { useState, type ReactNode } from 'react';
import { Icon } from './Icon';
import { ListCard } from './ListCard';

interface CollapsibleCardProps {
  id: string;
  title: string;
  description: string;

  count?: number | null;
  className?: string;
  children: ReactNode;
}


export function CollapsibleCard({ id, title, description, count, className = 'mt-6', children }: CollapsibleCardProps) {
  const [open, setOpen] = useState(false);
  const [opened, setOpened] = useState(false);

  return (
    <ListCard className={className}>
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => {
          setOpened(true);
          setOpen((value) => !value);
        }}
        className="flex w-full items-start gap-2 bg-surface p-4 text-left transition-colors hover:bg-surface-2 sm:p-5"
      >
        <Icon name="chevronRight" className={`mt-0.5 size-4 text-ink-3 transition-transform duration-200 ease-[var(--fmo-ease)] motion-reduce:transition-none ${open ? 'rotate-90' : ''}`} />
        <span className="min-w-0">
          <span className="text-base font-medium text-ink">{title}</span>
          {count !== null && count !== undefined && <span className="ml-2 text-sm text-ink-3 tabular">({count})</span>}
          <span className="mt-0.5 block text-xs text-ink-3 sm:text-sm">{description}</span>
        </span>
      </button>

      <div
        id={id}
        inert={!open}
        aria-hidden={!open}
        className={`grid transition-[grid-template-rows] duration-300 ease-[var(--fmo-ease)] motion-reduce:transition-none ${open ? 'grid-rows-[1fr]' : 'grid-rows-[0fr]'}`}
      >
        <div className="min-h-0 overflow-hidden">{opened && children}</div>
      </div>
    </ListCard>
  );
}
