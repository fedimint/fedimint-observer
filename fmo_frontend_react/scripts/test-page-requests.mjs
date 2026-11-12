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

// The gateway list's position (useGatewayOverview), clicked through every
// order of answers and failures the hook can see.
import { initialPagedList, pagedListReducer, requestedCursors, shownPageNumber } from '../src/utils/pagedList.ts';

const step = (state, ...actions) => actions.reduce(pagedListReducer, state);
const loaded = (state, data) => ({ type: 'loaded', key: state.key, cursors: requestedCursors(state), data });

test('Next moves the page number only once the next page arrives', () => {
  let s = step(initialPagedList('q'));
  s = step(s, loaded(s, 'page 1'));
  s = step(s, { type: 'next', cursor: 'c2' });
  assert.equal(shownPageNumber(s), 1, 'still page 1 while page 2 loads');
  assert.equal(s.data, 'page 1');
  assert.deepEqual(requestedCursors(s), ['c2']);
  s = step(s, loaded(s, 'page 2'));
  assert.equal(shownPageNumber(s), 2);
  assert.equal(s.data, 'page 2');
});

test('a failed Next keeps page 1, and Next again requests page 2 again', () => {
  let s = step(initialPagedList('q'));
  s = step(s, loaded(s, 'page 1'));
  s = step(s, { type: 'next', cursor: 'c2' });
  const firstAttempt = s.attempt;
  // The request fails: nothing is dispatched, the rows and the number stay
  assert.equal(shownPageNumber(s), 1);
  s = step(s, { type: 'next', cursor: 'c2' });
  assert.equal(shownPageNumber(s), 1, 'retrying cannot advance over stale rows');
  assert.deepEqual(requestedCursors(s), ['c2'], 'the same page, not page 3');
  assert.notEqual(s.attempt, firstAttempt, 'a new attempt, so the hook requests it again');
  s = step(s, loaded(s, 'page 2'));
  assert.equal(shownPageNumber(s), 2);
});

test('Previous returns through the pages, again only once each arrives', () => {
  let s = step(initialPagedList('q'));
  s = step(s, loaded(s, 'p1'), { type: 'next', cursor: 'c2' });
  s = step(s, loaded(s, 'p2'), { type: 'next', cursor: 'c3' });
  s = step(s, loaded(s, 'p3'));
  assert.equal(shownPageNumber(s), 3);
  s = step(s, { type: 'previous' });
  assert.equal(shownPageNumber(s), 3);
  assert.deepEqual(requestedCursors(s), ['c2']);
  s = step(s, loaded(s, 'p2 again'));
  assert.equal(shownPageNumber(s), 2);
  s = step(s, { type: 'previous' });
  s = step(s, loaded(s, 'p1 again'));
  assert.equal(shownPageNumber(s), 1);
  assert.equal(s.data, 'p1 again');
});

test('Previous on page 1 only drops a pending page', () => {
  let s = step(initialPagedList('q'));
  s = step(s, loaded(s, 'p1'));
  assert.equal(step(s, { type: 'previous' }), s);
  s = step(s, { type: 'next', cursor: 'c2' }, { type: 'previous' });
  assert.equal(s.pending, null);
  assert.equal(shownPageNumber(s), 1);
});

test('a refresh of the page shown replaces its rows without moving', () => {
  let s = step(initialPagedList('q'));
  s = step(s, loaded(s, 'p1'), { type: 'next', cursor: 'c2' });
  s = step(s, loaded(s, 'p2'));
  const before = requestedCursors(s);
  s = step(s, loaded(s, 'p2 refreshed'));
  assert.equal(s.data, 'p2 refreshed');
  assert.equal(shownPageNumber(s), 2);
  assert.equal(requestedCursors(s), before, 'same cursors array, so the request is not repeated');
});

test('a query change during a request goes back to page 1 and drops the late answer', () => {
  let s = step(initialPagedList('q1'));
  s = step(s, loaded(s, 'q1 p1'), { type: 'next', cursor: 'c2' });
  const inFlight = loaded(s, 'q1 p2'); // requested before the query changed
  s = step(s, { type: 'query', key: 'q2' });
  assert.equal(shownPageNumber(s), 1);
  assert.deepEqual(requestedCursors(s), []);
  s = step(s, inFlight);
  assert.equal(s.data, 'q1 p1', 'the old query\'s page 2 never lands in the new list');
  assert.equal(shownPageNumber(s), 1);
  s = step(s, loaded(s, 'q2 p1'));
  assert.equal(s.data, 'q2 p1');
});

test('an answer for a page the reader has left is ignored', () => {
  let s = step(initialPagedList('q'));
  s = step(s, loaded(s, 'p1'), { type: 'next', cursor: 'c2' });
  const late = loaded(s, 'p2');
  s = step(s, { type: 'previous' }); // gave up on page 2 while it loaded
  s = step(s, late);
  assert.equal(shownPageNumber(s), 1);
  assert.equal(s.data, 'p1');
});
