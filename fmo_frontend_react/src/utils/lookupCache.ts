interface Entry<T> {
  promise: Promise<T>;
  /** When the lookup failed, if it did */
  failedAt?: number;
}

interface LookupCacheOptions {
  /**
   * How long a failure is remembered before the key is tried again. A lookup
   * that fails (e.g. a federation whose guardians are unreachable) would
   * otherwise be retried on every refresh of the page that asks for it.
   */
  failureTtlMs: number;
  /** The clock, replaceable in tests */
  now?: () => number;
}

/**
 * Remembers settled lookups for the rest of the page session and shares one
 * request between concurrent callers. Failures are remembered for
 * `failureTtlMs`, then tried again.
 */
export class LookupCache<T> {
  private readonly entries = new Map<string, Entry<T>>();
  private readonly failureTtlMs: number;
  private readonly now: () => number;

  constructor({ failureTtlMs, now = Date.now }: LookupCacheOptions) {
    this.failureTtlMs = failureTtlMs;
    this.now = now;
  }

  get(key: string, load: () => Promise<T>): Promise<T> {
    const cached = this.entries.get(key);
    if (cached && (cached.failedAt === undefined || this.now() - cached.failedAt < this.failureTtlMs)) {
      return cached.promise;
    }
    const entry: Entry<T> = { promise: load() };
    this.entries.set(key, entry);
    entry.promise.catch(() => {
      entry.failedAt = this.now();
    });
    return entry.promise;
  }
}
