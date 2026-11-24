interface Entry<T> {
  promise: Promise<T>;

  failedAt?: number;
}

interface LookupCacheOptions {

  failureTtlMs: number;

  now?: () => number;
}


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
