import { lazy, Suspense, useEffect, useMemo, useState, type ReactNode } from 'react';
import { Link, useParams } from 'react-router-dom';
import { api } from '../services/api';
import { Copyable } from '../components/Copyable';
import type {
  GatewayInfo,
  GatewayProbeReport,
  GatewayWindow,
  ProbeOutcome,
} from '../types/api';
import { formatMsats, formatNumber, formatRelative } from '../utils/format';

const ProbeTrendChart = lazy(() => import('echarts-for-react'));
const WINDOWS: GatewayWindow[] = ['24h', '7d', '30d', '90d'];

interface OutcomeInfo {
  label: string;
  description: string;
  classes: string;
}

const OUTCOMES: Record<ProbeOutcome, OutcomeInfo> = {
  success: {
    label: 'Reached gateway',
    description: 'The probe reached the gateway node, which rejected the unknown payment hash. The gateway can receive this amount.',
    classes: 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300',
  },
  insufficient_liquidity: {
    label: 'Insufficient liquidity',
    description: 'The channel into the gateway could not forward this amount.',
    classes: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300',
  },
  unreachable: {
    label: 'Unreachable',
    description: 'The gateway node was offline or its channels were disabled.',
    classes: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300',
  },
  route_failure: {
    label: 'Route failure',
    description: 'An intermediate node on the route failed the probe.',
    classes: 'bg-orange-100 text-orange-800 dark:bg-orange-900/40 dark:text-orange-300',
  },
  no_route: {
    label: 'No route',
    description: 'No route to the gateway node could be found.',
    classes: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300',
  },
  local_failure: {
    label: 'Prober liquidity',
    description: "Failed in the prober's own channels. Excluded from the gateway's score.",
    classes: 'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300',
  },
  timeout: {
    label: 'Timed out',
    description: 'The probe did not resolve in time. Excluded from the gateway\'s score.',
    classes: 'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300',
  },
  error: {
    label: 'Prober error',
    description: 'The prober failed to send the probe. Excluded from the gateway\'s score.',
    classes: 'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-300',
  },
};

function OutcomeBadge({ outcome }: { outcome: ProbeOutcome }) {
  const info = OUTCOMES[outcome];
  return (
    <span title={info.description} className={`inline-block cursor-help whitespace-nowrap rounded-full px-2 py-0.5 text-xs font-medium ${info.classes}`}>
      {info.label}
    </span>
  );
}

function parseDate(value?: string | null): Date | null {
  if (!value) return null;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? null : parsed;
}

