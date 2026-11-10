import { ChevronUpIcon } from '../../../components/ui/icons';
import { SORT_OPTIONS, defaultDirectionFor, type SortDirection, type SortKey } from '../../../utils/sortFederations';

interface FederationSortControlProps {
  sortKey: SortKey;
  sortDirection: SortDirection;
  onChange: (key: SortKey, direction: SortDirection) => void;
}

/** "Sort by" a metric, with a button that reverses the order */
export function FederationSortControl({ sortKey, sortDirection, onChange }: FederationSortControlProps) {
  return (
    <div className="flex items-center gap-2 shrink-0 self-start">
      <label htmlFor="federation-sort" className="text-xs sm:text-sm font-medium text-gray-500 dark:text-gray-400">
        Sort by
      </label>
      <select
        id="federation-sort"
        value={sortKey}
        // A new metric starts "best first"; the button then reverses it
        onChange={(e) => {
          const key = e.target.value as SortKey;
          onChange(key, defaultDirectionFor(key));
        }}
        className="text-xs sm:text-sm rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-gray-900 focus:border-blue-500 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
      >
        {SORT_OPTIONS.map((option) => (
          <option key={option.key} value={option.key}>{option.label}</option>
        ))}
      </select>
      <button
        type="button"
        onClick={() => onChange(sortKey, sortDirection === 'asc' ? 'desc' : 'asc')}
        aria-label={`Sorted ${sortDirection === 'asc' ? 'ascending' : 'descending'}; reverse order`}
        title={sortDirection === 'asc' ? 'Ascending' : 'Descending'}
        className="rounded-lg border border-gray-300 bg-white p-1.5 text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600"
      >
        <ChevronUpIcon className={`w-3 h-3 transition-transform ${sortDirection === 'asc' ? '' : 'rotate-180'}`} />
      </button>
    </div>
  );
}
