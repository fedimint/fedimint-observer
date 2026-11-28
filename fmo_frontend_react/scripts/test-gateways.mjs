import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  buildUptimeStrip,
  formatCompactDuration,
  getGatewayStatus,
  toGatewayRow,
} from '../src/features/gateways/lib/gatewayRows.ts';
import { liveLookupGate, mergeGatewayData, selectGatewayData } from '../src/features/gateways/lib/gatewaySelection.ts';
import { gatewayViewFrom } from '../src/features/gateways/lib/gatewayView.ts';
import { request, share } from '../src/services/http.ts';

const NOW = Date.parse('2026-01-15T12:00:00Z');
const minutesAgo = (minutes) => new Date(NOW - minutes * 60_000);
const gateway = (id, extra = {}) => ({
  gateway_id: id,
  node_pub_key: `node-${id}`,
  lightning_alias: `alias-${id}`,
  api_endpoint: `https://${id}.example`,
  vetted: false,
  ...extra,
});
const sources = (extra) => ({
  observed: null,
  observedError: null,
  live: { status: 'skipped' },
  federationFailed: false,
  federationOffline: false,
  dataWindow: '7d',
  ...extra,
});

test('gateway status follows the time since it was last seen', () => {
  assert.equal(getGatewayStatus(null, NOW), 'unknown');
  assert.equal(getGatewayStatus(minutesAgo(15), NOW), 'online', 'three missed polls still count as online');
  assert.equal(getGatewayStatus(minutesAgo(16), NOW), 'degraded');
  assert.equal(getGatewayStatus(minutesAgo(31), NOW), 'offline');
  assert.equal(getGatewayStatus(minutesAgo(7 * 24 * 60 + 1), NOW), 'retired', 'gone for over a week');
});

test('compact durations round to the largest whole unit', () => {
  assert.equal(formatCompactDuration(0), '0m');
  assert.equal(formatCompactDuration(-5), '0m');
  assert.equal(formatCompactDuration(45), '45m');
  assert.equal(formatCompactDuration(90), '2h');
  assert.equal(formatCompactDuration(36 * 60), '2d');
});

test('the uptime strip reads each bucket, and is all unknown without poll history', () => {
  const buckets = Array.from({ length: 30 }, (_, index) => ({
    total_samples: index === 0 ? 0 : 4,
    seen_samples: index === 1 ? 0 : index === 2 ? 2 : 4,
  }));
  const strip = buildUptimeStrip(gateway('a', { uptime_window: { buckets } }));
  assert.deepEqual(strip.slice(0, 4), ['unknown', 'offline', 'degraded', 'online']);
  assert.deepEqual(buildUptimeStrip(gateway('b')), Array(30).fill('unknown'));
});

test('a row without activity has no score, and live rows count the whole window as unobserved', () => {
  const row = toGatewayRow(gateway('a', { last_seen: minutesAgo(5).toISOString() }), NOW, 1440);
  assert.equal(row.status, 'online');
  assert.equal(row.realActivityScore, null);
  assert.equal(row.estimatedUnknownMinutes, 1440);
  const active = toGatewayRow(gateway('b', { activity_window: { fund_count: 2, settle_count: 1, cancel_count: 0, total_volume_msat: 0 } }), NOW, 1440);
  assert.equal(active.realActivityScore, 5, 'funds + 3 × settles');
});

test('live registry details win, recorded history keeps its timings, and recorded-only gateways stay', () => {
  const observed = [gateway('a', { lightning_alias: 'old', last_seen: 'recorded' }), gateway('b')];
  const live = [gateway('a', { lightning_alias: 'new', last_seen: 'live' })];
  const merged = mergeGatewayData(observed, live);
  assert.deepEqual(merged.map((row) => row.gateway_id), ['a', 'b']);
  assert.equal(merged[0].lightning_alias, 'new');
  assert.equal(merged[0].last_seen, 'recorded');
});

test('a recorded page gets live details for its own rows only', () => {
  const page = [gateway('a')];
  const live = [gateway('a', { lightning_alias: 'live-a' }), gateway('z', { lightning_alias: 'live-only' })];
  assert.deepEqual(mergeGatewayData(page, live).map((row) => row.lightning_alias), ['live-a']);
  assert.deepEqual(mergeGatewayData([], live).map((row) => row.gateway_id), ['a', 'z']);
  assert.deepEqual(mergeGatewayData([], live, false), []);
  const narrowed = selectGatewayData(sources({ observed: [], live: { status: 'done', gateways: live, error: null }, listNarrowed: true }));
  assert.deepEqual(narrowed.gateways, []);
  assert.equal(narrowed.warning, null);
});

