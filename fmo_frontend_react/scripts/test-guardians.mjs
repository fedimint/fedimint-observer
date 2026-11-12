import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  availabilityLevel,
  buildGuardianRows,
  connectivityIssues,
  connectivityMatrix,
  federationWarnings,
  guardianSlots,
  guardianState,
  offlinePeriods,
  reachabilitySeries,
  resilience,
} from '../src/features/guardians/lib/guardianModel.ts';
import { attention, LIVE_STALE_AFTER_MS } from '../src/features/guardians/lib/attention.ts';

const NOW = '2026-11-11T12:00:00Z';
const ago = (minutes) => new Date(Date.parse(NOW) - minutes * 60_000).toISOString();
const guardians = [0, 1, 2, 3].map((id) => ({ id, url: `wss://g${id}.example/`, name: id === 0 ? 'Alice' : null }));

const live = (overrides = {}) => ({
  generated_at: NOW,
  guardians: [0, 1, 2, 3].map((id) => ({
    guardian_id: id,
    polled_at: ago(1),
    answered_at: ago(1),
    latency_ms: 100 + id,
    session_count: 5000,
    peers_online: 3,
    peers_offline: 0,
    peers_flagged: 0,
    scheduled_shutdown: null,
    peers: [0, 1, 2, 3].filter((peer) => peer !== id).map((peer) => ({ peer_id: peer, connected: true, flagged: false, last_contribution: 4999 })),
    ...overrides[id],
  })),
});

const rows = (liveData, extra = {}) => buildGuardianRows({ guardians, health: null, live: liveData, history7d: null, seatBindings: null, ...extra });

test('a guardian is online within 5 minutes of its last answer, offline when polled but silent', () => {
  const at = (answered, polled) => guardianState({ answered_at: answered, polled_at: polled }, NOW);
  assert.equal(at(ago(4), ago(1)), 'online');
  assert.equal(at(ago(6), ago(1)), 'offline');
  assert.equal(at(null, ago(20)), 'offline');
  // The collector backs off to 30-minute polls; beyond that it is not watching
  assert.equal(at(null, ago(40)), 'unknown');
  assert.equal(guardianState(undefined, NOW), 'unknown');
  assert.equal(guardianState({ answered_at: ago(1), polled_at: ago(1) }, null), 'unknown');
});

test('one guardian offline in a 4-guardian federation warns without claiming failure', () => {
  const data = rows(live({ 2: { answered_at: ago(30) } }));
  const res = resilience(data, true);
  assert.deepEqual({ online: res.online, threshold: res.threshold, level: res.level, faultTolerance: res.faultTolerance }, { online: 3, threshold: 3, level: 'at_threshold', faultTolerance: 1 });
  const warnings = federationWarnings(data, res, []);
  assert.equal(warnings[0].id, 'at-threshold');
  assert.equal(warnings[0].tone, 'warn');
  assert.ok(warnings.some((w) => w.id === 'offline-2' && w.tone === 'warn'));
  assert.ok(!warnings.some((w) => w.tone === 'crit'));
});

test('below the threshold is critical, and offline guardians are escalated with it', () => {
  const data = rows(live({ 1: { answered_at: ago(30) }, 2: { answered_at: ago(30) } }));
  const res = resilience(data, true);
  assert.equal(res.level, 'below_threshold');
  const warnings = federationWarnings(data, res, []);
  assert.equal(warnings[0].tone, 'crit');
  assert.ok(warnings.filter((w) => w.id.startsWith('offline-')).every((w) => w.tone === 'crit'));
});

test('unknown guardians leave consensus undetermined rather than failed', () => {
  const data = rows(live({ 1: { answered_at: null, polled_at: ago(50) }, 2: { answered_at: null, polled_at: ago(50) } }));
  assert.equal(resilience(data, true).level, 'unknown');
  assert.equal(resilience(rows(null), false).level, 'unknown');
});

test('a single-guardian federation is operational with its one guardian', () => {
  const single = buildGuardianRows({ guardians: [guardians[0]], health: null, live: { generated_at: NOW, guardians: [live().guardians[0]] }, history7d: null, seatBindings: null });
  assert.equal(resilience(single, true).level, 'operational');
});