function formatLatency(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return '—';
  if (ms >= 10_000) return `${(ms / 1000).toFixed(0)} s`;
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.round(ms)} ms`;
}

function formatDateTime(date: Date | null): string {
  if (!date) return 'N/A';
  return date.toLocaleString('en-US', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function StatTile({ label, value, detail }: { label: string; value: ReactNode; detail?: ReactNode }) {
  return (
    <div className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800">
      <div className="text-xs font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">{label}</div>
      <div className="mt-1.5 text-2xl font-semibold tracking-tight text-gray-950 dark:text-white">{value}</div>
      {detail && <div className="mt-1 text-xs text-gray-500 dark:text-gray-400">{detail}</div>}
    </div>
  );
}

export function GatewayDetail() {
  const { id, gatewayId } = useParams<{ id: string; gatewayId: string }>();
  const [timeWindow, setTimeWindow] = useState<GatewayWindow>('7d');
  const [federationName, setFederationName] = useState<string | null>(null);
  const [gateway, setGateway] = useState<GatewayInfo | null>(null);
  const [gatewayError, setGatewayError] = useState<string | null>(null);
  const [report, setReport] = useState<GatewayProbeReport | null>(null);
  const [reportError, setReportError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    api.getFederations()
      .then((federations) => {
        if (!cancelled) setFederationName(federations.find((fed) => fed.id === id)?.name ?? null);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [id]);

  useEffect(() => {
    if (!id || !gatewayId) return;
    let cancelled = false;
    setLoading(true);

    Promise.allSettled([
      api.getFederationGateway(id, gatewayId, timeWindow),
      api.getGatewayProbes(id, gatewayId, timeWindow),
    ]).then(([gatewayResult, reportResult]) => {
      if (cancelled) return;
      if (gatewayResult.status === 'fulfilled') {
        setGateway(gatewayResult.value);
        setGatewayError(null);
      } else {
        setGateway(null);
        setGatewayError(gatewayResult.reason instanceof Error ? gatewayResult.reason.message : 'Failed to load gateway');
      }
      if (reportResult.status === 'fulfilled') {
        setReport(reportResult.value);
        setReportError(null);
      } else {
        setReport(null);
        setReportError(reportResult.reason instanceof Error ? reportResult.reason.message : 'Failed to load probe results');
      }
      setLoading(false);
    });

    return () => {
      cancelled = true;
    };
  }, [id, gatewayId, timeWindow]);

  const summary = report?.summary ?? null;
  const trend = useMemo(() => report?.trend ?? [], [report]);
  const hourlyBuckets = timeWindow === '24h' || timeWindow === '1h';

  const trendOption = useMemo(() => ({
    animationDuration: 200,
    grid: { top: 30, right: 54, bottom: 30, left: 46 },
    legend: { top: 0, right: 0, textStyle: { color: '#6b7280', fontSize: 11 }, itemWidth: 12, itemHeight: 8 },
    tooltip: { trigger: 'axis' },
    xAxis: {
      type: 'category',
      data: trend.map((point) => {
        const date = new Date(point.bucket);
        return hourlyBuckets
          ? date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })
          : date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
      }),
      axisLabel: { color: '#6b7280', fontSize: 11 },
      axisLine: { lineStyle: { color: '#d1d5db' } },
    },
    yAxis: [
      { type: 'value', min: 0, max: 100, axisLabel: { formatter: '{value}%', color: '#6b7280', fontSize: 11 }, splitLine: { lineStyle: { color: '#e5e7eb' } } },
      { type: 'value', min: 0, axisLabel: { formatter: (value: number) => formatLatency(value), color: '#6b7280', fontSize: 11 }, splitLine: { show: false } },
    ],
    series: [
      {
        name: 'Routing success',
        type: 'bar',
        barMaxWidth: 18,
        data: trend.map((point) => (point.success_rate_pct === null ? null : Number(point.success_rate_pct.toFixed(1)))),
        itemStyle: { color: '#2563eb', borderRadius: [3, 3, 0, 0] },
        tooltip: { valueFormatter: (value: number | null) => (value === null || value === undefined ? 'no data' : `${value}%`) },
      },
      {
        name: 'Median latency',
        type: 'line',
        yAxisIndex: 1,
        smooth: true,
        connectNulls: true,
        symbol: trend.length > 45 ? 'none' : 'circle',
        symbolSize: 5,
        data: trend.map((point) => (point.latency_p50_ms === null ? null : Math.round(point.latency_p50_ms))),
        lineStyle: { color: '#d97706', width: 2 },
        itemStyle: { color: '#d97706' },
        tooltip: { valueFormatter: (value: number | null) => formatLatency(value) },
      },
    ],
  }), [trend, hourlyBuckets]);

  if (loading && !gateway && !report) {
    return (
      <div className="flex min-h-[400px] items-center justify-center">
        <div className="h-12 w-12 animate-spin rounded-full border-b-2 border-blue-600" />
      </div>
    );
  }

  const name = gateway?.lightning_alias || 'Unnamed Gateway';
  const uptime = gateway?.uptime_window;
  const activity = gateway?.activity_window;
  const lastProbe = parseDate(summary?.last_probe_time);
  const hasProbes = Boolean(summary && (summary.conclusive_probes > 0 || summary.inconclusive_probes > 0 || Object.keys(summary.outcome_counts).length > 0));
  const outcomeEntries = Object.entries(summary?.outcome_counts ?? {}) as [ProbeOutcome, number][];

  return (
    <div className="py-5 sm:py-8">
      <Link to={`/federations/${id}/gateways`} className="text-sm text-blue-700 hover:underline dark:text-blue-300">← All gateways{federationName ? ` of ${federationName}` : ''}</Link>

      <header className="mb-4 mt-4 rounded-2xl border border-slate-200 bg-gradient-to-br from-white to-blue-50 p-5 shadow-sm dark:border-gray-700 dark:from-gray-800 dark:to-blue-950/30 sm:p-6">
        <div className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
          <div className="min-w-0">
            <div className="text-xs font-semibold uppercase tracking-[0.16em] text-blue-700 dark:text-blue-300">Lightning gateway</div>
            <h1 className="mt-2 flex flex-wrap items-center gap-3 break-words text-3xl font-bold tracking-tight text-gray-950 dark:text-white">
              {name}
              {gateway?.vetted && <span className="rounded-full bg-indigo-100 px-2.5 py-0.5 text-xs font-semibold text-indigo-800 dark:bg-indigo-900/50 dark:text-indigo-200">Vetted</span>}
            </h1>
            {gateway && (
              <p className="mt-1 text-sm text-gray-600 dark:text-gray-400">
                First seen {formatDateTime(parseDate(gateway.first_seen))} · last seen {formatRelative(parseDate(gateway.last_seen))}
              </p>
            )}
          </div>
          <div className="rounded-xl bg-slate-100 p-1 dark:bg-gray-900">
            <div className="flex gap-1">
              {WINDOWS.map((window) => (
                <button
                  key={window}
                  type="button"
                  disabled={loading}
                  onClick={() => setTimeWindow(window)}
                  className={`rounded-lg px-3 py-2 text-sm font-semibold ${timeWindow === window ? 'bg-white text-blue-700 shadow-sm dark:bg-gray-700 dark:text-blue-300' : 'text-gray-600 dark:text-gray-400'} ${loading ? 'cursor-wait opacity-70' : ''}`}
                >
                  {window.toUpperCase()}
                </button>
              ))}
            </div>
            <div className="px-2 pt-1 text-right text-[11px] text-gray-500">{loading ? 'Refreshing…' : `Observed over ${timeWindow.toUpperCase()}`}</div>
          </div>
        </div>
      </header>

      {gatewayError && !gateway && (
        <div className="mb-4 rounded-xl border border-yellow-300 bg-yellow-50 p-4 text-sm text-yellow-900 dark:border-yellow-800 dark:bg-yellow-950/40 dark:text-yellow-200">
          This gateway is not in Observer's gateway history for this federation yet, so no details are available. ({gatewayError})
        </div>
      )}

      {gateway && (
        <>
          <section className="mb-4 grid gap-3 rounded-xl border border-gray-200 bg-white p-4 shadow-sm dark:border-gray-700 dark:bg-gray-800 sm:p-5 lg:grid-cols-3">
            <div className="min-w-0">
              <div className="mb-1 text-xs font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">Gateway ID</div>
              <Copyable text={gateway.gateway_id} />
            </div>
            <div className="min-w-0">
              <div className="mb-1 text-xs font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">LN node public key</div>
              <Copyable text={gateway.node_pub_key} />
            </div>
            <div className="min-w-0">
              <div className="mb-1 text-xs font-medium uppercase tracking-wide text-gray-500 dark:text-gray-400">API endpoint</div>
              <Copyable text={gateway.api_endpoint} />
            </div>
          </section>

          <section className="mb-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
            <StatTile
              label="Registry uptime"
              value={uptime && uptime.sample_count > 0 ? `${uptime.uptime_pct.toFixed(1)}%` : '—'}
              detail={uptime && uptime.sample_count > 0 ? `${formatNumber(uptime.seen_samples)} of ${formatNumber(uptime.sample_count)} registry polls` : 'No registry polls in window'}
            />
            <StatTile
              label="Payments funded"
              value={formatNumber(activity?.fund_count ?? 0)}
              detail="LN contracts using this gateway"
            />
            <StatTile
              label="Settled / cancelled"
              value={`${formatNumber(activity?.settle_count ?? 0)} / ${formatNumber(activity?.cancel_count ?? 0)}`}
            />
            <StatTile
              label="Volume"
              value={formatMsats(activity?.total_volume_msat ?? 0)}
            />
          </section>
        </>
      )}

      <section className="mb-4 rounded-xl border border-gray-200 bg-white shadow-md dark:border-gray-700 dark:bg-gray-800" aria-labelledby="routing-probes-heading">
        <div className="border-b border-gray-200 p-4 dark:border-gray-700 sm:p-5">
          <h2 id="routing-probes-heading" className="text-lg font-semibold text-gray-950 dark:text-white">Lightning routing probes</h2>
          <p className="mt-1 max-w-3xl text-sm text-gray-500 dark:text-gray-400">
            An independent prober periodically sends payments with an unknown payment hash to this gateway's Lightning node. They can never settle, so no funds move, but where they fail shows whether the gateway can actually be paid over Lightning.
          </p>
        </div>

        {reportError && !report ? (
          <div className="p-6 text-center text-sm text-gray-500 dark:text-gray-400">Probe results are unavailable: {reportError}</div>
        ) : !hasProbes || !summary ? (
          <div className="p-6 text-center text-sm text-gray-500 dark:text-gray-400">This gateway has not been probed in the selected window yet.</div>
        ) : (
          <div className="p-4 sm:p-5">
            <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
              <StatTile
                label="Routing success"
                value={summary.success_rate_pct === null ? '—' : `${summary.success_rate_pct.toFixed(1)}%`}
                detail={summary.base_amount_msat !== null
                  ? `${formatNumber(summary.successes)} of ${formatNumber(summary.conclusive_probes)} probes at ${formatMsats(summary.base_amount_msat)}`
                  : undefined}
              />
              <StatTile
                label="Latest probe"
                value={summary.base_amount_last_outcome ? <OutcomeBadge outcome={summary.base_amount_last_outcome} /> : '—'}
                detail={lastProbe ? `Probed ${formatRelative(lastProbe, 'never')}` : undefined}
              />
              <StatTile
                label="Latency"
                value={formatLatency(summary.latency_p50_ms)}
                detail={summary.latency_p95_ms !== null ? `median · p95 ${formatLatency(summary.latency_p95_ms)}` : 'median round trip'}
              />
              <StatTile
                label="Inbound liquidity"
                value={summary.max_successful_amount_msat !== null ? `≥ ${formatMsats(summary.max_successful_amount_msat)}` : '—'}
                detail="Largest amount that reached the gateway in the last 24h"
              />
            </div>

            {trend.length > 0 && (
              <div className="mt-5">
                <h3 className="text-sm font-semibold text-gray-950 dark:text-white">{hourlyBuckets ? 'Hourly' : 'Daily'} routing success and latency</h3>
                <Suspense fallback={<div className="flex h-52 items-center justify-center text-sm text-gray-500">Loading chart…</div>}>
                  <ProbeTrendChart option={trendOption} style={{ height: 230 }} notMerge lazyUpdate />
                </Suspense>
              </div>
            )}

            <div className="mt-4 flex flex-wrap items-center gap-2 text-xs text-gray-600 dark:text-gray-300">
              <span className="font-medium">All probes in window:</span>
              {outcomeEntries.map(([outcome, count]) => (
                <span key={outcome} className="inline-flex items-center gap-1">
                  <OutcomeBadge outcome={outcome} />
                  <span>{formatNumber(count)}</span>
                </span>
              ))}
            </div>
            <p className="mt-2 text-xs text-gray-500 dark:text-gray-400">
              Routing success only counts probes at the smallest amount; larger amounts are increased until one fails and are used to estimate inbound liquidity.
              {summary.inconclusive_probes > 0 && ` ${formatNumber(summary.inconclusive_probes)} probe${summary.inconclusive_probes === 1 ? '' : 's'} failed for reasons on the prober's side and ${summary.inconclusive_probes === 1 ? 'is' : 'are'} not counted.`}
            </p>
          </div>
        )}

        {report && report.recent.length > 0 && (
          <div className="overflow-x-auto border-t border-gray-200 dark:border-gray-700">
            <table className="w-full min-w-[720px] text-left text-sm text-gray-600 dark:text-gray-300">
              <thead className="bg-slate-50 text-xs uppercase tracking-wide text-gray-600 dark:bg-gray-700/70 dark:text-gray-300">
                <tr>
                  <th scope="col" className="px-4 py-2.5 sm:px-5">Time</th>
                  <th scope="col" className="px-4 py-2.5 sm:px-5">Amount</th>
                  <th scope="col" className="px-4 py-2.5 sm:px-5">Outcome</th>
                  <th scope="col" className="px-4 py-2.5 text-right sm:px-5">Latency</th>
                  <th scope="col" className="px-4 py-2.5 text-right sm:px-5">Hops</th>
                  <th scope="col" className="px-4 py-2.5 text-right sm:px-5">Route fee</th>
                  <th scope="col" className="px-4 py-2.5 sm:px-5">Failure</th>
                </tr>
              </thead>
              <tbody>
                {report.recent.map((probe) => (
                  <tr key={`${probe.probe_time}-${probe.amount_msat}`} className="border-b border-gray-100 last:border-b-0 dark:border-gray-700">
                    <td className="whitespace-nowrap px-4 py-2 sm:px-5" title={probe.probe_time}>{formatDateTime(parseDate(probe.probe_time))}</td>
                    <td className="whitespace-nowrap px-4 py-2 sm:px-5">{formatMsats(probe.amount_msat)}</td>
                    <td className="px-4 py-2 sm:px-5"><OutcomeBadge outcome={probe.outcome} /></td>
                    <td className="whitespace-nowrap px-4 py-2 text-right tabular-nums sm:px-5">{formatLatency(probe.latency_ms)}</td>
                    <td className="px-4 py-2 text-right tabular-nums sm:px-5">{probe.route_hops ?? '—'}</td>
                    <td className="whitespace-nowrap px-4 py-2 text-right tabular-nums sm:px-5">{probe.route_fee_msat !== undefined ? formatMsats(probe.route_fee_msat) : '—'}</td>
                    <td className="px-4 py-2 font-mono text-xs text-gray-500 dark:text-gray-400" title={probe.failure_source_index !== undefined ? `Reported by hop ${probe.failure_source_index}` : undefined}>
                      {probe.failure_code ?? '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
