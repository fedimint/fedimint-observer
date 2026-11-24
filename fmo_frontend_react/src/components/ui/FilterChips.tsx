import type { ReactNode } from 'react';
import { CONTROL_SIZE, TOGGLE_OFF, TOGGLE_ON } from './controls';

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

/** A wrapping row of filter toggles, exactly one of them pressed */
export function FilterChips<T extends string>({ options, value, onChange, renderLabel, label, className = '' }: FilterChipsProps<T>) {
  return (
    <div className={`flex flex-wrap gap-2 ${className}`} role="group" aria-label={label}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          aria-pressed={value === option}
          onClick={() => onChange(option)}
          className={`inline-flex items-center gap-1.5 whitespace-nowrap rounded-lg border font-medium capitalize transition-colors ${CONTROL_SIZE.sm} ${
            value === option ? TOGGLE_ON : TOGGLE_OFF
          }`}
        >
          {renderLabel(option)}
        </button>
      ))}
    </div>
  );
}
