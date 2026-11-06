/**
 * Remembers settled lookups for the rest of the page session and shares one
 * request between concurrent callers. Failures are forgotten, so they are
 * tried again next time.
 */
export class LookupCache<T> {
  private readonly entries = new Map<string, Promise<T>>();

  get(key: string, load: () => Promise<T>): Promise<T> {
    const cached = this.entries.get(key);
    if (cached) return cached;
    const pending = load();
    this.entries.set(key, pending);
    pending.catch(() => {
      if (this.entries.get(key) === pending) this.entries.delete(key);
    });
    return pending;
  }
}
