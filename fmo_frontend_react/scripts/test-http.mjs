import assert from 'node:assert/strict';
import { afterEach, test } from 'node:test';

const { ApiError, request, requestNoContent, requestShared, share } = await import('../src/services/http.ts');
const { federationRoute } = await import('../src/utils/federationRoute.ts');

const realFetch = globalThis.fetch;
afterEach(() => {
  globalThis.fetch = realFetch;
});

function answer(status, body, headers = {}) {
  globalThis.fetch = async () => new Response(body, { status, headers });
}

const json = { 'content-type': 'application/json' };

async function failure(promise) {
  try {
    await promise;
  } catch (err) {
    return err;
  }
  assert.fail('expected the request to fail');
}

test('a successful answer is parsed as JSON', async () => {
  answer(200, JSON.stringify({ total: 3 }), json);
  assert.deepEqual(await request('https://api.test/x'), { total: 3 });
});

test("a rejected request shows the server's own message, without a reference", async () => {
  answer(404, JSON.stringify({ error: 'Federation has no Wallet v1 module' }), { ...json, 'x-request-id': '42' });
  const err = await failure(request('https://api.test/x'));
  assert.ok(err instanceof ApiError);
  assert.equal(err.kind, 'http');
  assert.equal(err.status, 404);
  assert.equal(err.requestId, '42');
  assert.equal(err.message, 'Federation has no Wallet v1 module');
  assert.equal(err.retryable, false);
});

test('a server fault names the request so it can be found in the logs', async () => {
  answer(500, JSON.stringify({ error: 'Internal server error' }), { ...json, 'x-request-id': '7' });
  const err = await failure(request('https://api.test/x'));
  assert.equal(err.message, 'Internal server error (ref 7)');
  assert.equal(err.retryable, true);
});

test("a proxy's HTML page or the server's plain-text limits are never shown", async () => {
  answer(502, '<html><body><h1>502 Bad Gateway</h1></body></html>', { 'content-type': 'text/html' });
  const proxy = await failure(request('https://api.test/x'));
  assert.equal(proxy.message, 'The observer is busy or restarting, try again in a moment');
  assert.equal(proxy.retryable, true);

  answer(503, 'Server is busy', { 'content-type': 'text/plain', 'x-request-id': '9' });
  const busy = await failure(request('https://api.test/x'));
  assert.equal(busy.message, 'The observer is busy or restarting, try again in a moment (ref 9)');

  answer(504, 'Request timed out', { 'content-type': 'text/plain' });
  assert.equal((await failure(request('https://api.test/x'))).message, 'The observer did not answer in time');
});

test('a JSON body without a usable message falls back to the status', async () => {
  answer(400, JSON.stringify({ error: 42 }), json);
  assert.equal((await failure(request('https://api.test/x'))).message, 'The observer rejected the request (HTTP 400)');

  answer(400, JSON.stringify({ error: 'x'.repeat(301) }), json);
  assert.equal((await failure(request('https://api.test/x'))).message, 'The observer rejected the request (HTTP 400)');

  answer(429, '', json);
  const limited = await failure(request('https://api.test/x'));
  assert.equal(limited.message, 'Too many requests, try again in a moment');
  assert.equal(limited.retryable, true);
});

test("a request that never reaches the server replaces the browser's text", async () => {
  globalThis.fetch = async () => {
    throw new TypeError('Failed to fetch');
  };
  const err = await failure(request('https://api.test/x'));
  assert.equal(err.kind, 'network');
  assert.equal(err.message, 'Could not reach the observer');
  assert.equal(err.status, null);
  assert.equal(err.retryable, true);
});

// Rejects once the request's signal aborts, as fetch does
function hang() {
  globalThis.fetch = (_url, init) => new Promise((_resolve, reject) => {
    init.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')));
  });
}

test('a request that takes too long times out', async () => {
  hang();
  const err = await failure(request('https://api.test/x', { timeoutMs: 10 }));
  assert.equal(err.kind, 'timeout');
  assert.equal(err.message, 'The observer did not answer in time');
  assert.equal(err.retryable, true);
});

test('the time limit also covers reading the body', async () => {
  globalThis.fetch = async (_url, init) => {
    const body = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('{"partial":'));
        init.signal.addEventListener('abort', () => controller.error(new DOMException('aborted', 'AbortError')));
      },
    });
    return new Response(body, { status: 200, headers: json });
  };
  assert.equal((await failure(request('https://api.test/x', { timeoutMs: 10 }))).kind, 'timeout');
});

test("a request its caller cancels rejects with the caller's abort, not an ApiError", async () => {
  hang();
  const controller = new AbortController();
  const pending = request('https://api.test/x', { signal: controller.signal });
  controller.abort();
  const err = await failure(pending);
  assert.ok(!(err instanceof ApiError));
  assert.equal(err.name, 'AbortError');

  let sent = false;
  globalThis.fetch = async () => {
    sent = true;
    return new Response('{}');
  };
  assert.equal((await failure(request('https://api.test/x', { signal: controller.signal }))).name, 'AbortError');
  assert.equal(sent, false);
});

