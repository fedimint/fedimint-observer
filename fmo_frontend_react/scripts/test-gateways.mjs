import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  buildUptimeStrip,
  filterAndSortGateways,
  formatCompactDuration,
  getGatewayStatus,
  toGatewayRow,
} from '../src/features/gateways/lib/gatewayRows.ts';
import { mergeGatewayData, selectGatewayData } from '../src/features/gateways/lib/gatewaySelection.ts';

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

test('"all" hides retired gateways, and sorting by status puts the worst first', () => {
  const rows = [
    toGatewayRow(gateway('online', { last_seen: minutesAgo(1).toISOString() }), NOW, 1440),
    toGatewayRow(gateway('retired', { last_seen: minutesAgo(8 * 24 * 60).toISOString() }), NOW, 1440),
    toGatewayRow(gateway('offline', { last_seen: minutesAgo(60).toISOString() }), NOW, 1440),
  ];
  assert.deepEqual(filterAndSortGateways(rows, 'all', '', 'status', 'asc').map((row) => row.gateway_id), ['offline', 'online']);
  assert.deepEqual(filterAndSortGateways(rows, 'retired', '', 'status', 'asc').map((row) => row.gateway_id), ['retired']);
  assert.deepEqual(filterAndSortGateways(rows, 'all', 'ALIAS-ONLINE', 'status', 'asc').map((row) => row.gateway_id), ['online']);
});

test('live registry details win, recorded history keeps its timings, and recorded-only gateways stay', () => {
  const observed = [gateway('a', { lightning_alias: 'old', last_seen: 'recorded' }), gateway('b')];
  const live = [gateway('a', { lightning_alias: 'new', last_seen: 'live' })];
  const merged = mergeGatewayData(observed, live);
  assert.deepEqual(merged.map((row) => row.gateway_id), ['a', 'b']);
  assert.equal(merged[0].lightning_alias, 'new');
  assert.equal(merged[0].last_seen, 'recorded');
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
