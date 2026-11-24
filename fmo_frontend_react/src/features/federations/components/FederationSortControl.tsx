import { Button } from '../../../components/ui/Button';
import { Select } from '../../../components/ui/Select';
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
      <label htmlFor="federation-sort" className="text-xs font-medium text-ink-3">
        Sort by
      </label>
      <Select
        id="federation-sort"
        value={sortKey}
        onChange={(e) => {
          const key = e.target.value as SortKey;
          onChange(key, defaultDirectionFor(key));
        }}
      >
        {SORT_OPTIONS.map((option) => (
          <option key={option.key} value={option.key}>{option.label}</option>
        ))}
      </Select>
      <Button
        icon="sort"
        onClick={() => onChange(sortKey, sortDirection === 'asc' ? 'desc' : 'asc')}
        aria-label={`Sorted ${sortDirection === 'asc' ? 'ascending' : 'descending'}; reverse order`}
        title={sortDirection === 'asc' ? 'Ascending' : 'Descending'}
      >
        {sortDirection === 'asc' ? 'Asc' : 'Desc'}
      </Button>
    </div>
  );
}
