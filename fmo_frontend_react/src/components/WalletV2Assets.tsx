import { lazy, Suspense, useMemo, useState } from 'react';
import type { WalletV2OnChainTransaction } from '../types/api';
import { asBitcoin, shortMiddle } from '../utils/format';

const WalletV2ChainChart = lazy(() => import('./WalletV2ChainChart').then((module) => ({ default: module.WalletV2ChainChart })));

type MobileView = 'chart' | 'operations';
type OperationFilter = 'all' | 'deposits' | 'withdrawals';

interface WalletV2AssetsProps {
  balance: number | null | undefined;
  transactions: WalletV2OnChainTransaction[];
  loading: boolean;
  error: string | null;
  network?: string;
  observedAt?: string | null;
  stale?: boolean;
  balanceStale?: boolean;
  balanceObservedAt?: string | null;
  hasMore?: boolean;
  loadingMore?: boolean;
  onLoadMore?: () => void;
}

const card = 'rounded-xl border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800';
const muted = 'text-gray-500 dark:text-gray-400';
const button = 'rounded-lg px-3 py-2 text-xs font-medium transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-blue-500';

function explorerUrl(txid: string, network?: string): string | null {
  const paths: Record<string, string> = { bitcoin: '', mainnet: '', testnet: '/testnet', testnet4: '/testnet4', signet: '/signet' };
  const path = paths[network?.toLowerCase() ?? ''];
  return path === undefined ? null : `https://mempool.space${path}/tx/${txid}`;
}

