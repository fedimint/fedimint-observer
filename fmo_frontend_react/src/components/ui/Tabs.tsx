import type { ReactNode } from 'react';

export interface TabItem<T extends string> {
  id: T;
  label: ReactNode;
}

interface TabsProps<T extends string> {
  items: readonly TabItem<T>[];
  active: T;
  onSelect: (id: T) => void;
}

/** An underlined tab bar; the caller renders the active tab's content */
export function Tabs<T extends string>({ items, active, onSelect }: TabsProps<T>) {
  return (
    <div className="border-b border-gray-200 dark:border-gray-700">
      <nav className="-mb-px flex space-x-4 sm:space-x-8 overflow-x-auto">
        {items.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => onSelect(item.id)}
            className={`border-b-2 py-3 sm:py-4 px-1 text-xs sm:text-sm font-medium whitespace-nowrap ${
              active === item.id
                ? 'border-blue-500 text-blue-600 dark:text-blue-400'
                : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 dark:text-gray-400 dark:hover:text-gray-300'
            }`}
          >
            {item.label}
          </button>
        ))}
      </nav>
    </div>
  );
}
