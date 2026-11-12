import type { GatewayWindow } from '../../../types/api';
import { GATEWAY_V2_WINDOWS } from '../lib/gatewaysV2';

/** The history window, as a row of toggle buttons */
export function GatewayV2WindowPicker({ value, onChange }: { value: GatewayWindow; onChange: (window: GatewayWindow) => void }) {
  return (
    <div className="flex rounded-lg border border-gray-200 bg-white p-1 dark:border-gray-700 dark:bg-gray-800" role="group" aria-label="Gateway history window">
      {GATEWAY_V2_WINDOWS.map((window) => (
        <button
          key={window}
          type="button"
          aria-pressed={value === window}
          onClick={() => onChange(window)}
          className={`rounded-md px-4 py-2 text-xs font-semibold ${value === window ? 'bg-blue-600 text-white' : 'text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-700'}`}
        >
          {window.toUpperCase()}
        </button>
      ))}
    </div>
  );
}