test('session lag is measured against the furthest online guardian only', () => {
  const data = rows(live({ 1: { session_count: 4997 }, 3: { answered_at: ago(30), session_count: 9999 } }));
  assert.equal(data[1].sessionLag, 3);
  assert.equal(data[0].sessionLag, 0);
  assert.equal(data[3].sessionCount, null);
  assert.ok(data[1].warnings.some((w) => w.id === 'lag-1'));
});

test('only verified seat bindings attach a Manifold identity', () => {
  const binding = { peer_id: 2, fman_pubkey: 'ab'.repeat(32), display_name: 'blissful-chiffchaff', guardian_identity: '02', guardian_fee_pubkey: '03', issued_at: 1 };
  const verified = rows(live(), { seatBindings: { has_meta_module: true, observed_at: NOW, status: 'verified', error: null, bindings: [binding], distinct_guardians: 1 } });
  assert.equal(verified[2].binding?.display_name, 'blissful-chiffchaff');
  assert.equal(verified[0].binding, null);
  const invalid = rows(live(), { seatBindings: { has_meta_module: true, observed_at: NOW, status: 'invalid', error: 'x', bindings: [binding], distinct_guardians: 1 } });
  assert.equal(invalid[2].binding, null);
});

test('peer views reveal flags, splits and guardians isolated from their peers', () => {
  const disconnectedFrom = (target) => (id) => ({ peers: live().guardians[id].peers.map((p) => (p.peer_id === target ? { ...p, connected: false } : p)) });
  // Everyone reports guardian 3 disconnected while the observer reaches it
  const isolated = rows(live({ 0: disconnectedFrom(3)(0), 1: disconnectedFrom(3)(1), 2: disconnectedFrom(3)(2) }));
  const matrix = connectivityMatrix(isolated);
  assert.equal(matrix.cell(0, 3), 'disconnected');
  assert.equal(matrix.cell(3, 3), 'self');
  assert.deepEqual(connectivityIssues(isolated, matrix).map((i) => [i.peerId, i.kind]), [[3, 'isolated']]);

  // One guardian disagrees about guardian 1
  const split = rows(live({ 0: disconnectedFrom(1)(0) }));
  assert.deepEqual(connectivityIssues(split, connectivityMatrix(split)).map((i) => [i.peerId, i.kind]), [[1, 'split']]);

  // A flag outranks connection state
  const flagged = rows(live({ 2: { peers: live().guardians[2].peers.map((p) => (p.peer_id === 0 ? { ...p, flagged: true } : p)) } }));
  assert.deepEqual(connectivityIssues(flagged, connectivityMatrix(flagged)).map((i) => [i.peerId, i.kind]), [[0, 'flagged']]);

  // Offline guardians do not report
  const quiet = rows(live({ 0: { answered_at: ago(30) } }));
  assert.ok(!connectivityMatrix(quiet).reporters.includes(0));
});

// Six hourly buckets from 06:00; guardian 1 misses 08:00-10:59, 11:00 not polled
const history = {
  window: '24h',
  bucket_seconds: 3600,
  start: '2026-11-11T06:00:00Z',
  bucket_count: 6,
  generated_at: NOW,
  guardians: [
    { guardian_id: 0, uptime_pct: 100, avg_latency_ms: 100, last_online_hour: null, buckets: [0, 1, 2, 3, 4, 5].map((h) => ({ start: `2026-11-11T${String(6 + h).padStart(2, '0')}:00:00Z`, samples: 60, online_samples: 60, polled_s: 3600, online_s: 3600, avg_latency_ms: 100 + h * 10 })) },
    { guardian_id: 1, uptime_pct: 40, avg_latency_ms: 90, last_online_hour: null, buckets: [0, 1, 2, 3, 4].map((h) => ({ start: `2026-11-11T${String(6 + h).padStart(2, '0')}:00:00Z`, samples: 60, online_samples: h >= 2 ? 0 : 60, polled_s: 3600, online_s: h >= 2 ? 0 : 3600, avg_latency_ms: h >= 2 ? null : 90 })) },
  ],
};

