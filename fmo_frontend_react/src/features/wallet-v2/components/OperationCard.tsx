import { useState } from 'react';
import { BUTTON_VARIANT } from '../../../components/ui/controls';
import { Icon } from '../../../components/ui/Icon';
import type { WalletV2OnChainTransaction } from '../../../types/api';
import { asBitcoin, shortMiddle } from '../../../utils/format';
import { explorerTxUrl } from '../../../utils/network';
import { muted } from '../../../components/ui/styles';
import { button } from '../lib/styles';

/** One on-chain wallet operation, expandable to its amounts, fee and transaction */
export function OperationCard({ transaction, network }: { transaction: WalletV2OnChainTransaction; network?: string }) {
  const deposit = transaction.output_msat >= transaction.input_msat;
  const difference = Math.abs(transaction.output_msat - transaction.input_msat);
  const url = explorerTxUrl(transaction.txid, network);
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);

  const copyTransaction = async () => {
    try {
      await navigator.clipboard.writeText(transaction.txid);
      setCopied(true);
      setCopyFailed(false);
    } catch {
      setCopyFailed(true);
    }
  };

  return (
    <details className="group rounded-lg border border-line bg-surface-2 open:border-accent">
      <summary className="flex cursor-pointer list-none items-center gap-3 p-3 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent [&::-webkit-details-marker]:hidden">
        <span aria-hidden="true" className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-xl font-semibold ${deposit ? 'bg-ok-soft text-ok-ink' : 'bg-serious-soft text-serious-ink'}`}>
          {deposit ? '↓' : '↗'}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-semibold text-ink">{deposit ? 'Deposit merged' : 'Withdrawal peeled'} <span className={muted}>#{transaction.index}</span></span>
          <span className={`mt-0.5 block text-xs ${muted}`}>Block {transaction.created.toLocaleString()}</span>
        </span>
        <span className="text-right" title="Net change in wallet balance, including the on-chain fee">
          <span className={`block text-sm font-semibold ${deposit ? 'text-ok-ink' : 'text-serious-ink'}`}>{deposit ? '+' : '−'}{asBitcoin(difference, 8)}</span>
          <span className={`mt-0.5 hidden text-xs sm:block ${muted}`}>Balance {asBitcoin(transaction.output_msat, 8)}</span>
        </span>
        <Icon name="chevronDown" className="size-4 text-ink-3 transition group-open:rotate-180" />
      </summary>
      <div className="border-t border-line p-3">
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
          <div className="min-w-0 rounded-lg bg-surface p-3"><p className={`text-xs ${muted}`}>Before</p><p className="mt-1 break-all text-xs font-semibold text-ink sm:text-sm">{asBitcoin(transaction.input_msat, 8)}</p></div>
          <span aria-hidden="true" className="text-ink-3">→</span>
          <div className="min-w-0 rounded-lg bg-surface p-3"><p className={`text-xs ${muted}`}>After</p><p className="mt-1 break-all text-xs font-semibold text-ink sm:text-sm">{asBitcoin(transaction.output_msat, 8)}</p></div>
        </div>
        <dl className="mt-4 grid grid-cols-2 gap-3 text-xs sm:grid-cols-3">
          <div className="min-w-0"><dt className={muted}>Transaction</dt><dd className="mt-1 break-all font-medium text-ink" title={transaction.txid}>{shortMiddle(transaction.txid, 10, 8)}</dd></div>
          <div><dt className={muted}>On-chain fee</dt><dd className="mt-1 font-medium text-ink">{asBitcoin(transaction.fee_msat, 8)}</dd></div>
          <div><dt className={muted}>Size</dt><dd className="mt-1 font-medium text-ink">{transaction.vbytes.toLocaleString()} vbytes</dd></div>
        </dl>
        <div className="mt-4 flex flex-wrap items-center gap-2">
          <button type="button" onClick={copyTransaction} className={`${button} ${BUTTON_VARIANT.secondary}`}>{copied ? 'Copied' : 'Copy transaction ID'}</button>
          {url && <a href={url} target="_blank" rel="noreferrer" className={`${button} ${BUTTON_VARIANT.secondary}`}>View on explorer ↗</a>}
          {copyFailed && <span role="status" className="text-xs text-crit-ink">Copy failed. Transaction ID: <span className="select-all break-all">{transaction.txid}</span></span>}
        </div>
      </div>
    </details>
  );
}
