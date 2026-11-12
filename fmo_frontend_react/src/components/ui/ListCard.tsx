import type { ReactNode } from 'react';

/** The card around a list of federations */
export function ListCard({ className = '', children }: { className?: string; children: ReactNode }) {
  return (
    <div className={`relative overflow-x-auto bg-white shadow-md rounded-lg border border-gray-200 dark:bg-gray-800 dark:border-gray-700 ${className}`}>
      {children}
    </div>
  );
}

/** A list's loading, empty or other one-line state, in place of its rows */
export function ListMessage({ children }: { children: ReactNode }) {
  return (
    <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
      {children}
    </div>
  );
}
