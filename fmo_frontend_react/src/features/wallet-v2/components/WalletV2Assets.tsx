import { lazy, Suspense, useMemo, useState, type ReactNode } from 'react';
import type { WalletV2OnChainTransaction } from '../../../types/api';
import { asBitcoin } from '../../../utils/format';
import { formatUtcDateTime } from '../../../utils/time';
import { historyAlert, historyContent, type HistoryError } from '../../../utils/walletHistory';
import { card, muted } from '../../../components/ui/styles';
import { button } from '../lib/styles';
import { OperationCard } from './OperationCard';

const WalletV2ChainChart = lazy(() => import('./WalletV2ChainChart').then((module) => ({ default: module.WalletV2ChainChart })));

type MobileView = 'chart' | 'operations';
type OperationFilter = 'all' | 'deposits' | 'withdrawals';

interface WalletV2AssetsProps {
  balance: number | null | undefined;
  transactions: WalletV2OnChainTransaction[];
  /** A history page has loaded; it stays shown through later errors. */
  loaded: boolean;
  error: HistoryError | null;
  network?: string;
  observedAt?: string | null;
  stale?: boolean;
  balanceStale?: boolean;
  balanceObservedAt?: string | null;
  /** Previous/next page arrows, shown above and below the operations */
  pager?: ReactNode;
}


