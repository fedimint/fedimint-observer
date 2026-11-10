import assert from 'node:assert/strict';
import { test } from 'node:test';
import { PageRequests } from '../src/utils/pageRequests.ts';

const deferred = () => {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

// The paginated lists (wallet operations, gateway registry) share this gate.
for (const olderFirst of [false, true]) {
  test(`refresh racing pagination preserves the page boundary, older first=${olderFirst}`, async () => {
    const gate = new PageRequests();
    let page = Array.from({ length: 20 }, (_, i) => 100 - i);
    const origin = page;
    const refresh = deferred();
    const older = deferred();
    const refreshGeneration = gate.refresh();
    const refreshing = refresh.promise.then(next => {
      if (gate.acceptsRefresh(refreshGeneration)) page = next;
    });
    const generation = gate.beginPage();
    assert.equal(gate.refresh(), null, 'pause starts before pagination resolves');
    const paging = older.promise.then(next => {
      if (gate.finishPage(generation, true)) page = [...origin, ...next];
    });
    const finishRefresh = async () => { refresh.resolve(Array.from({ length: 20 }, (_, i) => 101 - i)); await refreshing; };
    const finishPage = async () => { older.resolve(Array.from({ length: 20 }, (_, i) => 80 - i)); await paging; };
    if (olderFirst) { await finishPage(); await finishRefresh(); }
    else { await finishRefresh(); await finishPage(); }
    assert.deepEqual(page, Array.from({ length: 40 }, (_, i) => 100 - i));
    assert.equal(gate.refresh(), null);
    gate.reset();
    assert.notEqual(gate.refresh(), null, 'show latest resumes refresh');
  });
}

test('failed pagination resumes refresh and invalidates the older refresh response', async () => {
  const gate = new PageRequests();
  const refreshGeneration = gate.refresh();
  const generation = gate.beginPage();
  const older = deferred();
  const paging = older.promise.catch(() => gate.finishPage(generation, false));
  older.reject(new Error('temporary failure'));
  assert.equal(await paging, true);
  assert.equal(gate.acceptsRefresh(refreshGeneration), false);
  assert.equal(gate.acceptsRefresh(gate.refresh()), true);
});

test('changing page generation discards an outstanding pagination response', () => {
  const gate = new PageRequests();
  const generation = gate.beginPage();
  gate.reset();
  const newer = gate.beginPage();
  assert.equal(gate.finishPage(generation, true), false);
  assert.equal(gate.refresh(), null);
  assert.equal(gate.finishPage(newer, false), true);
  assert.notEqual(gate.refresh(), null);
});

const { pageItems } = await import('../src/utils/pagination.ts');

test('numbered pagination shows the ends and the neighbours of the current page', () => {
  assert.deepEqual(pageItems(1, 1), [1]);
  assert.deepEqual(pageItems(2, 3), [1, 2, 3]);
  assert.deepEqual(pageItems(5, 10), [1, 'gap', 4, 5, 6, 'gap', 10]);
  assert.deepEqual(pageItems(1, 10), [1, 2, 'gap', 10]);
  assert.deepEqual(pageItems(10, 10), [1, 'gap', 9, 10]);
});
