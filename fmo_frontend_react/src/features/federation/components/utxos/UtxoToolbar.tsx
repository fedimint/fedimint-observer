import { UTXO_KIND_STYLES, UTXO_VIEWS } from '../../lib/utxos';
import type { FederationUtxos } from '../../hooks/useFederationUtxos';

/** Status filters with their counts, the search field, and which rows are shown */
export function UtxoToolbar({ utxos }: { utxos: FederationUtxos }) {
  const { counts, data, totalRows, pageStart, rows } = utxos;
  return (
    <div className="flex flex-col gap-3 border-b border-gray-200 px-4 py-3 dark:border-gray-700 sm:px-6 lg:flex-row lg:items-center lg:justify-between">
      <div className="flex flex-wrap gap-2" role="group" aria-label="Filter UTXOs by status">
        {UTXO_VIEWS.map((view) => {
          const active = utxos.view === view;
          const alert = view === 'mismatch' && (counts?.mismatch ?? 0) > 0;
          return (
            <button
              key={view}
              type="button"
              onClick={() => utxos.selectView(view)}
              aria-pressed={active}
              className={`inline-flex items-center gap-1.5 whitespace-nowrap rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
                active
                  ? 'border-blue-600 bg-blue-600 text-white'
                  : alert
                    ? 'border-rose-300 text-rose-700 hover:bg-rose-50 dark:border-rose-800 dark:text-rose-300 dark:hover:bg-rose-950/40'
                    : 'border-gray-300 text-gray-600 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700/50'
              }`}
            >
              <span>{view === 'all' ? 'All' : UTXO_KIND_STYLES[view].label}</span>
              {counts && <span className={`tabular-nums ${active ? 'text-white/80' : 'text-gray-400 dark:text-gray-500'}`}>{counts[view]}</span>}
            </button>
          );
        })}
      </div>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <input
          type="search"
          value={utxos.search}
          onChange={(event) => utxos.setSearch(event.target.value)}
          aria-label="Search UTXOs"
          placeholder="Search outpoint, txid, address or BTC amount"
          className="w-full rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 outline-hidden focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-900 dark:text-white sm:w-80"
        />
        <div className="whitespace-nowrap text-xs text-gray-500 dark:text-gray-400">
          {!data
            ? ''
            : totalRows === 0
              ? '0 UTXOs'
              : `${pageStart + 1}–${pageStart + rows.length} of ${totalRows}`}
        </div>
      </div>
    </div>
  );
}
