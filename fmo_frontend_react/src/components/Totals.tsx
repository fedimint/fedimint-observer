import { useEffect, useState } from 'react';
import { api } from '../services/api';
import type { FedimintTotals } from '../types/api';
import { toBitcoin, formatNumber } from '../utils/format';
import { Alert } from './Alert';

export function Totals() {
  const [totals, setTotals] = useState<FedimintTotals | null>(null);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;
    api
      .getTotals(signal)
      .then((data) => {
        if (signal.aborted) return;
        setTotals(data);
        setLoading(false);
      })
      .catch((err) => {
        if (signal.aborted) return;
        console.error('Failed to fetch totals:', err);
        setFailed(true);
        setLoading(false);
      });
    return () => controller.abort();
  }, []);

  // Zeros would read as real figures
  if (failed) {
    return <Alert level="error" message="Could not load the totals. Please try again later." />;
  }

  return (
    <div className="grid grid-cols-3 gap-2 sm:gap-8 text-gray-900 dark:text-white px-4 sm:px-0">
      <div className="text-center py-2 sm:py-0">
        <div className="text-xl sm:text-4xl font-bold mb-1 sm:mb-2">
          {loading ? '...' : formatNumber(totals?.federations || 0)}
        </div>
        <div className="text-[10px] sm:text-base text-gray-600 dark:text-gray-400">
          <span className="sm:hidden">Federations</span>
          <span className="hidden sm:inline">Observed Federations</span>
        </div>
      </div>
      <div className="text-center border-l border-gray-300 dark:border-gray-600 py-2 sm:py-0">
        <div className="text-xl sm:text-4xl font-bold mb-1 sm:mb-2">
          {loading ? '...' : formatNumber(totals?.tx_count || 0)}
        </div>
        <div className="text-[10px] sm:text-base text-gray-600 dark:text-gray-400">Total Transactions</div>
      </div>
      <div className="text-center border-l border-gray-300 dark:border-gray-600 py-2 sm:py-0">
        <div className="text-xl sm:text-4xl font-bold mb-1 sm:mb-2 flex items-center justify-center gap-1">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="16"
            height="16"
            fill="currentColor"
            className="bi bi-currency-bitcoin shrink-0 sm:w-10 sm:h-10"
            viewBox="0 0 16 16"
          >
            <path d="M5.5 13v1.25c0 .138.112.25.25.25h1a.25.25 0 0 0 .25-.25V13h.5v1.25c0 .138.112.25.25.25h1a.25.25 0 0 0 .25-.25V13h.084c1.992 0 3.416-1.033 3.416-2.82 0-1.502-1.007-2.323-2.186-2.44v-.088c.97-.242 1.683-.974 1.683-2.19C11.997 3.93 10.847 3 9.092 3H9V1.75a.25.25 0 0 0-.25-.25h-1a.25.25 0 0 0-.25.25V3h-.573V1.75a.25.25 0 0 0-.25-.25H5.75a.25.25 0 0 0-.25.25V3l-1.998.011a.25.25 0 0 0-.25.25v.989c0 .137.11.25.248.25l.755-.005a.75.75 0 0 1 .745.75v5.505a.75.75 0 0 1-.75.75l-.748.011a.25.25 0 0 0-.25.25v1c0 .138.112.25.25.25zm1.427-8.513h1.719c.906 0 1.438.498 1.438 1.312 0 .871-.575 1.362-1.877 1.362h-1.28zm0 4.051h1.84c1.137 0 1.756.58 1.756 1.524 0 .953-.626 1.45-2.158 1.45H6.927z"></path>
          </svg>
          <span className="whitespace-nowrap">{loading ? '...' : toBitcoin(totals?.tx_volume || 0, 3)}</span>
        </div>
        <div className="text-[10px] sm:text-base text-gray-600 dark:text-gray-400">Total Volume</div>
      </div>
    </div>
  );
}
