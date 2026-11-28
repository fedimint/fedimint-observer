
/** Normalizes API failures while preserving caller cancellations. */

type ApiErrorKind =
  | 'network' // the request never reached the server, or the connection broke
  | 'timeout' // no complete answer within the request's time limit
  | 'http' // the server answered with an error status
  | 'invalid_response'; // the server answered OK with a body that is not JSON

export class ApiError extends Error {
  readonly kind: ApiErrorKind;

  readonly status: number | null;

  readonly requestId: string | null;

  constructor(kind: ApiErrorKind, message: string, status: number | null = null, requestId: string | null = null) {
    super(message);
    this.name = 'ApiError';
    this.kind = kind;
    this.status = status;
    this.requestId = requestId;
  }


  /** Whether retrying the same request may succeed. */
  get retryable(): boolean {
    if (this.kind === 'http') return this.status === null || this.status >= 500 || this.status === 408 || this.status === 429;
    return this.kind !== 'invalid_response';
  }
}


const DEFAULT_TIMEOUT_MS = 35_000;
const MAX_SERVER_MESSAGE_LENGTH = 300;

interface RequestOptions {
  signal?: AbortSignal;
  method?: 'GET' | 'PUT';

  json?: unknown;
  timeoutMs?: number;
  /** Ask the server even when a shared GET of the same URL is still fresh */
  fresh?: boolean;
}

const shared = new Map<string, { startedAt: number; ttlMs: number; body: Promise<unknown> }>();

/** Sends a request and returns its parsed JSON body, reusing a shared GET of the same URL that is still fresh */
export function request<T>(url: string, options: RequestOptions = {}): Promise<T> {
  const reusable = !options.fresh && (options.method ?? 'GET') === 'GET' && options.json === undefined;
  const reused = reusable ? sharedAnswer(url) : undefined;
  return (reused ? untilAborted(reused, options.signal) : send(url, options, true)) as Promise<T>;
}

/** Starts a GET whose answer requests for the same URL reuse for `ttlMs`; a failed one is forgotten at once */
export function share(url: string, ttlMs: number): Promise<unknown> {
  const body = send(url, {}, true);
  shared.set(url, { startedAt: Date.now(), ttlMs, body });
  body.catch(() => {
    if (shared.get(url)?.body === body) shared.delete(url);
  });
  return body;
}

/** A GET answered from the shared one when it is fresh, otherwise shared for `ttlMs` */
export function requestShared<T>(url: string, ttlMs: number, signal?: AbortSignal): Promise<T> {
  return untilAborted(sharedAnswer(url) ?? share(url, ttlMs), signal) as Promise<T>;
}

function sharedAnswer(url: string): Promise<unknown> | undefined {
  const entry = shared.get(url);
  if (entry && Date.now() - entry.startedAt < entry.ttlMs) return entry.body;
  shared.delete(url);
  return undefined;
}

function untilAborted<T>(body: Promise<T>, signal: AbortSignal | undefined): Promise<T> {
  if (!signal) return body;
  return new Promise((resolve, reject) => {
    if (signal.aborted) return reject(signal.reason);
    const aborted = () => reject(signal.reason);
    signal.addEventListener('abort', aborted, { once: true });
    body.then(
      (value) => {
        signal.removeEventListener('abort', aborted);
        resolve(value);
      },
      (error: unknown) => {
        signal.removeEventListener('abort', aborted);
        reject(error);
      },
    );
  });
}

/** Sends a request whose answer has no body the caller needs */
export async function requestNoContent(url: string, options: RequestOptions = {}): Promise<void> {
  await send(url, options, false);
}

async function send(url: string, { signal, method = 'GET', json, timeoutMs = DEFAULT_TIMEOUT_MS }: RequestOptions, parseBody: boolean): Promise<unknown> {
  signal?.throwIfAborted();

  const controller = new AbortController();
  const forwardAbort = () => controller.abort(signal?.reason);
  signal?.addEventListener('abort', forwardAbort, { once: true });
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    controller.abort();
  }, timeoutMs);

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
      const body = await response.text().catch((err: unknown) => {
        if (signal?.aborted || timedOut) throw transportError(err);
        return '';
      });
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
