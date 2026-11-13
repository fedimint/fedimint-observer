import { useState } from 'react';
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
    <details className="group rounded-lg border border-gray-200 bg-gray-50 open:border-blue-400 dark:border-gray-700 dark:bg-gray-900/40 dark:open:border-blue-500">
      <summary className="flex cursor-pointer list-none items-center gap-3 p-3 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-500 [&::-webkit-details-marker]:hidden">
        <span aria-hidden="true" className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg text-xl font-semibold ${deposit ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/50 dark:text-emerald-400' : 'bg-orange-100 text-orange-700 dark:bg-orange-900/40 dark:text-orange-400'}`}>
          {deposit ? '↓' : '↗'}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-semibold text-gray-900 dark:text-white">{deposit ? 'Deposit merged' : 'Withdrawal peeled'} <span className={muted}>#{transaction.index}</span></span>
          <span className={`mt-0.5 block text-xs ${muted}`}>Block {transaction.created.toLocaleString()}</span>
        </span>
        <span className="text-right" title="Net change in wallet balance, including the on-chain fee">
          <span className={`block text-sm font-semibold ${deposit ? 'text-emerald-700 dark:text-emerald-400' : 'text-orange-700 dark:text-orange-400'}`}>{deposit ? '+' : '−'}{asBitcoin(difference, 8)}</span>
          <span className={`mt-0.5 hidden text-xs sm:block ${muted}`}>Balance {asBitcoin(transaction.output_msat, 8)}</span>
        </span>
        <svg aria-hidden="true" className="h-4 w-4 shrink-0 text-gray-500 transition group-open:rotate-180" viewBox="0 0 20 20" fill="none"><path d="m5 7.5 5 5 5-5" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" /></svg>
      </summary>
      <div className="border-t border-gray-200 p-3 dark:border-gray-700">
        <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-2">
          <div className="min-w-0 rounded-lg bg-white p-3 dark:bg-gray-800"><p className={`text-xs ${muted}`}>Before</p><p className="mt-1 break-all text-xs font-semibold text-gray-900 dark:text-white sm:text-sm">{asBitcoin(transaction.input_msat, 8)}</p></div>
          <span aria-hidden="true" className="text-gray-400">→</span>
          <div className="min-w-0 rounded-lg bg-white p-3 dark:bg-gray-800"><p className={`text-xs ${muted}`}>After</p><p className="mt-1 break-all text-xs font-semibold text-gray-900 dark:text-white sm:text-sm">{asBitcoin(transaction.output_msat, 8)}</p></div>
        </div>
        <dl className="mt-4 grid grid-cols-2 gap-3 text-xs sm:grid-cols-3">
          <div className="min-w-0"><dt className={muted}>Transaction</dt><dd className="mt-1 break-all font-medium text-gray-900 dark:text-white" title={transaction.txid}>{shortMiddle(transaction.txid, 10, 8)}</dd></div>
          <div><dt className={muted}>On-chain fee</dt><dd className="mt-1 font-medium text-gray-900 dark:text-white">{asBitcoin(transaction.fee_msat, 8)}</dd></div>
          <div><dt className={muted}>Size</dt><dd className="mt-1 font-medium text-gray-900 dark:text-white">{transaction.vbytes.toLocaleString()} vbytes</dd></div>
        </dl>
        <div className="mt-4 flex flex-wrap items-center gap-2">
          <button type="button" onClick={copyTransaction} className={`${button} border border-gray-300 text-gray-700 hover:bg-gray-100 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-700`}>{copied ? 'Copied' : 'Copy transaction ID'}</button>
          {url && <a href={url} target="_blank" rel="noreferrer" className={`${button} border border-blue-300 text-blue-700 hover:bg-blue-50 dark:border-blue-700 dark:text-blue-400 dark:hover:bg-gray-700`}>View on explorer ↗</a>}
          {copyFailed && <span role="status" className="text-xs text-red-600 dark:text-red-400">Copy failed. Transaction ID: <span className="select-all break-all">{transaction.txid}</span></span>}
        </div>
      </div>
    </details>
  );
}
