import type { FederationOverview } from '../../../../types/api';
import { Alert } from '../../../../components/ui/Alert';
import { NumberedPagination } from '../../../../components/ui/NumberedPagination';
import { PageSizeSelect } from '../../../../components/ui/PageSizeSelect';
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

      <section className="overflow-hidden rounded-xl border border-line bg-surface shadow-[0_1px_2px_rgba(10,17,34,0.04)]">
        <div className="flex flex-wrap items-end justify-between gap-3 border-b border-line px-4 py-4 sm:px-6">
          <div>
            <h2 className="text-base font-medium text-ink">Wallet UTXO inventory</h2>
            <p className="mt-0.5 text-xs text-ink-3 sm:text-sm">
              One row per outpoint. Guardian agreement is shown alongside the reconstructed observer record.
            </p>
          </div>
          <div className="min-h-4 text-xs text-ink-3">
            {utxos.data && `${utxos.comparedIds.length} of ${utxos.guardians.length} guardians compared`}
          </div>
        </div>

        <UtxoNotices utxos={utxos} overview={overview} overviewFailed={overviewFailed} />
        <UtxoToolbar utxos={utxos} />
        <UtxoTable utxos={utxos} network={network} />
        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-line px-4 py-3 sm:px-6">
          <PageSizeSelect value={utxos.pageSize} onChange={utxos.setPageSize} disabled={utxos.loading} />
          <NumberedPagination label="UTXO pages" page={utxos.currentPage} pageCount={utxos.pageCount} onChange={utxos.setPage} />
        </div>
      </section>
    </>
  );
}