test('history slots fill unpolled buckets and merge groups', () => {
  const slots = guardianSlots(history, 1);
  assert.equal(slots.length, 6);
  assert.equal(slots[5].polledS, 0);
  const merged = guardianSlots(history, 0, 4);
  assert.equal(merged.length, 2);
  assert.equal(merged[0].polledS, 4 * 3600);
  assert.equal(merged[1].end - merged[1].start, 2 * 3600_000);
  // Latency is weighted by answered polls
  assert.equal(merged[0].latencyMs, 115);
  assert.equal(guardianSlots(history, 7).every((slot) => slot.polledS === 0), true);
});

test('offline periods need consecutive polled silence; unpolled time ends a run', () => {
  const periods = offlinePeriods(history, 1, 2);
  assert.equal(periods.length, 1);
  assert.equal(new Date(periods[0].start).toISOString(), '2026-11-11T08:00:00.000Z');
  assert.equal(new Date(periods[0].end).toISOString(), '2026-11-11T11:00:00.000Z');
  assert.equal(periods[0].ongoing, false);
  assert.deepEqual(offlinePeriods(history, 1, 4), []);
  assert.deepEqual(offlinePeriods(history, 0, 1), []);
});

test('reachability sums each guardian online share per slot', () => {
  const series = reachabilitySeries(history, [0, 1]);
  assert.equal(series[0].answering, 2);
  assert.equal(series[3].answering, 1);
  // Only guardian 0 was polled in the last hour
  assert.equal(series[5].answering, 1);
  assert.equal(series[5].polledGuardians, 1);
});

test('availability levels', () => {
  assert.equal(availabilityLevel(null), 'none');
  assert.equal(availabilityLevel(100), 'ok');
  assert.equal(availabilityLevel(98), 'ok');
  assert.equal(availabilityLevel(97.9), 'minor');
  assert.equal(availabilityLevel(89), 'major');
  assert.equal(availabilityLevel(0), 'down');
});

test('the attention list is only an all-clear on complete, current live status', () => {
  const data = rows(live());
  const res = resilience(data, true);
  const now = Date.parse(NOW);
  const base = { warnings: [], res, liveGeneratedAt: NOW, liveFailed: false, now };
  assert.equal(attention(base).kind, 'clear');

  const failed = attention({ ...base, liveGeneratedAt: null, liveFailed: true });
  assert.equal(failed.kind, 'unknown', 'a failed live load is not an all-clear');
  assert.match(failed.message, /could not be loaded/);

  assert.equal(attention({ ...base, liveGeneratedAt: null }).kind, 'unknown', 'nothing loaded yet');

  const stale = attention({ ...base, now: now + LIVE_STALE_AFTER_MS + 60_000 });
  assert.equal(stale.kind, 'unknown', 'stale live status is not an all-clear');
  assert.match(stale.message, /minutes old/);

  const partial = rows(live({ 1: { answered_at: null, polled_at: null } }));
  const partialRes = resilience(partial, true);
  assert.ok(partialRes.unknown > 0);
  assert.equal(attention({ ...base, res: partialRes }).kind, 'unknown', 'unpolled guardians leave the picture incomplete');
});

test('warnings show with a note when the evidence behind them is incomplete', () => {
  const data = rows(live({ 2: { answered_at: ago(30) } }));
  const res = resilience(data, true);
  const warnings = federationWarnings(data, res, []);
  const now = Date.parse(NOW);
  const current = attention({ warnings, res, liveGeneratedAt: NOW, liveFailed: false, now });
  assert.equal(current.kind, 'warnings');
  assert.equal(current.note, null);
  const stale = attention({ warnings, res, liveGeneratedAt: NOW, liveFailed: true, now: now + LIVE_STALE_AFTER_MS + 60_000 });
  assert.equal(stale.kind, 'warnings');
  assert.match(stale.note, /minutes old/);
});

test('threshold warnings speak for the observer, not for consensus itself', () => {
  const below = rows(live({ 1: { answered_at: ago(30) }, 2: { answered_at: ago(30) } }));
  const belowWarning = federationWarnings(below, resilience(below, true), [])[0];
  assert.match(belowWarning.message, /answer the observer/);
  assert.match(belowWarning.message, /may/);
  assert.doesNotMatch(belowWarning.message, /cannot process transactions until/);
});