test('merging keeps the recorded uptime figures, not only the strip', () => {
  const recorded = {
    status: 'online',
    coverage_pct: 36.8,
    estimated_uptime_pct: 100,
    estimated_online_minutes: 530,
    estimated_offline_minutes: 0,
    estimated_unknown_minutes: 910,
    uptime_window: { sample_count: 106, seen_samples: 106, online_minutes: 530, offline_minutes: 0, uptime_pct: 100, buckets: [] },
  };
  const [merged] = mergeGatewayData([gateway('a', recorded)], [gateway('a', { lightning_alias: 'live' })]);
  assert.equal(merged.lightning_alias, 'live');
  for (const [key, value] of Object.entries(recorded)) assert.deepEqual(merged[key], value, key);
  const row = toGatewayRow(merged, NOW, 1440);
  assert.equal(row.coveragePct, 36.8);
  assert.equal(row.estimatedUnknownMinutes, 910);
});

test('a normal page shows no banner', () => {
  const selection = selectGatewayData(sources({ observed: [gateway('a')] }));
  assert.equal(selection.warning, null);
  assert.equal(selection.retry, false);
});

test('an empty history waits for the live registry before saying there are no gateways', () => {
  assert.equal(selectGatewayData(sources({ observed: [], live: { status: 'pending' } })).warning.title, 'Checking for gateways');
  assert.equal(selectGatewayData(sources({ observed: [] })).warning.title, 'No gateways');
  assert.equal(selectGatewayData(sources({ observed: [], federationOffline: true })).warning.title, 'Federation offline');
});

test('registry rows stand in for the history only once loading it failed', () => {
  const observedError = { window: '7d', message: 'boom', network: false };
  const live = { status: 'done', gateways: [gateway('a')], error: null };
  const selection = selectGatewayData(sources({ observedError, live }));
  assert.equal(selection.warning.title, 'Gateway history unavailable');
  assert.equal(selection.gateways.length, 1);
  assert.equal(selection.retry, true);
});

test('with nothing to show, a network failure asks to check the connection', () => {
  const selection = selectGatewayData(sources({ observedError: { window: '7d', message: 'Failed to fetch', network: true } }));
  assert.equal(selection.warning.level, 'error');
  assert.equal(selection.warning.message, 'Check your connection and try again.');
});

test('an offline summary is rechecked before the live registry lookup is skipped', () => {
  assert.equal(liveLookupGate('online', { health: undefined, loading: true }), 'run');
  assert.equal(liveLookupGate('degraded', { health: undefined, loading: true }), 'run');
  assert.equal(liveLookupGate('offline', { health: undefined, loading: true }), 'wait');
  assert.equal(liveLookupGate('offline', { health: 'online', loading: false }), 'run');
  assert.equal(liveLookupGate('offline', { health: 'offline', loading: false }), 'offline');
  assert.equal(liveLookupGate('offline', { health: undefined, loading: false }), 'offline');
});

test('a fresh recheck after an offline prefetch starts the live registry lookup', async () => {
  const realFetch = globalThis.fetch;
  const answers = [{ health: 'offline' }, { health: 'online' }];
  let calls = 0;
  globalThis.fetch = async () => new Response(JSON.stringify(answers[calls++]), { status: 200, headers: { 'content-type': 'application/json' } });
  try {
    const url = 'https://api.test/federations/abc/summary';
    share(url, 10_000);
    const prefetched = await request(url);
    assert.equal(liveLookupGate(prefetched.health, { health: undefined, loading: true }), 'wait');
    const recheck = await request(url, { fresh: true });
    assert.equal(liveLookupGate(prefetched.health, { health: recheck.health, loading: false }), 'run');
    assert.equal(calls, 2);
  } finally {
    globalThis.fetch = realFetch;
  }
});

test('the gateway view comes from the address bar, then the stored window, then the defaults', () => {
  const view = (search, stored = null) => gatewayViewFrom(new URLSearchParams(search), stored);
  assert.deepEqual(view(''), { timeWindow: '7d', filter: 'all', sort: 'freshness', direction: 'desc' });
  assert.equal(view('', '30d').timeWindow, '30d');
  assert.equal(view('?window=24h', '30d').timeWindow, '24h');
  assert.deepEqual(view('?window=1y&status=bogus&sort=uptime&dir=up', '90d'), {
    timeWindow: '90d', filter: 'all', sort: 'uptime', direction: 'desc',
  });
  assert.deepEqual(view('?status=offline&sort=status&dir=asc'), { timeWindow: '7d', filter: 'offline', sort: 'status', direction: 'asc' });
});
