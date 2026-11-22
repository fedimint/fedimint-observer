import type { KeyboardEvent } from 'react';
import { CONTROL_SIZE, FIELD, type ControlSize } from './controls';
import { Icon } from './Icon';

interface SearchInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  /** Accessible name of the field */
  label: string;
  /** Accessible name of the button that clears the field */
  clearLabel?: string;
  onKeyDown?: (event: KeyboardEvent<HTMLInputElement>) => void;
  size?: ControlSize;
  className?: string;
}

/** A search field with a search icon and a button to clear it */
export function SearchInput({ value, onChange, placeholder, label, clearLabel = 'Clear search', onKeyDown, size = 'sm', className = '' }: SearchInputProps) {
  const iconSize = size === 'sm' ? 'size-3.5' : 'size-4';
  const inset = size === 'sm' ? 'pl-8 pr-8' : 'pl-10 pr-10';
  return (
    <div className={`relative min-w-0 ${className}`}>
      <Icon name="search" className={`pointer-events-none absolute top-1/2 -translate-y-1/2 text-ink-3 ${iconSize} ${size === 'sm' ? 'left-2.5' : 'left-3.5'}`} />
      <input
        type="search"
        aria-label={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        className={`${FIELD} ${CONTROL_SIZE[size]} ${inset} block w-full [&::-webkit-search-cancel-button]:hidden`}
      />
      {value && (
        <button
          type="button"
          aria-label={clearLabel}
          onClick={() => onChange('')}
          className={`absolute top-1/2 -translate-y-1/2 rounded-md p-1 text-ink-3 hover:bg-surface-3 hover:text-ink ${size === 'sm' ? 'right-1.5' : 'right-2.5'}`}
        >
          <Icon name="close" className={iconSize} />
        </button>
      )}
    </div>
  );
}
