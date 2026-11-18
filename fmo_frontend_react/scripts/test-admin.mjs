import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { test } from 'node:test';

const { authorizationHeader, NostrSignerError } = await import('../src/services/nostrAuth.ts');
const { inviteProblem, keyProblem, shortKey } = await import('../src/features/admin/lib/format.ts');
const { SessionTracker } = await import('../src/features/admin/lib/sessions.ts');

const PUBKEY = 'a'.repeat(64);
const URL = 'https://observer.test/api/federations';

// Signs nothing: returns what a signer would, so the header's content can be read back
function signer(change = (event) => event) {
  const asked = [];
  return {
    asked,
    getPublicKey: async () => PUBKEY,
    signEvent: async (event) => {
      asked.push(event);
      return change({ ...event, id: 'f'.repeat(64), sig: 'e'.repeat(128) });
    },
  };
}

function decode(header) {
  assert.match(header, /^Nostr [A-Za-z0-9+/]+=*$/);
  return JSON.parse(Buffer.from(header.slice('Nostr '.length), 'base64').toString('utf8'));
}

const tag = (event, name) => event.tags.filter((entry) => entry[0] === name).map((entry) => entry[1]);

async function failure(promise) {
  try {
    await promise;
  } catch (err) {
    return err;
  }
  assert.fail('expected signing to fail');
}

test('the event names the request: URL, method and the hash of the exact body', async () => {
  const body = JSON.stringify({ invite: 'fed1ü' });
  const before = Math.floor(Date.now() / 1000);
  const event = decode(await authorizationHeader(signer(), PUBKEY, { url: URL, method: 'PUT', body }));

  assert.equal(event.kind, 27235);
  assert.equal(event.pubkey, PUBKEY);
  assert.equal(event.content, '');
  assert.deepEqual(tag(event, 'u'), [URL]);
  assert.deepEqual(tag(event, 'method'), ['PUT']);
  // Over the UTF-8 bytes that are sent
  assert.deepEqual(tag(event, 'payload'), [createHash('sha256').update(body, 'utf8').digest('hex')]);
  assert.ok(event.created_at >= before && event.created_at <= before + 2);
});

test('a request without a body signs no payload hash', async () => {
  const event = decode(await authorizationHeader(signer(), PUBKEY, { url: URL, method: 'GET' }));
  assert.deepEqual(tag(event, 'payload'), []);
});

test('two requests alike are two different events', async () => {
  const fake = signer();
  await authorizationHeader(fake, PUBKEY, { url: URL, method: 'GET' });
  await authorizationHeader(fake, PUBKEY, { url: URL, method: 'GET' });
  const [first, second] = fake.asked.map((event) => tag(event, 'nonce')[0]);
  assert.match(first, /^[0-9a-f]{32}$/);
  assert.notEqual(first, second);
});

test('a signer that switched keys or changed the request is refused', async () => {
  const switched = signer((event) => ({ ...event, pubkey: 'b'.repeat(64) }));
  const otherUrl = signer((event) => ({ ...event, tags: [['u', 'https://evil.test/'], ...event.tags.slice(1)] }));
  const otherKind = signer((event) => ({ ...event, kind: 1 }));
  for (const fake of [switched, otherUrl, otherKind]) {
    const err = await failure(authorizationHeader(fake, PUBKEY, { url: URL, method: 'GET' }));
    assert.ok(err instanceof NostrSignerError);
  }
});

test('a declined signature fails with a message fit to show', async () => {
  const declining = { getPublicKey: async () => PUBKEY, signEvent: async () => Promise.reject(new Error('User rejected')) };
  const err = await failure(authorizationHeader(declining, PUBKEY, { url: URL, method: 'GET' }));
  assert.ok(err instanceof NostrSignerError);
  assert.doesNotMatch(err.message, /User rejected/);
});

test('invite codes and public keys are checked before asking for a signature', () => {
  assert.equal(inviteProblem('fed11qgqzc2nhwden5te0'), null);
  assert.equal(inviteProblem('FED11QGQZC2'), null);
  for (const bad of ['', 'lnbc1abc', 'fed1 with space', `fed1${'q'.repeat(5000)}`]) assert.ok(inviteProblem(bad), bad);

  assert.equal(keyProblem(`npub1${'q'.repeat(58)}`), null);
  assert.equal(keyProblem('A'.repeat(64)), null);
  for (const bad of ['', 'npub1short', 'a'.repeat(63), 'hello']) assert.ok(keyProblem(bad), bad);
  // A pasted secret key is called out, not just refused
  assert.match(keyProblem(`nsec1${'q'.repeat(58)}`), /secret key/);
});

test('keys are shortened only when long', () => {
  assert.equal(shortKey('short'), 'short');
  assert.equal(shortKey(`npub1${'q'.repeat(52)}uvwxyz`), 'npub1qqqqqqq…uvwxyz');
});

test('a late refusal ends only the session its request was made in', () => {
  const sessions = new SessionTracker();
  const first = sessions.begin();
  assert.ok(sessions.isCurrent(first));
  // Signed out, then in again with the same key: the first session's answers are stale
  sessions.end();
  assert.equal(sessions.isCurrent(first), false);
  const second = sessions.begin();
  assert.notEqual(second, first);
  assert.equal(sessions.isCurrent(first), false);
  assert.ok(sessions.isCurrent(second));
  // Requests made while signed out belong to no session
  assert.equal(sessions.isCurrent(undefined), false);
});
