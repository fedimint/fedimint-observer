import assert from 'node:assert/strict';
import { test } from 'node:test';
import { LookupCache } from '../src/utils/lookupCache.ts';

const clock = () => {
  let time = 1_000;
  return { now: () => time, advance: (ms) => { time += ms; } };
};

test('a settled lookup is shared and not loaded again', async () => {
  const cache = new LookupCache({ failureTtlMs: 1_000 });
  let loads = 0;
  const load = async () => { loads += 1; return 'name'; };
  const [first, second] = await Promise.all([cache.get('a', load), cache.get('a', load)]);
  assert.equal(first, 'name');
  assert.equal(second, 'name');
  assert.equal(await cache.get('a', load), 'name');
  assert.equal(loads, 1);
});

test('a failed lookup is remembered until its time is up, then tried again', async () => {
  const time = clock();
  const cache = new LookupCache({ failureTtlMs: 15 * 60_000, now: time.now });
  let loads = 0;
  const failing = async () => { loads += 1; throw new Error('502'); };
  await assert.rejects(cache.get('dead', failing), /502/);
  // A refresh a minute later must not ask the server again
  time.advance(60_000);
  await assert.rejects(cache.get('dead', failing), /502/);
  assert.equal(loads, 1);
  time.advance(15 * 60_000);
  assert.equal(await cache.get('dead', async () => { loads += 1; return 'back'; }), 'back');
  assert.equal(loads, 2);
});

test('a failure does not affect other keys', async () => {
  const cache = new LookupCache({ failureTtlMs: 60_000 });
  await assert.rejects(cache.get('dead', async () => { throw new Error('502'); }));
  assert.equal(await cache.get('alive', async () => 'name'), 'name');
});
