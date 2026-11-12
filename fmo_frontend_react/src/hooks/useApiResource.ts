import { useCallback, useEffect, useState, type DependencyList } from 'react';

type Status = 'loading' | 'success' | 'error';

interface ApiResource<T> {
  /** The last successful answer, or null before one arrived */
  data: T | null;
  /** What the latest request threw; null unless `failed` */
  error: unknown;
  /** A request is running, or the first one has not started yet */
  loading: boolean;
  /** The latest request failed */
  failed: boolean;
  /** Repeats the request with the same inputs */
  reload: () => void;
}

interface ApiResourceOptions {
  /** While false no request is made; the resource stays `loading` until enabled */
  enabled?: boolean;
  /** Clear the data when a new request starts, instead of keeping it shown while it loads */
  resetOnReload?: boolean;
  /** Clear the data when a request fails, so earlier data never reads as this request's answer */
  resetOnError?: boolean;
  /** Failures are logged as "Failed to fetch <label>:" */
  label?: string;
}

/**
 * One API request tied to the component's lifetime: it is made when `deps`
 * change, the previous one is aborted, and answers to aborted requests are
 * ignored, so a slow earlier answer never overwrites a later one.
 *
 * `load` runs with the values of the render in which `deps` last changed, like
 * the body of an effect, so `deps` must list every input it reads.
 */
export function useApiResource<T>(
  load: (signal: AbortSignal) => Promise<T>,
  deps: DependencyList,
  { enabled = true, resetOnReload = false, resetOnError = false, label }: ApiResourceOptions = {},
): ApiResource<T> {
  const [state, setState] = useState<{ status: Status; data: T | null; error: unknown }>({
    status: 'loading',
    data: null,
    error: null,
  });
  const [reloads, setReloads] = useState(0);

  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    const { signal } = controller;
    setState((previous) => ({ status: 'loading', data: resetOnReload ? null : previous.data, error: null }));
    load(signal).then(
      (data) => {
        if (!signal.aborted) setState({ status: 'success', data, error: null });
      },
      (error: unknown) => {
        if (signal.aborted) return;
        if (label) console.error(`Failed to fetch ${label}:`, error);
        setState((previous) => ({ status: 'error', data: resetOnError ? null : previous.data, error }));
      },
    );
    return () => controller.abort();
    // `deps` stands in for `load`, as documented above
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [enabled, reloads, resetOnReload, resetOnError, label, ...deps]);

  const reload = useCallback(() => setReloads((count) => count + 1), []);

  return {
    data: state.data,
    error: state.error,
    loading: state.status === 'loading',
    failed: state.status === 'error',
    reload,
  };
}
