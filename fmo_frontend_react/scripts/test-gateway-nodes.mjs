import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  formatReachability,
  hasUnverified,
  isMissingRoute,
  lnv1ById,
  lnv1Numbers,
  nodeName,
  reachabilityPct,
  registrationKey,
  sortNodes,
} from '../src/features/gateway-nodes/lib/gatewayNodes.ts';
import { ApiError } from '../src/services/http.ts';

const KEY_A = `02${'a'.repeat(64)}`;
const KEY_B = `03${'b'.repeat(64)}`;
const activity = (contracts = 0) => ({
  incoming_contracts: contracts, outgoing_contracts: 0, incoming_msat: 0, outgoing_msat: 0,
  outgoing_claimed: 0, outgoing_refunded: 0, outgoing_cancelled: 0,
});
const lnv1 = (id) => ({
  protocol: 'lnv1', gateway_id: id, api_endpoint: `https://${id}.example/v1`,
  first_seen: '2026-11-01T00:00:00Z', last_seen: '2026-11-15T00:00:00Z',
});
const lnv2 = (endpoint, verified, extra = {}) => ({
  protocol: 'lnv2', module_instance_id: 3, api_endpoint: endpoint,
  first_seen: '2026-11-01T00:00:00Z', last_seen: '2026-11-15T00:00:00Z', status: 'online',
  url_matches_lnv1: verified, samples: 10, online_samples: 9, activity: activity(1), attribution: 'self_reported',
  ...extra,
});
const node = (key, registrations, totals = null) => ({
  node_pub_key: key, registrations, totals,
});
const overviewGateway = (id, extra = {}) => ({
  gateway_id: id, node_pub_key: KEY_A, lightning_alias: `alias-${id}`, api_endpoint: `https://${id}.example/v1`, vetted: false, ...extra,
});

test('Lightning v1 registrations are joined to the overview by gateway id', () => {
  const byId = lnv1ById([overviewGateway('g1', { coverage_pct: 99, estimated_uptime_pct: 97.5, activity_window: { fund_count: 4, settle_count: 3, cancel_count: 1, total_volume_msat: 5000 } })]);
  assert.deepEqual(lnv1Numbers(lnv1('g1'), byId), {
    uptimePct: 97.5,
    coveragePct: 99,
    activity: { fund_count: 4, settle_count: 3, cancel_count: 1, total_volume_msat: 5000 },
  });
  assert.deepEqual(lnv1Numbers(lnv1('unknown'), byId), { uptimePct: null, coveragePct: null, activity: null }, 'a gateway the overview lacks has no numbers');
});

test('uptime without sampled poll time is unavailable, while a measured 0% stays 0%', () => {
  // As the server reports a gateway no poll in the window sampled: 0% of nothing
  const unsampled = lnv1ById([overviewGateway('new', { coverage_pct: 0, estimated_uptime_pct: 0, estimated_unknown_minutes: 10080 })]);
  assert.equal(lnv1Numbers(lnv1('new'), unsampled).uptimePct, null);
  assert.equal(lnv1Numbers(lnv1('new'), unsampled).coveragePct, null, 'no coverage is shown without an uptime');
  const noCoverage = lnv1ById([overviewGateway('old', { estimated_uptime_pct: 0 })]);
  assert.equal(lnv1Numbers(lnv1('old'), noCoverage).uptimePct, null, 'an overview without coverage measured nothing');
  const down = lnv1ById([overviewGateway('down', { coverage_pct: 99, estimated_uptime_pct: 0 })]);
  assert.equal(lnv1Numbers(lnv1('down'), down).uptimePct, 0, 'polled and never listed is a real 0%');
  const partly = lnv1ById([overviewGateway('partly', { coverage_pct: 23, estimated_uptime_pct: 100 })]);
  assert.deepEqual([lnv1Numbers(lnv1('partly'), partly).uptimePct, lnv1Numbers(lnv1('partly'), partly).coveragePct], [100, 23], 'uptime comes with how much of the window was observed');
});

test('a missing Lightning v1 overview leaves every v1 registration without numbers', () => {
  for (const missing of [undefined, null, []]) {
    const byId = lnv1ById(missing);
    assert.equal(byId.size, 0);
    assert.deepEqual(lnv1Numbers(lnv1('g1'), byId), { uptimePct: null, coveragePct: null, activity: null });
  }
});

test('a node is named by its Lightning v1 alias, else by a short form of its key', () => {
  const byId = lnv1ById([overviewGateway('g1')]);
  assert.equal(nodeName(node(KEY_A, [lnv1('g1'), lnv2('https://a', true)]), byId), 'alias-g1');
  assert.equal(nodeName(node(KEY_B, [lnv2('https://b', false)]), byId), `Node ${KEY_B.slice(0, 8)}…${KEY_B.slice(-6)}`);
  assert.equal(nodeName(node(KEY_A, [lnv1('g1')]), lnv1ById([overviewGateway('g1', { lightning_alias: '' })])).startsWith('Node '), true, 'an empty alias is no name');
});

test('nodes are ordered by name, then key', () => {
  const byId = lnv1ById([overviewGateway('zed', { lightning_alias: 'Zed' }), overviewGateway('amy', { lightning_alias: 'Amy' })]);
  const sorted = sortNodes([node(KEY_B, [lnv1('zed')]), node(KEY_A, [lnv1('amy')])], byId);
  assert.deepEqual(sorted.map((entry) => entry.node_pub_key), [KEY_A, KEY_B]);
});

test('a node with any Lightning v2 registration whose URL no v1 registration announces is flagged unverified', () => {
  assert.equal(hasUnverified(node(KEY_A, [lnv1('g1'), lnv2('https://g1.example/v1', true)])), false);
  assert.equal(hasUnverified(node(KEY_A, [lnv1('g1'), lnv2('https://g1.example/v1', true), lnv2('iroh://node', false)])), true);
  assert.equal(hasUnverified(node(KEY_A, [lnv1('g1')])), false, 'Lightning v1 alone is never unverified');
});

test('reachability is a share of probes, and absent without any', () => {
  assert.equal(reachabilityPct(0, 0), null);
  assert.equal(reachabilityPct(8, 6), 75);
  assert.equal(formatReachability(0, 0), 'No probe history');
  assert.equal(formatReachability(8, 6), '75.0% of 8 probes');
});

test('registration keys are unique across protocols, instances and endpoints', () => {
  const keys = [lnv1('g1'), lnv2('https://a', true), lnv2('https://a', true, { module_instance_id: 4 }), lnv2('https://b', false)].map(registrationKey);
  assert.equal(new Set(keys).size, keys.length);
});

test('only a 404 means the grouped view is missing', () => {
  assert.equal(isMissingRoute(new ApiError('http', 'Not found', 404)), true);
  assert.equal(isMissingRoute(new ApiError('http', 'Internal error', 500)), false);
  assert.equal(isMissingRoute(new ApiError('network', 'Offline')), false);
  assert.equal(isMissingRoute(new Error('404')), false);
});
