import { pageItems } from '../../utils/pagination';

interface NumberedPaginationProps {
  /** 1-based number of the page shown */
  page: number;
  pageCount: number;
  onChange: (page: number) => void;
  /** What is paged, for screen readers, e.g. "UTXO pages" */
  label: string;
}

const stepButton = 'rounded-md border border-gray-300 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-40 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-700';

/** Previous/next buttons around page numbers, for lists with a known page count */
export function NumberedPagination({ page, pageCount, onChange, label }: NumberedPaginationProps) {
  if (pageCount <= 1) return null;
  return (
    <nav
      aria-label={label}
      className="flex flex-wrap items-center justify-between gap-2 border-t border-gray-200 px-4 py-3 dark:border-gray-700 sm:px-6"
    >
      <button type="button" onClick={() => onChange(page - 1)} disabled={page === 1} className={stepButton}>
        ← Previous
      </button>
      <div className="flex flex-wrap items-center gap-1">
        {pageItems(page, pageCount).map((item, index) =>
          item === 'gap' ? (
            <span key={`gap-${index}`} className="px-1 text-xs text-gray-400">…</span>
          ) : (
            <button
              key={item}
              type="button"
              onClick={() => onChange(item)}
              aria-current={item === page ? 'page' : undefined}
              className={`min-w-8 rounded-md px-2 py-1.5 text-xs font-medium ${item === page ? 'bg-blue-600 text-white' : 'text-gray-700 hover:bg-gray-100 dark:text-gray-200 dark:hover:bg-gray-700'}`}
            >
              {item}
            </button>
          )
        )}
      </div>
      <button type="button" onClick={() => onChange(page + 1)} disabled={page === pageCount} className={stepButton}>
        Next →
      </button>
    </nav>
  );
}
