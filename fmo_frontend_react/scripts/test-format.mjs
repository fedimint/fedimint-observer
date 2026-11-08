import assert from 'node:assert/strict';
import { test } from 'node:test';

// West of UTC, where a UTC midnight is still the previous local day
process.env.TZ = 'America/Los_Angeles';

const { formatMsatsCompact } = await import('../src/utils/format.ts');
const { explorerAddressUrl, explorerTxUrl, nostrNetworkTag } = await import('../src/utils/network.ts');
const { formatUtcDate, formatUtcDateTime, formatUtcDayHour } = await import('../src/utils/time.ts');
const { toHistogramEntries } = await import('../src/utils/histogram.ts');

test('compact amounts switch to M sats at a million sats, not 100k', () => {
  assert.equal(formatMsatsCompact(999_000), '999 sats');
  assert.equal(formatMsatsCompact(1_500_000), '1.5k sats');
  assert.equal(formatMsatsCompact(100_000_000), '100.0k sats');
  assert.equal(formatMsatsCompact(500_000_000), '500.0k sats');
  assert.equal(formatMsatsCompact(999_000_000), '999.0k sats');
  assert.equal(formatMsatsCompact(1_000_000_000), '1.0M sats');
  assert.equal(formatMsatsCompact(25_000_000_000), '25.0M sats');
  assert.equal(formatMsatsCompact(100_000_000_000), '1.00 BTC');
});

test('explorer links follow the federation network', () => {
  const txid = 'a'.repeat(64);
  assert.equal(explorerTxUrl(`${txid}:1`, 'bitcoin'), `https://mempool.space/tx/${txid}`);
  assert.equal(explorerTxUrl(txid, 'signet'), `https://mempool.space/signet/tx/${txid}`);
  assert.equal(explorerTxUrl(txid, 'Testnet4'), `https://mempool.space/testnet4/tx/${txid}`);
  assert.equal(explorerAddressUrl('tb1qxyz', 'testnet'), 'https://mempool.space/testnet/address/tb1qxyz');
  assert.equal(explorerTxUrl(txid, 'regtest'), null);
  assert.equal(explorerTxUrl(txid, undefined), null);
});

test('Nostr network tags use NIP-87 names and never guess', () => {
  assert.equal(nostrNetworkTag('bitcoin'), 'mainnet');
  assert.equal(nostrNetworkTag('signet'), 'signet');
  assert.equal(nostrNetworkTag('regtest'), 'regtest');
  assert.equal(nostrNetworkTag('unknown'), null);
  assert.equal(nostrNetworkTag(undefined), null);
});

test('UTC days and times do not shift with the viewer time zone', () => {
  assert.equal(formatUtcDate('2024-05-31'), 'May 31, 2024');
  assert.equal(formatUtcDate('2026-11-07T00:00:00Z', { month: 'short', day: 'numeric' }), 'Nov 7');
  assert.equal(formatUtcDateTime('2026-11-07T19:20:00Z'), 'Nov 7, 2026, 19:20 UTC');
  assert.equal(formatUtcDateTime(null, 'Not yet observed'), 'Not yet observed');
  assert.equal(formatUtcDateTime('not a date'), 'N/A');
  assert.equal(formatUtcDayHour('2026-11-08T00:00:00Z'), 'Nov 8, 00:00');
});

test('histogram days are shown as sent, with outliers left out only when asked', () => {
  const histogram = {
    days: [
      { date: '2024-05-30', count: 0, volume_msat: 0, avg_count: 0, avg_volume_msat: 0, outlier: false },
      { date: '2024-05-31', count: 2, volume_msat: 200_000_000_000, avg_count: 1, avg_volume_msat: 100_000_000_000, outlier: true },
      { date: '2024-06-01', count: 1, volume_msat: 50_000_000_000, avg_count: null, avg_volume_msat: null, outlier: false },
    ],
    total_count: 3,
    total_volume_msat: 250_000_000_000,
  };
  const all = toHistogramEntries(histogram, false);
  assert.deepEqual(all.map((entry) => entry.date), ['May 30, 2024', 'May 31, 2024', 'Jun 1, 2024']);
  assert.equal(all[1].timestamp, Date.UTC(2024, 4, 31));
  assert.equal(all[1].volume, 2);
  assert.equal(all[1].avgVolume, 1);
  assert.equal(all[2].avgCount, undefined);

  const filtered = toHistogramEntries(histogram, true);
  assert.deepEqual(filtered.map((entry) => entry.date), ['May 30, 2024', 'Jun 1, 2024']);
});
