import type { ReactNode } from 'react';

/** The card around a list of rows: the dashboard card surface with no padding */
export function ListCard({ className = '', children }: { className?: string; children: ReactNode }) {
  return (
    <div className={`relative overflow-x-auto rounded-xl border border-line bg-surface shadow-[0_1px_2px_rgba(10,17,34,0.04)] ${className}`}>
      {children}
    </div>
  );
}

/** A list's loading, empty or other one-line state, in place of its rows */
export function ListMessage({ children }: { children: ReactNode }) {
  return <div className="bg-surface px-3 py-4 text-center text-xs text-ink-3 sm:px-6 sm:text-sm">{children}</div>;
}
