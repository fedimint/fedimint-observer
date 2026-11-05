import { useEffect, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { api } from '../services/api';
import type { ClientConfig, GatewayV2Registry } from '../types/api';

export function FederationGatewaysV2() {
  const { id } = useParams<{ id: string }>();
  const [config, setConfig] = useState<ClientConfig | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    if (!id) return;
    const controller = new AbortController();
    api.getFederationConfig(id, controller.signal).then((data) => { if (!controller.signal.aborted) setConfig(data); })
      .catch(() => { if (!controller.signal.aborted) setError(true); });
    return () => controller.abort();
  }, [id]);
  const instances = Object.entries(config?.modules ?? {}).filter(([, module]) => module.kind === 'lnv2');
  return <div className="space-y-5 py-6 text-gray-900 dark:text-white">
    <Link to={`/federations/${id}`} className="text-sm text-blue-600 dark:text-blue-400">← Back to federation</Link>
    <h1 className="text-2xl font-bold">Lightning v2 gateways</h1>
    <p className="text-sm text-gray-500 dark:text-gray-400">Endpoints registered with this federation’s guardians. Registration does not establish that a gateway is reachable or able to route a payment.</p>
    {error ? <p role="alert">Could not load federation configuration.</p> : !config ? <p>Loading…</p> : instances.length && id ? <GatewayModules id={id} instances={instances.map(([instance]) => Number(instance))} /> : <p>This federation has no Lightning v2 module.</p>}
  </div>;
}

function GatewayModules({ id, instances }: { id: string; instances: number[] }) {
  const [instance, setInstance] = useState(instances[0]);
  return <>
    {instances.length > 1 && <label className="block text-sm">Lightning module <select value={instance} onChange={(event) => setInstance(Number(event.target.value))} className="ml-2 rounded-lg border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800">{instances.map((id) => <option key={id} value={id}>Lightning #{id}</option>)}</select></label>}
    <GatewayRegistry key={instance} id={id} instance={instance} />
  </>;
}

function GatewayRegistry({ id, instance }: { id: string; instance: number }) {
  const [registry, setRegistry] = useState<GatewayV2Registry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [olderLoaded, setOlderLoaded] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    let inFlight = false;
    const refresh = async () => {
      if (inFlight || document.visibilityState === 'hidden') return;
      inFlight = true;
      try {
        const data = await api.getGatewayV2Registry(id, instance, undefined, controller.signal);
        if (!controller.signal.aborted) { setRegistry(data); setError(null); }
      } catch (error) {
        if (!controller.signal.aborted) setError(error instanceof Error ? error.message : 'Could not refresh gateways');
      } finally { inFlight = false; }
    };
    if (!olderLoaded) void refresh();
    const timer = window.setInterval(() => { if (!olderLoaded) void refresh(); }, 60_000);
    return () => { window.clearInterval(timer); controller.abort(); };
  }, [id, instance, olderLoaded]);

  const loadMore = async () => {
    if (!registry?.next_after || loadingMore) return;
    setLoadingMore(true);
    try {
      const next = await api.getGatewayV2Registry(id, instance, registry.next_after);
      setOlderLoaded(true);
      setRegistry((previous) => previous && ({ ...previous, next_after: next.next_after, gateways: [...previous.gateways, ...next.gateways] }));
    } catch (error) { setError(error instanceof Error ? error.message : 'Could not load more gateways'); }
    finally { setLoadingMore(false); }
  };

  return <>
    {error && <p role="alert" className="rounded-lg bg-amber-50 p-4 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-300">{error}. Existing registrations are the last successful observation.</p>}
    {!registry && !error && <p>Loading registrations…</p>}
    {registry && <>
      <div role="status" className="rounded-xl border border-gray-200 bg-white p-4 text-sm dark:border-gray-700 dark:bg-gray-800">
        <p>{registry.observed_at ? `Observed ${new Date(registry.observed_at).toLocaleString()}` : 'Collecting gateway registrations…'}</p>
        <p className="mt-1 text-gray-500 dark:text-gray-400">{registry.successful_guardians} of {registry.total_guardians} guardians answered the latest poll.</p>
        {registry.stale && registry.observed_at && <p className="mt-2 text-amber-700 dark:text-amber-300">Collection is delayed. Showing the last observed registrations.</p>}
        {registry.successful_guardians < registry.total_guardians && registry.observed_at && <p className="mt-2 text-gray-500 dark:text-gray-400">Partial coverage: previously registered endpoints are retained until every guardian answers.</p>}
      </div>
      {registry.observed_at && registry.gateways.length === 0 && <p>No registered gateways were reported in the latest poll.</p>}
      <div className="space-y-3">{registry.gateways.map((gateway) => <article key={gateway.api_endpoint} className="rounded-xl border border-gray-200 bg-white p-4 dark:border-gray-700 dark:bg-gray-800">
        <div className="flex flex-wrap items-start justify-between gap-3"><h2 className="break-all text-sm font-semibold">{gateway.api_endpoint}</h2><span className={`rounded-full px-2 py-1 text-xs ${gateway.registered ? 'bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-300' : 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300'}`}>{gateway.registered ? 'Registered' : 'Removed from registry'}</span></div>
        <dl className="mt-3 grid gap-2 text-xs text-gray-500 dark:text-gray-400 sm:grid-cols-3"><div><dt>Last reported by guardians</dt><dd>{gateway.announcing_guardians.map((id) => `#${id}`).join(', ')}</dd></div><div><dt>First seen</dt><dd>{new Date(gateway.first_seen).toLocaleString()}</dd></div><div><dt>Last seen</dt><dd>{new Date(gateway.last_seen).toLocaleString()}</dd></div></dl>
      </article>)}</div>
      {registry.next_after && <button type="button" disabled={loadingMore} onClick={() => { void loadMore(); }} className="rounded-lg bg-blue-600 px-4 py-2 text-sm text-white disabled:opacity-50">{loadingMore ? 'Loading…' : 'Load more gateways'}</button>}
      {olderLoaded && <button type="button" onClick={() => setOlderLoaded(false)} className="text-sm text-blue-600 dark:text-blue-400">Refresh registrations</button>}
    </>}
  </>;
}
