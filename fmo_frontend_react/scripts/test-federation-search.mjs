import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  isInviteCodeQuery,
  lookupForInput,
  normalizeSearchInput,
  observedSessionSpan,
  searchKey,
} from '../src/features/federations/lib/federationList.ts';

test('normalizeSearchInput cleans whitespace and protocol schemes', () => {
  assert.equal(normalizeSearchInput('  fed1example  '), 'fed1example');
  assert.equal(normalizeSearchInput('fedimint://fed1example'), 'fed1example');
  assert.equal(normalizeSearchInput('fedimint:fed1example'), 'fed1example');
  assert.equal(normalizeSearchInput('web+fedimint://fed1example'), 'fed1example');
  assert.equal(normalizeSearchInput('web+fedimint:fed1example'), 'fed1example');
  assert.equal(normalizeSearchInput('Alpha Federation'), 'Alpha Federation');
  assert.equal(normalizeSearchInput('245806c0fa1a'), '245806c0fa1a');
});

test('isInviteCodeQuery recognizes Fedimint invite codes and ignores other queries', () => {
  assert.equal(isInviteCodeQuery('fed11qgqpyyrhwwaz7tes9cczuvpwxqyhnqvps'), true);
  assert.equal(isInviteCodeQuery('FED11QGQPYYRH...'), true);
  assert.equal(isInviteCodeQuery('fedimint://fed11qgqpy...'), true);
  assert.equal(isInviteCodeQuery('  fed11qgqpy...  '), true);

  assert.equal(isInviteCodeQuery('fedimint1qgqpyyrhwwaz'), true);
  assert.equal(isInviteCodeQuery('FEDIMINT1...'), true);
  assert.equal(isInviteCodeQuery('fedimint://fedimint...'), true);

  assert.equal(isInviteCodeQuery('Mutiny Federation'), false);
  assert.equal(isInviteCodeQuery('fed'), false);
  assert.equal(isInviteCodeQuery('245806c0fa1a9412e8477ee3a4a7536d5b00c3c52a921d7b3da27d2c3df440e8'), false);
});

test('lookupForInput returns a lookup only for the query that started it', () => {
  const queryA = 'fed11_query_for_fed_a';
  const queryB = 'fed11_query_for_fed_b';
  const doneA = { query: searchKey(queryA), status: 'done', federationId: '245806c0fa1a9412e8477ee3a4a7536d5b00c3c52a921d7b3da27d2c3df440e8' };

  assert.equal(lookupForInput(null, queryA), null);

  assert.equal(lookupForInput(doneA, queryB), null);

  assert.equal(lookupForInput(doneA, ''), null);

  assert.equal(lookupForInput(doneA, queryA), doneA);
  assert.equal(lookupForInput(doneA, `  fedimint://${queryA.toUpperCase()} `), doneA);

  const loadingA = { query: searchKey(queryA), status: 'loading' };
  const errorA = { query: searchKey(queryA), status: 'error', message: 'not found' };
  assert.equal(lookupForInput(loadingA, queryB), null);
  assert.equal(lookupForInput(errorA, queryB), null);
  assert.equal(lookupForInput(errorA, queryA), errorA);
});

test('observedSessionSpan shows both bounds as a UTC range with an estimate note', () => {
  const span = observedSessionSpan(1_715_375_314, 1_725_984_472);
  assert.equal(span.text, 'May 10, 2024 – Sep 10, 2024');
  assert.match(span.title, /^May 10, 2024, 21:08 UTC – Sep 10, 2024, 16:07 UTC\. /);
  assert.match(span.title, /not announced start or shutdown dates/);
});

test('observedSessionSpan reads a missing bound, or an older server omitting both, as unavailable', () => {
  assert.equal(observedSessionSpan(1_715_375_314, null).text, 'Dates unavailable');
  assert.equal(observedSessionSpan(null, null).text, 'Dates unavailable');
  assert.equal(observedSessionSpan(undefined, undefined).text, 'Dates unavailable');
  assert.doesNotMatch(observedSessionSpan(undefined, undefined).title, /UTC –/);
});
