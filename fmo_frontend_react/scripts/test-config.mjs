import assert from 'node:assert/strict';
import { test } from 'node:test';
import { configSource, instancesOf, kindsByInstance, parseConfig } from '../src/utils/config.ts';

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

test('module instances are found by kind, from the summary outline or the config alike', () => {
  const kinds = kindsByInstance({ 0: { kind: 'ln' }, 1: { kind: 'lnv2' }, 4: { kind: 'lnv2' }, 5: {} });
  assert.deepEqual(kinds, { 0: 'ln', 1: 'lnv2', 4: 'lnv2', 5: 'unknown' });
  assert.deepEqual(instancesOf(kinds, 'lnv2'), [1, 4]);
  assert.deepEqual(instancesOf({ 2: 'walletv2' }, 'walletv2'), [2]);
  assert.deepEqual(instancesOf(kindsByInstance(undefined), 'lnv2'), []);
});

test('the invite lookup stands in for a failed stored config, and a retry goes back to the stored one', () => {
  assert.equal(configSource(false, 'fed1invite'), 'stored');
  assert.equal(configSource(true, 'fed1invite'), 'invite');
  assert.equal(configSource(true, undefined), 'stored');
  assert.equal(configSource(false, 'fed1invite'), 'stored');
});
