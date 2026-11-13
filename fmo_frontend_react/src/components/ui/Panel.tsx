import type { ReactNode } from 'react';

interface PanelProps {
  /** Shown as the panel's heading */
  title?: ReactNode;
  /** Padding classes, replacing the default */
  padding?: string;
  /** Further classes, e.g. grid placement */
  className?: string;
  children: ReactNode;
}

/** A white card with an optional heading, as on the federation page */
export function Panel({ title, padding = 'p-4 sm:p-6', className = '', children }: PanelProps) {
  return (
    <div className={`bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 ${padding} ${className}`}>
      {title && (
        <h2 className="text-base sm:text-lg font-semibold text-gray-900 dark:text-white mb-3 sm:mb-4">
          {title}
        </h2>
      )}
      {children}
    </div>
  );
}
