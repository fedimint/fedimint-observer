import { useEffect, useRef, useState } from 'react';
import { api } from '../services/api';
import type { ClientConfig, WalletV2History } from '../types/api';
import { WalletV2Assets } from './WalletV2Assets';
import { CursorPager } from './CursorPager';
import { PageRequests } from '../utils/pageRequests';
import { startAutoRefresh } from '../utils/autoRefresh';
import type { HistoryError } from '../utils/walletHistory';

const REFRESH_MS = 60_000;

/** `active` is false while another tab of the page is shown: the panel stays mounted with its data, but does not poll. */
export function WalletV2Panel({ id, modules, active }: { id: string; modules: NonNullable<ClientConfig['modules']>; active: boolean }) {
  const instances = Object.entries(modules).filter(([, config]) => config.kind === 'walletv2');
  const [selected, setSelected] = useState(() => Number(instances[0]?.[0]));
  return <div className="space-y-4">
    {instances.length > 1 && <label className="block text-sm text-gray-700 dark:text-gray-300">Wallet module
      <select value={selected} onChange={(event) => setSelected(Number(event.target.value))} className="ml-3 rounded-lg border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800">
        {instances.map(([instance]) => <option key={instance} value={instance}>Wallet #{instance}</option>)}
      </select>
    </label>}
    <WalletInstance key={`${id}:${selected}`} id={id} instance={selected} network={modules[String(selected)]?.network} active={active} />
  </div>;
}

function WalletInstance({ id, instance, network, active }: { id: string; instance: number; network?: string; active: boolean }) {
  // The newest page, kept current by the refresh, and the older page shown, if any
  const [latest, setLatest] = useState<WalletV2History | null>(null);
  const [older, setOlder] = useState<WalletV2History | null>(null);
  // The `before` cursor of each older page on the way to the one shown; page 1 has none
  const [cursors, setCursors] = useState<number[]>([]);
  const [error, setError] = useState<HistoryError | null>(null);
  const [loadingPage, setLoadingPage] = useState(false);
  const requests = useRef(new PageRequests());
  const lastSuccess = useRef<number | null>(null);
  const pageRequest = useRef<AbortController | null>(null);
  const onOlderPage = cursors.length > 0;
  const page = onOlderPage ? older : latest;

  useEffect(() => () => pageRequest.current?.abort(), []);

  useEffect(() => {
    // Keep an older page stable while it is read; going back to page 1
    // resumes automatic updates.
    if (!active || onOlderPage) return;
    const controller = new AbortController();
    const gate = requests.current;
    let inFlight = false;
    const refresh = async () => {
      if (inFlight) return;
      const generation = gate.refresh();
      if (generation === null) return;
      inFlight = true;
      try {
        const next = await api.getWalletV2OnChain(id, instance, undefined, controller.signal);
        if (!controller.signal.aborted && gate.acceptsRefresh(generation)) {
          lastSuccess.current = Date.now();
          setLatest(next);
          setError(null);
        }
      } catch {
        if (!controller.signal.aborted && gate.acceptsRefresh(generation)) setError('refresh');
      } finally {
        inFlight = false;
      }
    };
    const stop = startAutoRefresh(() => { void refresh(); }, REFRESH_MS, { lastSuccessAt: () => lastSuccess.current });
    return () => { stop(); controller.abort(); };
  }, [id, instance, onOlderPage, active]);

  const showLatest = () => {
    pageRequest.current?.abort();
    pageRequest.current = null;
    requests.current.reset();
    lastSuccess.current = null; // fetch the newest page now, however recent
    setLoadingPage(false);
    setCursors([]);
    setOlder(null);
    setError(null);
  };

  const showOlderPage = async (before: number, nextCursors: number[]) => {
    if (loadingPage) return;
    const generation = requests.current.beginPage();
    if (generation === null) return;
    const controller = new AbortController();
    pageRequest.current = controller;
    setLoadingPage(true);
    try {
      const next = await api.getWalletV2OnChain(id, instance, before, controller.signal);
      if (requests.current.finishPage(generation, true)) {
        setCursors(nextCursors);
        setOlder(next);
        setError(null);
      }
    } catch {
      if (requests.current.finishPage(generation, false)) setError('older');
    } finally {
      if (pageRequest.current === controller) {
        pageRequest.current = null;
        setLoadingPage(false);
      }
    }
  };

  const nextPage = () => {
    if (page?.next_before == null) return;
    void showOlderPage(page.next_before, [...cursors, page.next_before]);
  };

  const previousPage = () => {
    if (cursors.length <= 1) showLatest();
    else void showOlderPage(cursors[cursors.length - 2], cursors.slice(0, -1));
  };

  return <>
    {page?.accounting_pending && <p role="status" className="text-sm text-gray-500 dark:text-gray-400">Historical deposit accounting is catching up. Activity totals may still increase.</p>}
    {onOlderPage && <p className="text-xs text-gray-500 dark:text-gray-400">Automatic refresh is paused while reading older operations. <button type="button" className="text-blue-600 dark:text-blue-400" onClick={showLatest}>Show latest</button></p>}
    <WalletV2Assets balance={latest ? latest.balance_msat : undefined} transactions={page?.transactions ?? []} loaded={page !== null} error={error} network={network}
      observedAt={page?.observed_at} stale={page?.stale} balanceStale={latest?.balance_stale} balanceObservedAt={latest?.balance_observed_at}
      pager={page && <CursorPager label="Wallet operation pages" page={cursors.length + 1} hasNext={page.next_before != null} loading={loadingPage} onPrevious={previousPage} onNext={nextPage} />} />
  </>;
}
