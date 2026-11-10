import type { FederationOverview } from '../../../../types/api';
import { Alert } from '../../../../components/ui/Alert';
import { NumberedPagination } from '../../../../components/ui/NumberedPagination';
import type { FederationUtxos } from '../../hooks/useFederationUtxos';
import { UtxoNotices } from './UtxoNotices';
import { UtxoToolbar } from './UtxoToolbar';
import { UtxoTable } from './UtxoTable';

interface UtxoInventoryProps {
  utxos: FederationUtxos;
  overview: FederationOverview | null;
  overviewFailed: boolean;
  network: string | undefined;
}

/** The observer's reconstructed UTXO set, compared output by output with the guardians' wallets */
export function UtxoInventory({ utxos, overview, overviewFailed, network }: UtxoInventoryProps) {
  return (
    <>
      <Alert
        level="info"
        message="This view compares reconstructed observer history with guardian wallet data. Results may change while history is catching up."
      />

      <section className="mt-5 overflow-hidden border border-gray-200 bg-white dark:border-gray-700 dark:bg-gray-800">
        <div className="flex flex-wrap items-end justify-between gap-3 border-b border-gray-200 px-4 py-4 dark:border-gray-700 sm:px-6">
          <div>
            <h2 className="text-base font-semibold text-gray-950 dark:text-white">Wallet UTXO Inventory</h2>
            <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
              One row per outpoint. Guardian agreement is shown alongside the reconstructed observer record.
            </p>
          </div>
          {utxos.data && (
            <div className="text-xs text-gray-500 dark:text-gray-400">
              {utxos.comparedIds.length} of {utxos.guardians.length} guardians compared
            </div>
          )}
        </div>

        <UtxoNotices utxos={utxos} overview={overview} overviewFailed={overviewFailed} />
        <UtxoToolbar utxos={utxos} />
        <UtxoTable utxos={utxos} network={network} />
        <NumberedPagination label="UTXO pages" page={utxos.currentPage} pageCount={utxos.pageCount} onChange={utxos.setPage} />
      </section>
    </>
  );
}
