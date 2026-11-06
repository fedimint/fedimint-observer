import { lazy, Suspense, useEffect, useMemo, useRef, useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import type { EChartsOption } from 'echarts';
import { api } from '../services/api';
import type { ClientConfig, GatewayV2Registry, GatewayV2RoutingInfo, GatewayWindow } from '../types/api';
import { Copyable } from '../components/Copyable';
import { useTheme } from '../hooks/useTheme';
import { asBitcoin, shortMiddle } from '../utils/format';
import { PageRequests } from '../utils/pageRequests';

const TrendChart = lazy(() => import('../components/UptimeTrendChart').then((module) => ({ default: module.UptimeTrendChart })));
const windows: GatewayWindow[] = ['24h', '7d', '30d', '90d'];
type Gateway = GatewayV2Registry['gateways'][number];
type Status = Gateway['status'];
const muted = 'text-gray-500 dark:text-gray-400';
const panel = 'rounded-xl border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800';
const statusColors: Record<Status, string> = {
  online: 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300',
  degraded: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300',
  offline: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300',
  unknown: 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300',
  removed: 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300',
};
const gatewayName = (gateway: Gateway) => gateway.routing_info?.lightning_alias || `Gateway ${shortMiddle(gateway.routing_info?.module_public_key ?? gateway.api_endpoint.replace(/^\w+:\/\//, ''), 8, 6)}`;
const date = (value: string | null) => value ? new Date(value).toLocaleString() : 'Not yet observed';
const fee = (value?: GatewayV2RoutingInfo['receive_fee']) => value ? `${(value.base / 1000).toLocaleString(undefined, { maximumFractionDigits: 3 })} sat + ${value.parts_per_million.toLocaleString()} ppm` : 'Awaiting node details';

export function FederationGatewaysV2() {
  const { id = '' } = useParams<{ id: string }>();
  const [params, setParams] = useSearchParams();
  const selectedWindow = params.get('window');
  const timeWindow = windows.includes(selectedWindow as GatewayWindow) ? selectedWindow as GatewayWindow : '7d';
  const [config, setConfig] = useState<ClientConfig | null>(null);
  const [name, setName] = useState('Federation');
  const [error, setError] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    Promise.all([api.getFederationConfig(id, controller.signal), api.getFederations(controller.signal).catch(() => [])])
      .then(([config, federations]) => {
        if (controller.signal.aborted) return;
        setConfig(config);
        setName(federations.find((fed) => fed.id === id)?.name || 'Federation');
      }).catch(() => { if (!controller.signal.aborted) setError(true); });
    return () => controller.abort();
  }, [id]);
  const instances = Object.entries(config?.modules ?? {}).filter(([, module]) => module.kind === 'lnv2').map(([instance]) => Number(instance));
  return <div className="py-6 text-gray-900 dark:text-white">
    <Link to={`/federations/${id}`} className="text-sm text-blue-600 hover:underline dark:text-blue-400">← Back to {name}</Link>
    <div className="mb-6 mt-4 flex flex-col justify-between gap-4 sm:flex-row sm:items-end">
      <div><div className="flex flex-wrap items-center gap-3"><h1 className="text-2xl font-bold sm:text-3xl">Gateway Details</h1><span className="rounded-full bg-blue-100 px-2.5 py-1 text-xs font-semibold text-blue-700 dark:bg-blue-900/40 dark:text-blue-300">Lightning v2</span></div><p className={`mt-2 text-sm ${muted}`}>Gateway health, routing fees and observed contract activity for {name}.</p></div>
      <div className="flex rounded-lg border border-gray-200 bg-white p-1 dark:border-gray-700 dark:bg-gray-800" role="group" aria-label="Gateway history window">
        {windows.map((window) => <button key={window} type="button" aria-pressed={timeWindow === window} onClick={() => setParams((previous) => { const next = new URLSearchParams(previous); next.set('window', window); return next; })} className={`rounded-md px-4 py-2 text-xs font-semibold ${timeWindow === window ? 'bg-blue-600 text-white' : 'text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-700'}`}>{window.toUpperCase()}</button>)}
      </div>
    </div>
    {error ? <p role="alert" className={`${panel} p-5`}>Could not load federation configuration. Reload to try again.</p> : !config ? <p className={muted}>Loading gateway details…</p> : instances.length ? <GatewayModules id={id} instances={instances} timeWindow={timeWindow} /> : <p>This federation has no Lightning v2 module.</p>}
  </div>;
}

