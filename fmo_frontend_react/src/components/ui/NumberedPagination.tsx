import { pageItems } from '../../utils/pagination';
import { Button } from './Button';
import { TOGGLE_ON } from './controls';

interface NumberedPaginationProps {

  page: number;
  pageCount: number;
  onChange: (page: number) => void;

  label: string;
}

/** Previous/next buttons around page numbers, for lists with a known page count */
export function NumberedPagination({ page, pageCount, onChange, label }: NumberedPaginationProps) {
  if (pageCount <= 1) return null;
  return (
    <nav aria-label={label} className="flex flex-wrap items-center justify-between gap-2">
      <Button icon="arrowLeft" onClick={() => onChange(page - 1)} disabled={page === 1}>Previous</Button>
      <div className="flex flex-wrap items-center gap-1">
        {pageItems(page, pageCount).map((item, index) =>
          item === 'gap' ? (
            <span key={`gap-${index}`} className="px-1 text-xs text-ink-3">…</span>
          ) : (
            <Button
              key={item}
              variant="ghost"
              onClick={() => onChange(item)}
              aria-current={item === page ? 'page' : undefined}
              className={`min-w-8 px-2 tabular ${item === page ? TOGGLE_ON : ''}`}
            >
              {item}
            </Button>
          )
        )}
      </div>
      <Button onClick={() => onChange(page + 1)} disabled={page === pageCount}>
        Next <span aria-hidden="true">→</span>
      </Button>
    </nav>
  );
}
