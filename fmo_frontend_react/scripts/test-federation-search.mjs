import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  groupFederations,
  isInviteCodeQuery,
  lookupForInput,
  normalizeSearchInput,
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

const mockFederation = (id, name, invite, extra = {}) => ({
  id,
  name,
  invite,
  last_7d_activity: [],
  deposits: null,
  nostr_votes: { count: 0, avg: null },
  health: 'healthy',
  expiry_timestamp: null,
  successor: null,
  successor_id: null,
  ...extra,
});

test('groupFederations matches by name, hex ID, canonical invite, and resolved invite ID', () => {
  const fedA = mockFederation('245806c0fa1a9412e8477ee3a4a7536d5b00c3c52a921d7b3da27d2c3df440e8', 'Mutiny Federation', 'fed11_mutiny_peer0');
  const fedB = mockFederation('a1b2c3d4e5f60000000000000000000000000000000000000000000000000000', 'Chama Mint', 'fed11_chama_peer0');
  const fedC = mockFederation('f9e8d7c6b5a40000000000000000000000000000000000000000000000000000', null, 'fed11_anonymous_peer0');

  const federations = [fedA, fedB, fedC];

  const all = groupFederations(federations, '', 'name', 'asc', null);
  assert.equal(all.active.length, 3);

  const byName = groupFederations(federations, 'mutiny', 'name', 'asc', null);
  assert.equal(byName.active.length, 1);
  assert.equal(byName.active[0].id, fedA.id);

  const byNameUpper = groupFederations(federations, 'CHAMA', 'name', 'asc', null);
  assert.equal(byNameUpper.active.length, 1);
  assert.equal(byNameUpper.active[0].id, fedB.id);

  const byHexPrefix = groupFederations(federations, '245806c0fa', 'name', 'asc', null);
  assert.equal(byHexPrefix.active.length, 1);
  assert.equal(byHexPrefix.active[0].id, fedA.id);

  const byHexCase = groupFederations(federations, 'A1B2C3D4', 'name', 'asc', null);
  assert.equal(byHexCase.active.length, 1);
  assert.equal(byHexCase.active[0].id, fedB.id);

  const byCanonicalInvite = groupFederations(federations, 'mutiny_peer0', 'name', 'asc', null);
  assert.equal(byCanonicalInvite.active.length, 1);
  assert.equal(byCanonicalInvite.active[0].id, fedA.id);

  const nonCanonicalQuery = 'fed11qgqpyyrhwwaz_peer1_different_invite_code';
  const byResolvedInvite = groupFederations(federations, nonCanonicalQuery, 'name', 'asc', null, fedB.id);
  assert.equal(byResolvedInvite.active.length, 1);
  assert.equal(byResolvedInvite.active[0].id, fedB.id);

  const bySingleChar = groupFederations(federations, 'chama', 'name', 'asc', null);
  assert.equal(bySingleChar.active.length, 1);
  assert.equal(bySingleChar.active[0].id, fedB.id);

  const byShortNonPrefix = groupFederations(federations, '06c0', 'name', 'asc', null);
  assert.equal(byShortNonPrefix.active.length, 0);

  const byLongSuffix = groupFederations(federations, 'df440e8', 'name', 'asc', null);
  assert.equal(byLongSuffix.active.length, 1);
  assert.equal(byLongSuffix.active[0].id, fedA.id);

  const bySchemeOnly = groupFederations(federations, 'fedimint:', 'name', 'asc', null);
  assert.equal(bySchemeOnly.active.length, 0);

  const anonymousMatch = groupFederations(federations, 'f9e8d7', 'name', 'asc', null);
  assert.equal(anonymousMatch.active.length, 1);
  assert.equal(anonymousMatch.active[0].id, fedC.id);

  const none = groupFederations(federations, 'nonexistent query', 'name', 'asc', null);
  assert.equal(none.active.length, 0);
});

test('lookupForInput returns a lookup only for the query that started it', () => {
  const fedA = mockFederation('245806c0fa1a9412e8477ee3a4a7536d5b00c3c52a921d7b3da27d2c3df440e8', 'Mutiny Federation', 'fed11_mutiny');
  const fedB = mockFederation('a1b2c3d4e5f60000000000000000000000000000000000000000000000000000', 'Chama Mint', 'fed11_chama');
  const federations = [fedA, fedB];
  const queryA = 'fed11_query_for_fed_a';
  const queryB = 'fed11_query_for_fed_b';
  const doneA = { query: searchKey(queryA), status: 'done', federationId: fedA.id };

  assert.equal(lookupForInput(null, queryA), null);

  assert.equal(lookupForInput(doneA, queryB), null);
  assert.equal(groupFederations(federations, queryB, 'name', 'asc', null, null).active.length, 0);

  assert.equal(lookupForInput(doneA, ''), null);

  assert.equal(lookupForInput(doneA, queryA), doneA);
  assert.equal(lookupForInput(doneA, `  fedimint://${queryA.toUpperCase()} `), doneA);
  const groups = groupFederations(federations, queryA, 'name', 'asc', null, doneA.federationId);
  assert.deepEqual(groups.active.map((f) => f.id), [fedA.id]);

  const loadingA = { query: searchKey(queryA), status: 'loading' };
  const errorA = { query: searchKey(queryA), status: 'error', message: 'not found' };
  assert.equal(lookupForInput(loadingA, queryB), null);
  assert.equal(lookupForInput(errorA, queryB), null);
  assert.equal(lookupForInput(errorA, queryA), errorA);
});