function OperationCard({ transaction, network }: { transaction: WalletV2OnChainTransaction; network?: string }) {
  const deposit = transaction.output_msat >= transaction.input_msat;
  const difference = Math.abs(transaction.output_msat - transaction.input_msat);
  const url = explorerUrl(transaction.txid, network);
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

export function WalletV2Assets({ balance, transactions, loading, error, network, observedAt, stale, balanceStale, balanceObservedAt, hasMore, loadingMore, onLoadMore }: WalletV2AssetsProps) {
  const [mobileView, setMobileView] = useState<MobileView>('chart');
  const [filter, setFilter] = useState<OperationFilter>('all');
  const [newestFirst, setNewestFirst] = useState(true);
  const ordered = useMemo(() => [...transactions].sort((a, b) => b.index - a.index), [transactions]);
  const latest = ordered[0];
  const visible = ordered.filter((tx) => filter === 'all' || (filter === 'deposits' ? tx.output_msat >= tx.input_msat : tx.output_msat < tx.input_msat));
  if (!newestFirst) visible.reverse();

  return (
    <div className="space-y-4">
      <section className={`${card} p-4 sm:p-6`} aria-labelledby="wallet-details-title">
        <h4 id="wallet-details-title" className="text-base font-semibold text-gray-900 dark:text-white">About this wallet</h4>
        <p className={`mt-2 text-sm ${muted}`}>Wallet v2 tracks a single federation UTXO. Deposits are merged into it, and withdrawals are peeled from it. The chart and operations below use the latest guardian-reported wallet history. Operations may still be pending on Bitcoin.</p>
      </section>

      <section className={`${card} p-4 sm:p-6`} aria-labelledby="wallet-v2-title">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="flex flex-wrap items-center gap-2"><h3 id="wallet-v2-title" className="text-lg font-bold text-gray-900 dark:text-white">Federation wallet</h3><span className="rounded-full bg-blue-100 px-2 py-0.5 text-xs font-medium text-blue-700 dark:bg-blue-900/50 dark:text-blue-300">Wallet v2</span></div>
            <p className={`mt-1 max-w-xl text-sm ${muted}`}>Deposits and withdrawals are reflected in the federation’s on-chain wallet.</p>
          </div>
          <div className="min-w-0 sm:text-right"><p className={`text-xs ${muted}`}>Current wallet balance</p><p className="mt-1 break-all text-2xl font-bold text-gray-900 dark:text-white sm:text-3xl">{balance === undefined ? 'Loading…' : balance === null ? 'Awaiting balance' : asBitcoin(balance, 8)}</p>
            {!loading && !error && latest && <p className={`mt-2 text-xs font-medium ${latest.output_msat >= latest.input_msat ? 'text-emerald-700 dark:text-emerald-400' : 'text-orange-700 dark:text-orange-400'}`}>Latest operation #{latest.index}: {latest.output_msat >= latest.input_msat ? '+' : '−'}{asBitcoin(Math.abs(latest.output_msat - latest.input_msat), 8)}</p>}
          </div>
        </div>
        <div className="mt-5 grid grid-cols-2 gap-3 border-t border-gray-200 pt-4 dark:border-gray-700 sm:grid-cols-3">
          <div><p className={`text-xs ${muted}`}>Loaded operations</p><p className="mt-1 text-sm font-semibold text-gray-900 dark:text-white">{loading ? 'Loading…' : error ? 'Unavailable' : ordered.length}</p></div>
          <div><p className={`text-xs ${muted}`}>Latest operation</p><p className="mt-1 text-sm font-semibold text-gray-900 dark:text-white">{loading ? 'Loading…' : error ? 'Unavailable' : latest ? `#${latest.index}` : 'None yet'}</p></div>
          <div className="col-span-2 sm:col-span-1"><p className={`text-xs ${muted}`}>Latest operation block</p><p className="mt-1 text-sm font-semibold text-gray-900 dark:text-white">{loading ? 'Loading…' : error ? 'Unavailable' : latest ? latest.created.toLocaleString() : '—'}</p></div>
        </div>
        <p className={`mt-3 text-xs ${muted}`}>History loads 20 operations at a time.</p>
      </section>

      <div className="flex rounded-lg border border-gray-200 bg-gray-100 p-1 dark:border-gray-700 dark:bg-gray-800 sm:hidden" role="group" aria-label="Wallet details view">
        {(['chart', 'operations'] as const).map((view) => <button key={view} type="button" aria-pressed={mobileView === view} onClick={() => setMobileView(view)} className={`flex-1 ${button} ${mobileView === view ? 'bg-blue-600 text-white' : 'text-gray-600 dark:text-gray-300'}`}>{view === 'chart' ? 'Chart' : 'Operations'}</button>)}
      </div>

      {(observedAt || stale || balanceStale) && <p role="status" className={`text-xs ${muted}`}>
        {observedAt ? `History observed ${new Date(observedAt).toLocaleString()}.` : 'Collecting wallet history.'}
        {stale && observedAt ? ' History collection is delayed; showing the last observation.' : ''}
        {balanceStale ? ` Balance collection is delayed${balanceObservedAt ? `; last observed ${new Date(balanceObservedAt).toLocaleString()}` : ''}.` : ''}
      </p>}
      {error && <div role="alert" className="rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-900/20 dark:text-amber-300">Wallet history could not refresh. Any displayed data is the last successful observation.</div>}

      <section className={`${card} p-4 sm:block sm:p-6 ${mobileView === 'chart' ? 'block' : 'hidden'}`} aria-labelledby="wallet-chart-title">
        <h4 id="wallet-chart-title" className="text-base font-semibold text-gray-900 dark:text-white">Balance history</h4>
        <p className={`mt-1 text-xs ${muted}`}>Balance after each of the latest on-chain operations.</p>
        {loading ? <p className={`py-12 text-center text-sm ${muted}`}>Loading wallet history…</p> : error ? <p className={`py-12 text-center text-sm ${muted}`}>History is unavailable.</p> : ordered.length === 0 ? <p className={`py-12 text-center text-sm ${muted}`}>No on-chain operations recorded yet.</p> : <>
          <Suspense fallback={<div className={`flex h-[320px] items-center justify-center text-sm ${muted}`}>Loading chart…</div>}><WalletV2ChainChart transactions={ordered} /></Suspense>
          <div className={`flex flex-wrap gap-x-5 gap-y-2 text-xs ${muted}`}><span><span className="mr-2 inline-block h-2.5 w-2.5 rounded-full bg-emerald-500" />Deposit merged</span><span><span className="mr-2 inline-block h-2.5 w-2.5 rounded-full bg-orange-500" />Withdrawal peeled</span></div>
        </>}
      </section>

      <section className={`${card} p-4 sm:block sm:p-6 ${mobileView === 'operations' ? 'block' : 'hidden'}`} aria-labelledby="wallet-operations-title">
        <div className="flex flex-wrap items-center justify-between gap-3"><div><h4 id="wallet-operations-title" className="text-base font-semibold text-gray-900 dark:text-white">Recent on-chain operations</h4><p className={`mt-1 text-xs ${muted}`}>Select an operation for its details.</p></div><button type="button" onClick={() => setNewestFirst(!newestFirst)} className={`${button} border border-gray-200 text-gray-600 hover:bg-gray-100 dark:border-gray-700 dark:text-gray-300 dark:hover:bg-gray-700`}>{newestFirst ? 'Newest first ↓' : 'Oldest first ↑'}</button></div>
        <div className="mt-4 flex flex-wrap gap-2" role="group" aria-label="Filter operations">{(['all', 'deposits', 'withdrawals'] as const).map((item) => <button key={item} type="button" aria-pressed={filter === item} onClick={() => setFilter(item)} className={`${button} ${filter === item ? 'bg-blue-600 text-white' : 'bg-gray-100 text-gray-600 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600'}`}>{item === 'all' ? 'All' : item === 'deposits' ? 'Deposits' : 'Withdrawals'}</button>)}</div>
        <div className="mt-4 space-y-2">{loading ? <p className={`py-6 text-center text-sm ${muted}`}>Loading on-chain history…</p> : error ? <p className={`py-6 text-center text-sm ${muted}`}>History is unavailable.</p> : visible.length === 0 ? <p className={`py-6 text-center text-sm ${muted}`}>{ordered.length ? 'No operations match this filter.' : 'No on-chain operations recorded yet.'}</p> : visible.map((transaction) => <OperationCard key={`${transaction.index}-${transaction.txid}`} transaction={transaction} network={network} />)}</div>
        {hasMore && <button type="button" disabled={loadingMore} onClick={onLoadMore} className={`${button} mt-4 bg-blue-600 text-white disabled:opacity-50`}>{loadingMore ? 'Loading…' : 'Load older operations'}</button>}
      </section>

    </div>
  );
}