test('an OK answer that is not JSON is reported as unreadable', async () => {
  answer(200, '<!doctype html><title>app</title>', { 'content-type': 'text/html', 'x-request-id': '5' });
  const err = await failure(request('https://api.test/x'));
  assert.equal(err.kind, 'invalid_response');
  assert.equal(err.message, 'The observer sent an answer this page could not read (ref 5)');
  assert.equal(err.retryable, false);
});

test('a PUT sends its JSON body, and a no-content request ignores the answer body', async () => {
  let seen;
  globalThis.fetch = async (url, init) => {
    seen = { url, method: init.method, type: init.headers['Content-Type'], body: init.body };
    return new Response('not json', { status: 200 });
  };
  await requestNoContent('https://api.test/nostr', { method: 'PUT', json: { kind: 1 } });
  assert.deepEqual(seen, { url: 'https://api.test/nostr', method: 'PUT', type: 'application/json', body: '{"kind":1}' });
});

// An error answer whose body never finishes, failing once the request's signal aborts
function hangingErrorBody(status) {
  globalThis.fetch = async (_url, init) => {
    const body = new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('{"error":'));
        init.signal.addEventListener('abort', () => controller.error(new DOMException('aborted', 'AbortError')));
      },
    });
    return new Response(body, { status, headers: json });
  };
}

test('a timeout while reading an error body stays a timeout', async () => {
  hangingErrorBody(500);
  assert.equal((await failure(request('https://api.test/x', { timeoutMs: 10 }))).kind, 'timeout');
});

test('a cancel while reading an error body stays a cancel', async () => {
  hangingErrorBody(500);
  const controller = new AbortController();
  const pending = request('https://api.test/x', { signal: controller.signal });
  setTimeout(() => controller.abort(), 10);
  const err = await failure(pending);
  assert.ok(!(err instanceof ApiError));
  assert.equal(err.name, 'AbortError');
});

test('an error body that breaks off keeps the HTTP error, without the server message', async () => {
  globalThis.fetch = async () => {
    const body = new ReadableStream({ start(controller) { controller.error(new TypeError('connection reset')); } });
    return new Response(body, { status: 503, headers: json });
  };
  const err = await failure(request('https://api.test/x'));
  assert.equal(err.kind, 'http');
  assert.equal(err.message, 'The observer is busy or restarting, try again in a moment');
});

function counted(status, body) {
  const calls = { count: 0 };
  globalThis.fetch = async () => {
    calls.count += 1;
    return new Response(body, { status, headers: json });
  };
  return calls;
}

test('a request reuses a shared GET of the same URL instead of asking again', async () => {
  const calls = counted(200, JSON.stringify({ id: 1 }));
  share('https://api.test/shared', 10_000);
  assert.deepEqual(await request('https://api.test/shared'), { id: 1 });
  assert.deepEqual(await requestShared('https://api.test/shared', 10_000), { id: 1 });
  assert.equal(calls.count, 1);
  await request('https://api.test/other');
  assert.equal(calls.count, 2);
});

test('a failed shared GET is forgotten, so a retry asks again', async () => {
  const calls = counted(503, 'busy');
  await failure(share('https://api.test/failing', 10_000));
  await failure(request('https://api.test/failing'));
  assert.equal(calls.count, 2);
});

test('a shared GET is asked again once its time is up', async () => {
  const calls = counted(200, '{}');
  await share('https://api.test/expiring', 0);
  await request('https://api.test/expiring');
  assert.equal(calls.count, 2);
});

test('aborting one reader of a shared GET leaves the answer for the others', async () => {
  const calls = counted(200, JSON.stringify({ id: 2 }));
  const controller = new AbortController();
  const aborted = requestShared('https://api.test/readers', 10_000, controller.signal);
  controller.abort();
  await failure(aborted);
  assert.deepEqual(await requestShared('https://api.test/readers', 10_000), { id: 2 });
  assert.deepEqual(await request('https://api.test/readers'), { id: 2 });
  assert.equal(calls.count, 1);
});

test('a fresh request asks the server even while a shared GET is fresh', async () => {
  const answers = [{ health: 'offline' }, { health: 'online' }];
  let calls = 0;
  globalThis.fetch = async () => new Response(JSON.stringify(answers[calls++]), { status: 200, headers: json });
  await share('https://api.test/recheck', 10_000);
  assert.deepEqual(await request('https://api.test/recheck', { fresh: true }), { health: 'online' });
  assert.equal(calls, 2);
  assert.deepEqual(await request('https://api.test/recheck'), { health: 'offline' });
  assert.equal(calls, 2);
});

test('a federation path is read for prefetching, and an undecodable id is skipped', () => {
  assert.deepEqual(federationRoute('/federations/abc%20def/Gateways/'), { id: 'abc def', tab: 'gateways' });
  assert.deepEqual(federationRoute('/federations/abc'), { id: 'abc', tab: '' });
  assert.equal(federationRoute('/federations/%E0%A4%A'), null);
  assert.equal(federationRoute('/manifold/guardians'), null);
});
