import { useId } from 'react';
import { PAGE_SIZES, type PageSize } from '../../utils/pageSize';
import { Select } from './Select';

interface PageSizeSelectProps {
  value: PageSize;
  onChange: (size: PageSize) => void;
  disabled?: boolean;

  total?: number;
}

/** "Rows per page", sent to the backend as the list's `limit` */
export function PageSizeSelect({ value, onChange, disabled, total }: PageSizeSelectProps) {
  const id = useId();
  return (
    <span className="inline-flex items-center gap-2 text-xs text-ink-3">
      <label htmlFor={id}>Rows per page</label>
      <Select id={id} value={value} onChange={(event) => onChange(Number(event.target.value) as PageSize)} disabled={disabled} className="pl-2.5">
        {PAGE_SIZES.map((size) => <option key={size} value={size}>{size}</option>)}
      </Select>
      {total !== undefined && <span className="tabular">{total.toLocaleString('en-US')} total</span>}
    </span>
  );
}
