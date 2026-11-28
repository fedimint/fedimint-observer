import { CONTROL_SIZE, TOGGLE_OFF, TOGGLE_ON } from '../../../../components/ui/controls';
import { SearchInput } from '../../../../components/ui/SearchInput';
import { UTXO_KIND_STYLES, UTXO_VIEWS } from '../../lib/utxos';
import type { FederationUtxos } from '../../hooks/useFederationUtxos';

/** Status filters with their counts, the search field, and which rows are shown */
export function UtxoToolbar({ utxos }: { utxos: FederationUtxos }) {
  const { counts, data, totalRows, pageStart, rows } = utxos;
  return (
    <div className="flex flex-col gap-3 border-b border-line px-4 py-3 sm:px-6 lg:flex-row lg:items-center lg:justify-between">
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
              className={`inline-flex items-center gap-1.5 whitespace-nowrap rounded-lg border font-medium transition-colors ${CONTROL_SIZE.sm} ${
                active ? TOGGLE_ON : alert ? 'border-crit/30 bg-surface text-crit-ink hover:bg-crit-soft' : TOGGLE_OFF
              }`}
            >
              <span>{view === 'all' ? 'All' : UTXO_KIND_STYLES[view].label}</span>
              {counts && <span className={`tabular-nums ${active ? 'text-accent-ink/70' : 'text-ink-3'}`}>{counts[view]}</span>}
            </button>
          );
        })}
      </div>
      <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
        <SearchInput value={utxos.search} onChange={utxos.setSearch} label="Search UTXOs" placeholder="Search outpoint, txid, address or BTC amount" className="w-full sm:w-80" />
        <div className="min-h-4 whitespace-nowrap text-xs text-ink-3">
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