export function WalletV2Assets({ balance, transactions, loaded, error, network, observedAt, stale, balanceStale, balanceObservedAt, pager }: WalletV2AssetsProps) {
  const [mobileView, setMobileView] = useState<MobileView>('chart');
  const [filter, setFilter] = useState<OperationFilter>('all');
  const [newestFirst, setNewestFirst] = useState(true);
  const ordered = useMemo(() => [...transactions].sort((a, b) => b.index - a.index), [transactions]);
  const latest = ordered[0];
  const visible = ordered.filter((tx) => filter === 'all' || (filter === 'deposits' ? tx.output_msat >= tx.input_msat : tx.output_msat < tx.input_msat));
  if (!newestFirst) visible.reverse();
  const content = historyContent(loaded, error, ordered.length);
  const alert = historyAlert(loaded, error);
  const tile = (value: string | number) => content === 'loading' ? 'Loading…' : content === 'unavailable' ? 'Unavailable' : value;

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
          <div className="min-w-0 sm:text-right"><p className={`text-xs ${muted}`}>Current wallet balance</p><p className="mt-1 break-all text-2xl font-bold text-gray-900 dark:text-white sm:text-3xl">{tile(balance == null ? 'Awaiting balance' : asBitcoin(balance, 8))}</p>
            {content === 'ready' && latest && <p className={`mt-2 text-xs font-medium ${latest.output_msat >= latest.input_msat ? 'text-emerald-700 dark:text-emerald-400' : 'text-orange-700 dark:text-orange-400'}`}>Latest operation #{latest.index}: {latest.output_msat >= latest.input_msat ? '+' : '−'}{asBitcoin(Math.abs(latest.output_msat - latest.input_msat), 8)}</p>}
          </div>
        </div>
        <div className="mt-5 grid grid-cols-2 gap-3 border-t border-gray-200 pt-4 dark:border-gray-700 sm:grid-cols-3">
          <div><p className={`text-xs ${muted}`}>Operations on this page</p><p className="mt-1 text-sm font-semibold text-gray-900 dark:text-white">{tile(ordered.length)}</p></div>
          <div><p className={`text-xs ${muted}`}>Latest operation</p><p className="mt-1 text-sm font-semibold text-gray-900 dark:text-white">{tile(latest ? `#${latest.index}` : 'None yet')}</p></div>
          <div className="col-span-2 sm:col-span-1"><p className={`text-xs ${muted}`}>Latest operation block</p><p className="mt-1 text-sm font-semibold text-gray-900 dark:text-white">{tile(latest ? latest.created.toLocaleString() : '—')}</p></div>
        </div>
        <p className={`mt-3 text-xs ${muted}`}>History shows 20 operations per page.</p>
      </section>

      <div className="flex rounded-lg border border-gray-200 bg-gray-100 p-1 dark:border-gray-700 dark:bg-gray-800 sm:hidden" role="group" aria-label="Wallet details view">
        {(['chart', 'operations'] as const).map((view) => <button key={view} type="button" aria-pressed={mobileView === view} onClick={() => setMobileView(view)} className={`flex-1 ${button} ${mobileView === view ? 'bg-blue-600 text-white' : 'text-gray-600 dark:text-gray-300'}`}>{view === 'chart' ? 'Chart' : 'Operations'}</button>)}
      </div>

      {(observedAt || stale || balanceStale) && <p role="status" className={`text-xs ${muted}`}>
        {observedAt ? `History observed ${formatUtcDateTime(observedAt)}.` : 'Collecting wallet history.'}
        {stale && observedAt ? ' History collection is delayed; showing the last observation.' : ''}
        {balanceStale ? ` Balance collection is delayed${balanceObservedAt ? `; last observed ${formatUtcDateTime(balanceObservedAt)}` : ''}.` : ''}
      </p>}
      {alert && <div role="alert" className="rounded-lg border border-amber-300 bg-amber-50 p-4 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-900/20 dark:text-amber-300">{alert}</div>}

      <section className={`${card} p-4 sm:block sm:p-6 ${mobileView === 'chart' ? 'block' : 'hidden'}`} aria-labelledby="wallet-chart-title">
        <h4 id="wallet-chart-title" className="text-base font-semibold text-gray-900 dark:text-white">Balance history</h4>
        <p className={`mt-1 text-xs ${muted}`}>Balance after each of the latest on-chain operations.</p>
        {content === 'loading' ? <p className={`py-12 text-center text-sm ${muted}`}>Loading wallet history…</p> : content === 'unavailable' ? <p className={`py-12 text-center text-sm ${muted}`}>History is unavailable.</p> : content === 'empty' ? <p className={`py-12 text-center text-sm ${muted}`}>No on-chain operations recorded yet.</p> : <>
          <Suspense fallback={<div className={`flex h-[320px] items-center justify-center text-sm ${muted}`}>Loading chart…</div>}><WalletV2ChainChart transactions={ordered} /></Suspense>
          <div className={`flex flex-wrap gap-x-5 gap-y-2 text-xs ${muted}`}><span><span className="mr-2 inline-block h-2.5 w-2.5 rounded-full bg-emerald-500" />Deposit merged</span><span><span className="mr-2 inline-block h-2.5 w-2.5 rounded-full bg-orange-500" />Withdrawal peeled</span></div>
        </>}
      </section>

      <section className={`${card} p-4 sm:block sm:p-6 ${mobileView === 'operations' ? 'block' : 'hidden'}`} aria-labelledby="wallet-operations-title">
        <div className="flex flex-wrap items-center justify-between gap-3"><div><h4 id="wallet-operations-title" className="text-base font-semibold text-gray-900 dark:text-white">Recent on-chain operations</h4><p className={`mt-1 text-xs ${muted}`}>Select an operation for its details.</p></div><button type="button" onClick={() => setNewestFirst(!newestFirst)} className={`${button} border border-gray-200 text-gray-600 hover:bg-gray-100 dark:border-gray-700 dark:text-gray-300 dark:hover:bg-gray-700`}>{newestFirst ? 'Newest first ↓' : 'Oldest first ↑'}</button></div>
        <div className="mt-4 flex flex-wrap gap-2" role="group" aria-label="Filter operations">{(['all', 'deposits', 'withdrawals'] as const).map((item) => <button key={item} type="button" aria-pressed={filter === item} onClick={() => setFilter(item)} className={`${button} ${filter === item ? 'bg-blue-600 text-white' : 'bg-gray-100 text-gray-600 hover:bg-gray-200 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600'}`}>{item === 'all' ? 'All' : item === 'deposits' ? 'Deposits' : 'Withdrawals'}</button>)}</div>
        <div className="mt-4 space-y-2">{content === 'loading' ? <p className={`py-6 text-center text-sm ${muted}`}>Loading on-chain history…</p> : content === 'unavailable' ? <p className={`py-6 text-center text-sm ${muted}`}>History is unavailable.</p> : visible.length === 0 ? <p className={`py-6 text-center text-sm ${muted}`}>{ordered.length ? 'No operations match this filter.' : 'No on-chain operations recorded yet.'}</p> : visible.map((transaction) => <OperationCard key={`${transaction.index}-${transaction.txid}`} transaction={transaction} network={network} />)}</div>
        {pager && <div className="mt-4">{pager}</div>}
      </section>

    </div>
  );
}
