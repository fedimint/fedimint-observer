import { useEffect, useRef, useState } from 'react';
import { api } from '../services/api';
import type { ClientConfig, WalletV2History } from '../types/api';
import { WalletV2Assets } from './WalletV2Assets';
import { PageRequests } from '../utils/pageRequests';

export function WalletV2Panel({ id, modules }: { id: string; modules: NonNullable<ClientConfig['modules']> }) {
  const instances = Object.entries(modules).filter(([, config]) => config.kind === 'walletv2');
  const [selected, setSelected] = useState(() => Number(instances[0]?.[0]));
  return <div className="space-y-4">
    {instances.length > 1 && <label className="block text-sm text-gray-700 dark:text-gray-300">Wallet module
      <select value={selected} onChange={(event) => setSelected(Number(event.target.value))} className="ml-3 rounded-lg border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800">
        {instances.map(([instance]) => <option key={instance} value={instance}>Wallet #{instance}</option>)}
      </select>
    </label>}
    <WalletInstance key={`${id}:${selected}`} id={id} instance={selected} network={modules[String(selected)]?.network} />
  </div>;
}

function WalletInstance({ id, instance, network }: { id: string; instance: number; network?: string }) {
  const [page, setPage] = useState<WalletV2History | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [olderLoaded, setOlderLoaded] = useState(false);
  const requests = useRef(new PageRequests());

  useEffect(() => {
    const controller = new AbortController();
    const gate = requests.current;
    let inFlight = false;
    const refresh = async () => {
      if (inFlight || document.visibilityState === 'hidden') return;
      const generation = gate.refresh();
      if (generation === null) return;
      inFlight = true;
      try {
        const next = await api.getWalletV2OnChain(id, instance, undefined, controller.signal);
        if (!controller.signal.aborted && gate.acceptsRefresh(generation)) { setPage(next); setError(null); }
      } catch (error) {
        if (!controller.signal.aborted && gate.acceptsRefresh(generation)) setError(error instanceof Error ? error.message : 'Could not refresh history');
      } finally {
        inFlight = false;
        if (!controller.signal.aborted) setLoading(false);
      }
    };
    // Keep a paginated view stable while reading older entries. Reopening this
    // tab starts at the newest page and resumes automatic updates.
    if (olderLoaded) return () => { controller.abort(); gate.reset(); };
    void refresh();
    const timer = window.setInterval(() => { void refresh(); }, 60_000);
    return () => { window.clearInterval(timer); controller.abort(); gate.reset(); };
  }, [id, instance, olderLoaded]);

  const loadMore = async () => {
    if (page?.next_before == null || loadingMore) return;
    const generation = requests.current.beginPage();
    if (generation === null) return;
    setLoadingMore(true);
    try {
      const next = await api.getWalletV2OnChain(id, instance, page.next_before);
      if (!requests.current.finishPage(generation, true)) return;
      setOlderLoaded(true);
      setPage({ ...page, next_before: next.next_before, transactions: [...page.transactions, ...next.transactions] });
      setLoadingMore(false);
      setError(null);
    } catch (error) {
      if (requests.current.finishPage(generation, false)) {
        setError(error instanceof Error ? error.message : 'Could not load older operations');
        setLoadingMore(false);
      }
    }
  };

  return <>
    {page?.accounting_pending && <p role="status" className="text-sm text-gray-500 dark:text-gray-400">Historical deposit accounting is catching up. Activity totals may still increase.</p>}
    {olderLoaded && <p className="text-xs text-gray-500 dark:text-gray-400">Automatic refresh is paused while reading older operations. <button type="button" className="text-blue-600 dark:text-blue-400" onClick={() => { requests.current.reset(); setLoadingMore(false); setOlderLoaded(false); }}>Show latest</button></p>}
    <WalletV2Assets balance={page ? page.balance_msat : undefined} transactions={page?.transactions ?? []} loading={loading} error={error} network={network}
      observedAt={page?.observed_at} stale={page?.stale} balanceStale={page?.balance_stale} balanceObservedAt={page?.balance_observed_at}
      hasMore={page?.next_before != null} loadingMore={loadingMore} onLoadMore={() => { void loadMore(); }} />
  </>;
}
