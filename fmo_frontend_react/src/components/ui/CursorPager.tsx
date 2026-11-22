import type { PageSize } from '../../utils/pageSize';
import { PageSizeSelect } from './PageSizeSelect';
import { Button } from './Button';

interface CursorPagerProps {
  /** 1-based number of the page shown */
  page: number;
  hasNext: boolean;
  loading: boolean;
  onPrevious: () => void;
  onNext: () => void;
  /** What is paged, for screen readers, e.g. "Gateway pages" */
  label: string;
  /** Rows per page, sent to the backend as `limit`; omit to offer no choice */
  pageSize?: PageSize;
  onPageSizeChange?: (size: PageSize) => void;
  /** Rows across all pages, when the backend says */
  total?: number;
}

/**
 * Rows per page and previous/next for lists the backend pages with a cursor.
 * Each click asks the backend for that page only. Cursor paging knows whether
 * a next page exists, not how many there are, so only the current page number
 * is shown (with the row count, when the backend gives one).
 */
export function CursorPager({ page, hasNext, loading, onPrevious, onNext, label, pageSize, onPageSizeChange, total }: CursorPagerProps) {
  const withSize = pageSize !== undefined && onPageSizeChange !== undefined;
  const paged = page > 1 || hasNext;
  if (!paged && !withSize) return null;
  return (
    <nav aria-label={label} className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 text-xs text-ink-3">
      {withSize ? <PageSizeSelect value={pageSize} onChange={onPageSizeChange} disabled={loading} total={total} /> : <span />}
      {paged && (
        <span className="inline-flex items-center gap-2">
          <Button icon="arrowLeft" onClick={onPrevious} disabled={loading || page === 1} aria-label="Previous page" title="Previous page" />
          <span className="min-w-16 text-center tabular" aria-live="polite">{loading ? 'Loading…' : `Page ${page}`}</span>
          <Button icon="chevronRight" onClick={onNext} disabled={loading || !hasNext} aria-label="Next page" title="Next page" />
        </span>
      )}
    </nav>
  );
}
