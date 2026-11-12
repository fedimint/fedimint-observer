/**
 * The one way the app talks to the observer API. Every failure becomes an
 * `ApiError` of a known kind with a message fit to show, so no browser text
 * ("Failed to fetch", "Load failed"), status-only message or raw response body
 * (a proxy's HTML error page) ever reaches the page. A request cancelled by its
 * caller rejects with the caller's abort reason, unchanged, as `fetch` does.
 */

type ApiErrorKind =
  | 'network' // the request never reached the server, or the connection broke
  | 'timeout' // no complete answer within the request's time limit
  | 'http' // the server answered with an error status
  | 'invalid_response'; // the server answered OK with a body that is not JSON

export class ApiError extends Error {
  readonly kind: ApiErrorKind;
  /** The HTTP status, for `http` errors */
  readonly status: number | null;
  /** The server's `x-request-id`, which finds the request in the server's logs */
  readonly requestId: string | null;

  constructor(kind: ApiErrorKind, message: string, status: number | null = null, requestId: string | null = null) {
    super(message);
    this.name = 'ApiError';
    this.kind = kind;
    this.status = status;
    this.requestId = requestId;
  }

  /**
   * Whether the same request may succeed later. A request the server rejected
   * (4xx: bad input, unknown federation, missing module) fails the same way again.
   */
  get retryable(): boolean {
    if (this.kind === 'http') return this.status === null || this.status >= 500 || this.status === 408 || this.status === 429;
    return this.kind !== 'invalid_response';
  }
}

/** Above the server's own 30s limit, which answers 504, so the server's answer arrives first */
const DEFAULT_TIMEOUT_MS = 35_000;
// The server's messages are short sentences; anything longer is not one of them
const MAX_SERVER_MESSAGE_LENGTH = 300;

interface RequestOptions {
  signal?: AbortSignal;
  method?: 'GET' | 'PUT';
  /** Sent as a JSON body */
  json?: unknown;
  timeoutMs?: number;
}

/** Sends a request and returns its parsed JSON body */
export function request<T>(url: string, options: RequestOptions = {}): Promise<T> {
  return send(url, options, true) as Promise<T>;
}

/** Sends a request whose answer has no body the caller needs */
export async function requestNoContent(url: string, options: RequestOptions = {}): Promise<void> {
  await send(url, options, false);
}

async function send(url: string, { signal, method = 'GET', json, timeoutMs = DEFAULT_TIMEOUT_MS }: RequestOptions, parseBody: boolean): Promise<unknown> {
  signal?.throwIfAborted();

  // One signal for both the caller's cancel and the time limit, so the limit also
  // covers reading the body
  const controller = new AbortController();
  const forwardAbort = () => controller.abort(signal?.reason);
  signal?.addEventListener('abort', forwardAbort, { once: true });
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, timeoutMs);

  // A failed fetch or body read: the caller's cancel stays as it was, the rest get a kind
  const transportError = (err: unknown): unknown => {
    if (signal?.aborted) return err;
    if (timedOut) return new ApiError('timeout', 'The observer did not answer in time');
    return new ApiError('network', 'Could not reach the observer');
  };

  try {
    let response: Response;
    try {
      response = await fetch(url, {
        method,
        signal: controller.signal,
        ...(json === undefined ? {} : { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(json) }),
      });
    } catch (err) {
      throw transportError(err);
    }

    const requestId = response.headers.get('x-request-id');
    if (!response.ok) {
      // A body that can't be read only loses the server's message, not the error
      const body = await response.text().catch(() => '');
      throw httpError(response.status, serverMessage(response, body), requestId);
    }
    if (!parseBody) return undefined;

    let body: string;
    try {
      body = await response.text();
    } catch (err) {
      throw transportError(err);
    }
    try {
      return JSON.parse(body);
    } catch {
      throw new ApiError('invalid_response', withReference('The observer sent an answer this page could not read', requestId), response.status, requestId);
    }
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', forwardAbort);
  }
}

// The backend answers errors with `{ "error": "<message fit to show>" }`. Anything
// else (plain text from the server's busy or timeout limits, a proxy's HTML page)
// is not shown.
function serverMessage(response: Response, body: string): string | null {
  if (!response.headers.get('content-type')?.includes('application/json')) return null;
  try {
    const parsed: unknown = JSON.parse(body);
    if (!parsed || typeof parsed !== 'object' || !('error' in parsed) || typeof parsed.error !== 'string') return null;
    const message = parsed.error.trim();
    return message && message.length <= MAX_SERVER_MESSAGE_LENGTH ? message : null;
  } catch {
    return null;
  }
}

function httpError(status: number, message: string | null, requestId: string | null): ApiError {
  const text = message ?? statusMessage(status);
  // A server fault is worth reporting, so it names the request; a rejected request explains itself
  return new ApiError('http', status >= 500 ? withReference(text, requestId) : text, status, requestId);
}

function statusMessage(status: number): string {
  switch (status) {
    case 404:
      return 'Not found';
    case 429:
      return 'Too many requests, try again in a moment';
    case 502:
    case 503:
      return 'The observer is busy or restarting, try again in a moment';
    case 504:
      return 'The observer did not answer in time';
    default:
      return status >= 500 ? `The observer had an internal error (HTTP ${status})` : `The observer rejected the request (HTTP ${status})`;
  }
}

function withReference(message: string, requestId: string | null): string {
  return requestId ? `${message} (ref ${requestId})` : message;
}
