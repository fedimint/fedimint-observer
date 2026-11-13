interface CursorPagerProps {
  /** 1-based number of the page shown */
  page: number;
  hasNext: boolean;
  loading: boolean;
  onPrevious: () => void;
  onNext: () => void;
  /** What is paged, for screen readers, e.g. "Gateway pages" */
  label: string;
}

const arrow = 'inline-flex h-8 w-8 items-center justify-center rounded-md border border-gray-300 text-base text-gray-700 hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-40 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-700 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-500';

/**
 * Previous/next arrows for lists the backend pages with a cursor. Cursor
 * pagination knows whether a next page exists, not how many pages there are,
 * so only the current page number is shown.
 */
export function CursorPager({ page, hasNext, loading, onPrevious, onNext, label }: CursorPagerProps) {
  if (page === 1 && !hasNext) return null;
  return (
    <nav aria-label={label} className="flex items-center justify-end gap-2">
      <button type="button" onClick={onPrevious} disabled={loading || page === 1} aria-label="Previous page" title="Previous page" className={arrow}>←</button>
      <span className="min-w-16 text-center text-xs text-gray-500 dark:text-gray-400" aria-live="polite">{loading ? 'Loading…' : `Page ${page}`}</span>
      <button type="button" onClick={onNext} disabled={loading || !hasNext} aria-label="Next page" title="Next page" className={arrow}>→</button>
    </nav>
  );
}
