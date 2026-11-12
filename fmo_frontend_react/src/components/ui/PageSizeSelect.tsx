import { useId } from 'react';
import { PAGE_SIZES, type PageSize } from '../../utils/pageSize';

interface PageSizeSelectProps {
  value: PageSize;
  onChange: (size: PageSize) => void;
  disabled?: boolean;
  /** Rows across all pages, when the backend says */
  total?: number;
}

/** "Rows per page", sent to the backend as the list's `limit` */
export function PageSizeSelect({ value, onChange, disabled, total }: PageSizeSelectProps) {
  const id = useId();
  return (
    <span className="inline-flex items-center gap-2 text-xs text-ink-3">
      <label htmlFor={id}>Rows per page</label>
      <select
        id={id}
        value={value}
        onChange={(event) => onChange(Number(event.target.value) as PageSize)}
        disabled={disabled}
        className="h-8 rounded-lg border border-line bg-surface-2 py-0 pr-7 pl-2 text-xs text-ink focus:border-accent focus:ring-0"
      >
        {PAGE_SIZES.map((size) => <option key={size} value={size}>{size}</option>)}
      </select>
      {total !== undefined && <span className="tabular">{total.toLocaleString('en-US')} total</span>}
    </span>
  );
}
