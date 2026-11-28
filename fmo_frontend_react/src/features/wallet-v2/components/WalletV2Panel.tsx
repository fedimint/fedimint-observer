import { useCallback, useState } from 'react';
import { Select } from '../../../components/ui/Select';
import { api } from '../../../services/api';
import type { WalletV2History } from '../../../types/api';
import { WalletV2Assets } from './WalletV2Assets';
import { CursorPager } from '../../../components/ui/CursorPager';
import { useCursorPages } from '../../../hooks/useCursorPages';
import { usePageSize } from '../../../hooks/usePageSize';
import type { HistoryError } from '../../../utils/walletHistory';

const REFRESH_MS = 60_000;
const nextBefore = (history: WalletV2History) => history.next_before;

/** `active` is false while another tab of the page is shown: the panel stays mounted with its data, but does not poll. */
export function WalletV2Panel({ id, instances, network, active }: { id: string; instances: number[]; network: string; active: boolean }) {
  const [selected, setSelected] = useState(() => Number(instances[0]));
  return <div className="space-y-4">
    {instances.length > 1 && <label className="inline-flex items-center gap-2 text-xs text-ink-3">Wallet module
      <Select value={selected} onChange={(event) => setSelected(Number(event.target.value))}>
        {instances.map((instance) => <option key={instance} value={instance}>Wallet #{instance}</option>)}
      </Select>
    </label>}
    <WalletInstance key={`${id}:${selected}`} id={id} instance={selected} network={network} active={active} />
  </div>;
}

function WalletInstance({ id, instance, network, active }: { id: string; instance: number; network?: string; active: boolean }) {
  const [pageSize, setPageSize] = usePageSize('wallet-v2-history', 25);
  const fetchPage = useCallback(
    (before: number | undefined, signal: AbortSignal) => api.getWalletV2OnChain(id, instance, pageSize, before, signal),
    [id, instance, pageSize],
  );
  const pages = useCursorPages({ fetchPage, nextCursor: nextBefore, refreshMs: REFRESH_MS, enabled: active, clearErrorOnRestart: true });
  const { page, latest } = pages;
  const error: HistoryError | null = pages.error && (pages.error.kind === 'refresh' ? 'refresh' : 'older');

  return <>
    {page?.accounting_pending && <p role="status" className="text-sm text-ink-3">Historical deposit accounting is catching up. Activity totals may still increase.</p>}
    {pages.onLaterPage && <p className="text-xs text-ink-3">Automatic refresh is paused while reading older operations. <button type="button" className="font-medium text-accent-ink hover:underline" onClick={pages.restart}>Show latest</button></p>}
    <WalletV2Assets balance={latest ? latest.balance_msat : undefined} transactions={page?.transactions ?? []} loaded={page !== null} error={error} network={network}
      observedAt={page?.observed_at} stale={page?.stale} balanceStale={latest?.balance_stale} balanceObservedAt={latest?.balance_observed_at}
      pager={page && <CursorPager label="Wallet operation pages" page={pages.pageNumber} hasNext={pages.hasNext} loading={pages.loadingPage} onPrevious={pages.previous} onNext={pages.next} pageSize={pageSize} onPageSizeChange={setPageSize} />} />
  </>;
}
