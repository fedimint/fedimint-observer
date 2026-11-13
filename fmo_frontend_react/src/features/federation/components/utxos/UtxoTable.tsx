import type { ReactNode } from 'react';
import type { GuardianClaimedUtxoState } from '../../../../types/api';
import { asBitcoin, shortMiddle } from '../../../../utils/format';
import { explorerAddressUrl, explorerTxUrl } from '../../../../utils/network';
import { ExplorerLink, TxidLinkedText } from '../../../../components/ui/ExplorerLink';
import { shortOutpoint, UTXO_KIND_STYLES, type UtxoInventoryRow } from '../../lib/utxos';
import type { FederationUtxos } from '../../hooks/useFederationUtxos';

function dotClass(state: GuardianClaimedUtxoState | undefined): string {
  if (!state) return 'border border-gray-400 dark:border-gray-500';
  return state === 'spendable' || state === 'unconfirmed_change' ? 'bg-emerald-500' : 'bg-amber-400';
}

/** One dot per compared guardian: filled when it lists the output, and in what state */
function GuardianAgreement({ utxo, guardianIds }: { utxo: UtxoInventoryRow; guardianIds: number[] }) {
  if (guardianIds.length === 0) return null;
  const heldBy = guardianIds.filter((guardianId) => utxo.guardianStates.has(guardianId)).length;
  return (
    <div className="flex items-center gap-2" aria-label={`Listed by ${heldBy} of ${guardianIds.length} guardians`}>
      <div className="flex gap-1">
        {guardianIds.map((guardianId) => {
          const state = utxo.guardianStates.get(guardianId);
          return (
            <span
              key={guardianId}
              title={`Guardian ${guardianId}: ${state ? state.replaceAll('_', ' ') : 'not listed'}`}
              className={`h-2.5 w-2.5 rounded-full ${dotClass(state)}`}
            />
          );
        })}
      </div>
      <span className="text-xs tabular-nums text-gray-500 dark:text-gray-400">{heldBy}/{guardianIds.length}</span>
    </div>
  );
}

function UtxoRow({ utxo, guardianIds, network }: { utxo: UtxoInventoryRow; guardianIds: number[]; network: string | undefined }) {
  const style = UTXO_KIND_STYLES[utxo.kind];
  return (
    <tr className={`align-top transition-colors hover:bg-gray-50 dark:hover:bg-gray-700/40 ${utxo.kind === 'mismatch' ? 'bg-rose-50/50 dark:bg-rose-950/10' : ''}`}>
      <td className="px-4 py-2.5 sm:px-6">
        <ExplorerLink
          href={explorerTxUrl(utxo.outPoint, network)}
          className="block truncate font-mono text-xs text-blue-700 hover:underline dark:text-blue-400"
          title={utxo.outPoint}
        >
          {shortOutpoint(utxo.outPoint)}
        </ExplorerLink>
        {utxo.address && (
          <ExplorerLink
            href={explorerAddressUrl(utxo.address, network)}
            className="mt-0.5 block truncate font-mono text-[11px] text-gray-500 hover:underline dark:text-gray-400"
            title={utxo.address}
          >
            {shortMiddle(utxo.address, 10, 8)}
          </ExplorerLink>
        )}
        {utxo.detail && <p className={`mt-1 text-xs ${style.detail}`}><TxidLinkedText text={utxo.detail} network={network} /></p>}
      </td>
      <td className="hidden px-2 py-2.5 sm:table-cell">
        <GuardianAgreement utxo={utxo} guardianIds={guardianIds} />
      </td>
      <td className="hidden px-2 py-2.5 sm:table-cell">
        <span className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${style.pill}`}>{style.label}</span>
      </td>
      <td className="px-4 py-2.5 text-right sm:px-6">
        <div className="whitespace-nowrap font-mono text-xs tabular-nums text-gray-900 dark:text-white">
          {asBitcoin(utxo.amount, 8)}
        </div>
        <span className={`mt-1 inline-block rounded-full px-2 py-0.5 text-[11px] font-medium sm:hidden ${style.pill}`}>{style.label}</span>
      </td>
    </tr>
  );
}

function MessageRow({ children }: { children: ReactNode }) {
  return (
    <tr>
      <td colSpan={4} className="px-4 py-6 text-center text-sm text-gray-500 dark:text-gray-400">{children}</td>
    </tr>
  );
}

/** One row per outpoint, with guardian agreement, status and amount */
export function UtxoTable({ utxos, network }: { utxos: FederationUtxos; network: string | undefined }) {
  const { data, loading, error, rows, query, comparedIds } = utxos;
  return (
    <table className="w-full table-fixed text-left text-sm">
      <colgroup>
        <col />
        <col className="hidden w-32 sm:table-column" />
        <col className="hidden w-28 sm:table-column" />
        <col className="w-36 sm:w-40" />
      </colgroup>
      <thead className="border-b border-gray-200 bg-gray-50 text-[11px] font-medium uppercase tracking-wide text-gray-500 dark:border-gray-700 dark:bg-gray-900/40 dark:text-gray-400">
        <tr>
          <th scope="col" className="px-4 py-2 sm:px-6">Outpoint</th>
          <th scope="col" className="hidden px-2 py-2 sm:table-cell">Guardians</th>
          <th scope="col" className="hidden px-2 py-2 sm:table-cell">Status</th>
          <th scope="col" className="px-4 py-2 text-right sm:px-6">Amount</th>
        </tr>
      </thead>
      <tbody aria-busy={loading} className={`divide-y divide-gray-100 transition-opacity dark:divide-gray-700/70 ${loading && data ? 'opacity-60' : ''}`}>
        {loading && !data ? (
          <MessageRow>Loading UTXOs...</MessageRow>
        ) : error ? (
          <MessageRow>UTXOs unavailable</MessageRow>
        ) : rows.length === 0 ? (
          <MessageRow>{query ? `No UTXOs match "${query}"` : 'No UTXOs found'}</MessageRow>
        ) : (
          rows.map((utxo) => <UtxoRow key={utxo.outPoint} utxo={utxo} guardianIds={comparedIds} network={network} />)
        )}
      </tbody>
    </table>
  );
}
