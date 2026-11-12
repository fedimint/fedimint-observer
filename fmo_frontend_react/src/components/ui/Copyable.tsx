import { useState } from 'react';

interface CopyableProps {
  text: string;
}

export function Copyable({ text }: CopyableProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy text:', err);
    }
  };

  return (
    <div className="flex items-center gap-2">
      <input
        className="bg-gray-50 dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-lg px-2 sm:px-3 py-1.5 sm:py-2 text-xs sm:text-sm text-gray-900 dark:text-white flex-1 min-w-0 truncate"
        value={text}
        readOnly
        title={text}
      />
      <button
        onClick={handleCopy}
        className="shrink-0 text-gray-900 bg-white hover:bg-gray-100 border border-gray-300 focus:ring-4 focus:outline-hidden focus:ring-gray-100 rounded-lg text-sm p-1.5 sm:p-2 text-center inline-flex items-center dark:focus:ring-gray-600 dark:bg-gray-800 dark:border-gray-700 dark:text-white dark:hover:bg-gray-700"
        title={copied ? 'Copied!' : 'Copy to clipboard'}
      >
        {copied ? (
          <svg
            className="w-4 h-4 sm:w-[18px] sm:h-[18px] text-green-600 dark:text-green-400"
            aria-hidden="true"
            xmlns="http://www.w3.org/2000/svg"
            width="24"
            height="24"
            fill="none"
            viewBox="0 0 24 24"
          >
            <path
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2.3"
              d="M5 11.917 9.724 16.5 19 7.5"
            />
          </svg>
        ) : (
          <svg
            className="w-4 h-4 sm:w-[18px] sm:h-[18px] text-gray-800 dark:text-white"
            aria-hidden="true"
            xmlns="http://www.w3.org/2000/svg"
            width="24"
            height="24"
            fill="none"
            viewBox="0 0 24 24"
          >
            <path
              stroke="currentColor"
              strokeLinejoin="round"
              strokeWidth="2.3"
              d="M9 8v3a1 1 0 0 1-1 1H5m11 4h2a1 1 0 0 0 1-1V5a1 1 0 0 0-1-1h-7a1 1 0 0 0-1 1v1m4 3v10a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-7.13a1 1 0 0 1 .24-.65L7.7 8.35A1 1 0 0 1 8.46 8H13a1 1 0 0 1 1 1Z"
            />
          </svg>
        )}
      </button>
    </div>
  );
}
