import type { ReactNode } from 'react';
import { Card } from './Card';

interface PanelProps {

  title?: ReactNode;

  padding?: string;

  className?: string;
  children: ReactNode;
}

/** A card with an optional heading; the same surface as every dashboard block */
export function Panel({ title, padding, className = '', children }: PanelProps) {
  return (
    <Card as="div" padding={padding} className={className}>
      {title && <h2 className="mb-4 text-base font-medium text-ink">{title}</h2>}
      {children}
    </Card>
  );
}
