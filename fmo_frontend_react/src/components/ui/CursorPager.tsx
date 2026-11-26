import type { PageSize } from '../../utils/pageSize';
import { PageSizeSelect } from './PageSizeSelect';
import { Button } from './Button';

interface CursorPagerProps {

  page: number;
  hasNext: boolean;
  loading: boolean;
  onPrevious: () => void;
  onNext: () => void;

  label: string;

  pageSize?: PageSize;
  onPageSizeChange?: (size: PageSize) => void;

  total?: number;
}


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
