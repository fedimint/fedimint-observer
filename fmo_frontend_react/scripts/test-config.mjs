import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseConfig } from '../src/utils/config.ts';

test('Wallet v2 supplies the network without inventing v1 finality settings', () => {
  const config = parseConfig({ modules: { 3: { kind: 'walletv2', network: 'signet' } } });
  assert.equal(config.network, 'signet');
  assert.equal(config.confirmationsRequired, 0);
});

test('Wallet v1 configuration stays compatible in a mixed federation', () => {
  const config = parseConfig({ modules: {
    0: { kind: 'wallet', network: 'bitcoin', finality_delay: 9 },
    3: { kind: 'walletv2', network: 'bitcoin' },
  } });
  assert.equal(config.network, 'bitcoin');
  assert.equal(config.confirmationsRequired, 10);
});

test('Missing wallet configuration stays unknown', () => {
  assert.equal(parseConfig({ modules: {} }).network, 'unknown');
});
