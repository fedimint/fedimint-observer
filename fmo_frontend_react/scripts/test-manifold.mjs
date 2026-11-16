import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  adFreshness,
  cheapestPlanMsats,
  formatPriceMsats,
  formatSizes,
  minorVersion,
} from '../src/features/manifold/lib/directory.ts';
import { concentration } from '../src/features/manifold/lib/concentration.ts';

const NOW = 1_800_000_000;

test('advertisement freshness follows the FA rule', () => {
  assert.equal(adFreshness(NOW - 60, NOW + 3600, NOW), 'fresh');
  // Exactly two hours old is still fresh; a second more is stale
  assert.equal(adFreshness(NOW - 7200, NOW + 60, NOW), 'fresh');
  assert.equal(adFreshness(NOW - 7201, NOW + 60, NOW), 'stale');
  // Expiry wins over age, and expires_at == now is expired
  assert.equal(adFreshness(NOW - 9000, NOW, NOW), 'expired');
  // A withdrawal republishes with expires_at == issued_at
  assert.equal(adFreshness(NOW - 10, NOW - 10, NOW), 'expired');
  assert.equal(adFreshness(NOW + 5, NOW + 3600, NOW), 'not_yet_valid');
});

// Search, filters, order and facets run on the backend (fmo_core/src/manifold/directory.rs, with its tests)
test('versions and prices format like the directory', () => {
  assert.equal(minorVersion('0.10.1+fedi'), '0.10');
  assert.equal(cheapestPlanMsats([]), null);
  assert.equal(cheapestPlanMsats([
    { kind: 'infinite_best_effort', price_msats: 9000 },
    { kind: 'subscription', initial_price_msats: 5000, renewal_price_msats: 1, period: 'P30D' },
  ]), 5000);
  assert.equal(formatPriceMsats(50_000_000), '50,000 sats');
  assert.equal(formatPriceMsats(1_500), '1.5 sats');
});

test('operator concentration follows the consensus threshold', () => {
  // 4 guardians: threshold 3, tolerates 1
  assert.equal(concentration(1, 4).tone, 'ok');
  assert.equal(concentration(2, 4).tone, 'warn');
  assert.equal(concentration(3, 4).tone, 'crit');
  // 7 guardians: threshold 5, tolerates 2
  assert.equal(concentration(2, 7).tone, 'ok');
  assert.equal(concentration(3, 7).tone, 'warn');
  assert.equal(concentration(5, 7).tone, 'crit');
});

test('federation sizes print as ranges', () => {
  assert.equal(formatSizes([7, 8, 9, 10, 20]), '7–10, 20');
  assert.equal(formatSizes([4, 5]), '4, 5');
  assert.equal(formatSizes([9, 7, 8, 8]), '7–9');
  assert.equal(formatSizes([]), '');
});
