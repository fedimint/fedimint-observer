import type { ReactNode } from 'react';

interface FilterChipsProps<T extends string> {
  options: readonly T[];
  value: T;
  onChange: (value: T) => void;
  /** The text of each chip, e.g. with a count */
  renderLabel: (option: T) => ReactNode;
  /** Accessible name of the group */
  label: string;
  /** Classes of the row of chips, e.g. its margin */
  className?: string;
}

/** A row of pill-shaped toggle buttons, exactly one of them pressed */
export function FilterChips<T extends string>({ options, value, onChange, renderLabel, label, className = '' }: FilterChipsProps<T>) {
  return (
    <div className={`flex flex-wrap gap-2 ${className}`} role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          aria-pressed={value === option}
          onClick={() => onChange(option)}
          className={`whitespace-nowrap rounded-full border px-3 py-1.5 text-xs font-medium capitalize ${
            value === option ? 'border-blue-600 bg-blue-600 text-white' : 'border-gray-300 text-gray-600 dark:border-gray-600 dark:text-gray-300'
          }`}
        >
          {renderLabel(option)}
        </button>
      ))}
    </div>
  );
}