function GatewayModules({ id, instances, timeWindow }: { id: string; instances: number[]; timeWindow: GatewayWindow }) {
  const [instance, setInstance] = useState(instances[0]);
  return <>
    {instances.length > 1 && <label className="mb-4 block text-sm">Lightning module <select value={instance} onChange={(event) => setInstance(Number(event.target.value))} className="ml-2 rounded-lg border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800">{instances.map((id) => <option key={id} value={id}>Lightning #{id}</option>)}</select></label>}
    <GatewayDirectory key={`${id}:${instance}:${timeWindow}`} id={id} instance={instance} timeWindow={timeWindow} />
  </>;
}

function GatewayDirectory({ id, instance, timeWindow }: { id: string; instance: number; timeWindow: GatewayWindow }) {
  const [registry, setRegistry] = useState<GatewayV2Registry | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [olderLoaded, setOlderLoaded] = useState(false);
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState<Status | 'all'>('all');
  const [sort, setSort] = useState('name');
  const [reload, setReload] = useState(0);
  const requests = useRef(new PageRequests());
  useEffect(() => {
    const controller = new AbortController();
    const gate = requests.current;
    let inFlight = false;
    const refresh = async () => {
      if (inFlight || document.visibilityState === 'hidden' || olderLoaded) return;
      const generation = gate.refresh();
      if (generation === null) return;
      inFlight = true;
      try {
        const data = await api.getGatewayV2Registry(id, instance, timeWindow, undefined, controller.signal);
        if (!controller.signal.aborted && gate.acceptsRefresh(generation)) { setRegistry(data); setError(null); }
      } catch (error) { if (!controller.signal.aborted && gate.acceptsRefresh(generation)) setError(error instanceof Error ? error.message : 'Could not refresh gateways'); }
      finally { inFlight = false; }
    };
    void refresh();
    const timer = window.setInterval(() => { void refresh(); }, 60_000);
    return () => { window.clearInterval(timer); controller.abort(); gate.reset(); };
  }, [id, instance, timeWindow, olderLoaded, reload]);
  const gateways = registry?.gateways ?? [];
  const totals = { online: 0, degraded: 0, offline: 0, unknown: 0, removed: 0 };
  for (const gateway of gateways) totals[gateway.status]++;
  const samples = gateways.reduce((sum, gateway) => sum + gateway.samples, 0);
  const successful = gateways.reduce((sum, gateway) => sum + gateway.online_samples, 0);
  const latencies = gateways.filter((gateway) => gateway.status === 'online' && gateway.latency_ms !== null).map((gateway) => gateway.latency_ms!).sort((a, b) => a - b);
  const medianLatency = latencies.length ? (latencies[Math.floor((latencies.length - 1) / 2)] + latencies[Math.floor(latencies.length / 2)]) / 2 : null;
  const query = search.trim().toLowerCase();
  const visible = gateways.filter((gateway) => (filter === 'all' ? gateway.registered : gateway.status === filter) && `${gatewayName(gateway)} ${gateway.routing_info?.lightning_public_key ?? ''} ${gateway.api_endpoint}`.toLowerCase().includes(query));
  visible.sort((left, right) => sort === 'latency' ? (left.latency_ms ?? Infinity) - (right.latency_ms ?? Infinity) : sort === 'activity' ? (right.activity.incoming_contracts + right.activity.outgoing_contracts) - (left.activity.incoming_contracts + left.activity.outgoing_contracts) : sort === 'status' ? left.status.localeCompare(right.status) : gatewayName(left).localeCompare(gatewayName(right)));
  const loadMore = async () => {
    if (!registry?.next_after || loadingMore) return;
    const generation = requests.current.beginPage();
    if (generation === null) return;
    setLoadingMore(true);
    try {
      const next = await api.getGatewayV2Registry(id, instance, timeWindow, registry.next_after);
      if (!requests.current.finishPage(generation, true)) return;
      setOlderLoaded(true);
      setRegistry({ ...registry, next_after: next.next_after, gateways: [...registry.gateways, ...next.gateways] });
      setLoadingMore(false);
      setError(null);
    } catch (error) {
      if (requests.current.finishPage(generation, false)) {
        setError(error instanceof Error ? error.message : 'Could not load more gateways');
        setLoadingMore(false);
      }
    }
  };
  return <>
    {error && <p role="alert" className="mb-4 rounded-lg bg-amber-50 p-4 text-sm text-amber-800 dark:bg-amber-900/20 dark:text-amber-300">{error}. <button type="button" className="underline" onClick={() => { requests.current.reset(); setLoadingMore(false); setOlderLoaded(false); setReload((count) => count + 1); }}>Try again</button></p>}
    <div className="mb-4 grid grid-cols-2 gap-3 lg:grid-cols-4">
      <Metric label="Gateways shown" value={registry ? gateways.filter((gateway) => gateway.registered).length.toLocaleString() : '—'} detail={registry?.next_after ? 'More gateways available below' : 'Registered with the federation'} />
      <Metric label="Online now" value={registry ? String(totals.online) : '—'} detail={`${totals.offline} offline · ${totals.unknown} unknown`} />
      <Metric label={`${timeWindow.toUpperCase()} probe success`} value={samples ? `${(successful / samples * 100).toFixed(1)}%` : '—'} detail={samples ? `${samples.toLocaleString()} recorded checks` : 'History starts with the first probe'} />
      <Metric label="Median response" value={medianLatency === null ? '—' : `${medianLatency.toLocaleString()} ms`} detail="Latest successful API checks" />
    </div>
    {registry && !registry.activity_caught_up && <p role="status" className={`mb-3 text-xs ${muted}`}>Historical contract activity is being indexed. Totals may increase as the backfill catches up.</p>}
    <AvailabilityTrend gateways={gateways} timeWindow={timeWindow} />
    {registry && <div className={`mb-4 flex flex-wrap items-center gap-x-4 gap-y-1 text-xs ${muted}`}><span>Registry: {registry.successful_guardians}/{registry.total_guardians} guardians answered</span><span>Updated {date(registry.observed_at)}</span>{registry.stale && <span className="text-amber-600 dark:text-amber-300">Registry collection delayed</span>}{registry.successful_guardians < registry.total_guardians && <span>Previous registrations are retained during partial coverage.</span>}</div>}
    <section className={`${panel} overflow-hidden`}>
      <div className="border-b border-gray-200 p-4 dark:border-gray-700 sm:p-5">
        <div className="flex flex-col justify-between gap-3 lg:flex-row lg:items-center"><div><h2 className="text-lg font-semibold">Gateway directory</h2><p className={`mt-1 text-xs ${muted}`}>{visible.length} of {gateways.length} loaded gateways · {timeWindow.toUpperCase()} observed history</p></div><div className="flex flex-col gap-2 sm:flex-row"><input value={search} onChange={(event) => setSearch(event.target.value)} aria-label="Search gateways" placeholder="Search gateway, node or endpoint" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm outline-hidden focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-900" /><select value={sort} onChange={(event) => setSort(event.target.value)} aria-label="Sort gateways" className="rounded-lg border border-gray-300 bg-white px-3 py-2 text-sm dark:border-gray-600 dark:bg-gray-900"><option value="name">Gateway name</option><option value="status">Status</option><option value="latency">Response time</option><option value="activity">Contract activity</option></select></div></div>
        <div className="mt-4 flex flex-wrap gap-2" role="group" aria-label="Filter gateways by status">{(['all', 'online', 'degraded', 'offline', 'unknown', 'removed'] as const).map((status) => <button key={status} type="button" aria-pressed={filter === status} onClick={() => setFilter(status)} className={`rounded-full border px-3 py-1.5 text-xs font-medium capitalize ${filter === status ? 'border-blue-600 bg-blue-600 text-white' : 'border-gray-300 text-gray-600 dark:border-gray-600 dark:text-gray-300'}`}>{status === 'all' ? 'All active' : status} ({status === 'all' ? gateways.filter((gateway) => gateway.registered).length : totals[status]})</button>)}</div>
      </div>
      {!registry && !error && <p className={`p-8 text-center text-sm ${muted}`}>Loading gateway details…</p>}
      {registry && visible.length === 0 && <p className={`p-8 text-center text-sm ${muted}`}>{gateways.length ? 'No gateways match this view.' : registry.observed_at ? 'No registered gateways have been reported.' : 'Collecting gateway registrations…'}</p>}
      <div className="divide-y divide-gray-200 dark:divide-gray-700">{visible.map((gateway) => <GatewayRow key={gateway.api_endpoint} gateway={gateway} timeWindow={timeWindow} />)}</div>
      {registry?.next_after && <button type="button" disabled={loadingMore} onClick={() => { void loadMore(); }} className="m-4 rounded-lg bg-blue-600 px-4 py-2 text-sm text-white disabled:opacity-50">{loadingMore ? 'Loading…' : 'Load more gateways'}</button>}
    </section>
    <p className={`mt-3 text-xs ${muted}`}>Online means the gateway answered a routing-info check and supports this federation. Contract counts describe public federation activity, not payment routing capacity.</p>
    {olderLoaded && <button type="button" onClick={() => { requests.current.reset(); setLoadingMore(false); setOlderLoaded(false); }} className="mt-2 text-sm text-blue-600 dark:text-blue-400">Refresh all loaded results</button>}
  </>;
}

function Metric({ label, value, detail }: { label: string; value: string; detail: string }) {
  return <div className={`${panel} p-4`}><p className={`text-xs ${muted}`}>{label}</p><p className="mt-2 text-2xl font-bold">{value}</p><p className={`mt-1 text-xs ${muted}`}>{detail}</p></div>;
}

function AvailabilityTrend({ gateways, timeWindow }: { gateways: Gateway[]; timeWindow: GatewayWindow }) {
  const { isDark } = useTheme();
  const buckets = useMemo(() => Array.from({ length: 30 }, (_, index) => ({
    start: gateways[0]?.availability[index]?.start,
    samples: gateways.reduce((sum, gateway) => sum + (gateway.availability[index]?.samples ?? 0), 0),
    online: gateways.reduce((sum, gateway) => sum + (gateway.availability[index]?.online_samples ?? 0), 0),
  })), [gateways]);
  const hasSamples = buckets.some((bucket) => bucket.samples > 0);
  const option = useMemo<EChartsOption>(() => ({
    animation: false,
    grid: { left: 42, right: 20, top: 15, bottom: 30 },
    tooltip: { trigger: 'axis', valueFormatter: (value) => value === null ? 'No observations' : `${Number(value).toFixed(1)}%` },
    xAxis: { type: 'category', data: buckets.map((bucket) => bucket.start ? new Date(bucket.start).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit' }) : ''), axisLabel: { color: isDark ? '#9ca3af' : '#6b7280', fontSize: 10 } },
    yAxis: { type: 'value', min: 0, max: 100, axisLabel: { formatter: '{value}%', color: isDark ? '#9ca3af' : '#6b7280', fontSize: 10 }, splitLine: { lineStyle: { color: isDark ? '#374151' : '#e5e7eb', type: 'dashed' } } },
    series: [{ name: 'Successful gateway checks', type: 'line', data: buckets.map((bucket) => bucket.samples ? bucket.online / bucket.samples * 100 : null), connectNulls: false, symbol: 'circle', symbolSize: 5, lineStyle: { color: '#6366f1', width: 2 }, itemStyle: { color: '#6366f1' }, areaStyle: { color: 'rgba(99,102,241,0.10)' } }],
  }), [buckets, isDark]);
  return <section className={`${panel} mb-4 p-4 sm:p-5`}><div className="mb-3 flex flex-wrap items-center justify-between gap-2"><h2 className="text-base font-semibold">Gateway availability trend ({timeWindow.toUpperCase()})</h2><span className={`text-xs ${muted}`}>Historical API checks · gaps mean no observation</span></div>{hasSamples ? <Suspense fallback={<p className={`flex h-[210px] items-center justify-center text-sm ${muted}`}>Loading chart…</p>}><TrendChart option={option} /></Suspense> : <div className={`flex h-[210px] flex-col items-center justify-center gap-2 text-center text-sm ${muted}`}><span>No probe history yet</span><span className="text-xs">Availability will appear as gateway checks are recorded.</span></div>}</section>;
}

function GatewayRow({ gateway, timeWindow }: { gateway: Gateway; timeWindow: GatewayWindow }) {
  const routing = gateway.routing_info;
  const activity = gateway.activity;
  return <article className="p-4 sm:p-5">
    <div className="grid gap-4 lg:grid-cols-4">
      <div><h3 className="truncate font-semibold" title={gatewayName(gateway)}>{gatewayName(gateway)}</h3><p className={`mt-1 text-xs ${muted}`}>{gateway.api_endpoint.startsWith('iroh:') ? 'Iroh' : 'HTTPS'} gateway</p><p className={`mt-2 break-all font-mono text-xs ${muted}`} title={routing?.lightning_public_key}>Node: {routing ? shortMiddle(routing.lightning_public_key, 10, 8) : 'Awaiting node details'}</p></div>
      <div><div className="flex items-center justify-between gap-2"><span className={`rounded-full px-2.5 py-1 text-xs font-semibold capitalize ${statusColors[gateway.status]}`}>{gateway.status}</span><span className={`text-xs ${muted}`}>{gateway.latency_ms === null ? '—' : `${gateway.latency_ms} ms`}</span></div><p className="mt-2 text-sm font-semibold">{gateway.samples ? `${(gateway.online_samples / gateway.samples * 100).toFixed(1)}% successful checks` : 'No probe history'}</p><div className="mt-2 flex gap-0.5" role="img" aria-label={`${timeWindow} availability: ${gateway.online_samples} successful of ${gateway.samples} probes`}>{gateway.availability.map((bucket, index) => <span key={index} className={`h-5 min-w-0 flex-1 rounded-xs ${!bucket.samples ? 'bg-gray-200 dark:bg-gray-600' : bucket.online_samples === bucket.samples ? 'bg-green-500' : bucket.online_samples ? 'bg-yellow-500' : 'bg-red-500'}`} title={`${date(bucket.start)}: ${bucket.online_samples}/${bucket.samples} checks succeeded`} />)}</div><p className={`mt-1 text-xs ${muted}`}>{gateway.samples.toLocaleString()} probes · oldest → newest</p></div>
      <div className="text-xs"><p className={`mb-2 ${muted}`}>Routing fees</p><p>Send: <span className="font-medium">{fee(routing?.send_fee_default)}</span></p><p className="mt-2">Receive: <span className="font-medium">{fee(routing?.receive_fee)}</span></p><p className={`mt-2 ${muted}`}>Probed {date(gateway.probed_at)}</p></div>
      <div className="text-xs"><p className={`mb-2 ${muted}`}>Contract activity ({timeWindow.toUpperCase()})</p>{routing ? <><p><span className="font-semibold">{activity.incoming_contracts.toLocaleString()}</span> incoming · <span className="font-semibold">{activity.outgoing_contracts.toLocaleString()}</span> outgoing</p><p className={`mt-2 ${muted}`}>{activity.outgoing_claimed} claimed · {activity.outgoing_refunded} refunded · {activity.outgoing_cancelled} cancelled</p></> : <p className={muted}>Waiting for gateway identity to attribute activity.</p>}</div>
    </div>
    <details className="mt-4 border-t border-gray-100 pt-3 dark:border-gray-700"><summary className="cursor-pointer text-xs font-medium text-blue-600 dark:text-blue-400">Gateway details</summary><div className="mt-4 grid gap-4 text-xs sm:grid-cols-2"><div><p className={`mb-1 ${muted}`}>Endpoint</p><Copyable text={gateway.api_endpoint} /></div><div><p className={`mb-1 ${muted}`}>Lightning node public key</p>{routing ? <Copyable text={routing.lightning_public_key} /> : <p>Awaiting node details</p>}</div><div><p className={`mb-1 ${muted}`}>Federation module key</p>{routing ? <Copyable text={routing.module_public_key} /> : <p>Awaiting node details</p>}</div><div><p className={muted}>Last reported by guardians</p><p className="mt-1">{gateway.announcing_guardians.map((id) => `#${id}`).join(', ')}</p></div><div><p className={muted}>First seen / last registered</p><p className="mt-1">{date(gateway.first_seen)} / {date(gateway.last_seen)}</p></div><div><p className={muted}>Node details observed</p><p className="mt-1">{date(gateway.routing_observed_at)}</p></div>{routing && <><div><p className={muted}>Minimum send fee / expiry delta</p><p className="mt-1">{fee(routing.send_fee_minimum)} / {routing.expiration_delta_minimum} blocks</p></div><div><p className={muted}>Funded contract volume</p><p className="mt-1">Incoming {asBitcoin(activity.incoming_msat, 8)} · Outgoing {asBitcoin(activity.outgoing_msat, 8)}</p></div></>}</div><p className={`mt-4 text-xs ${muted}`}>Activity counts contracts funded in this window and their observed outcomes. A successful API probe does not verify Lightning liquidity.</p></details>
  </article>;
}
