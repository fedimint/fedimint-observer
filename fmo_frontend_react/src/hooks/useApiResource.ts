import { useCallback, useEffect, useState, type DependencyList } from 'react';

type Status = 'loading' | 'success' | 'error';

interface ApiResource<T> {
  data: T | null;
  error: unknown;
  loading: boolean;
  failed: boolean;
  reload: () => void;
}

interface ApiResourceOptions {
  enabled?: boolean;
  resetOnReload?: boolean;
  /** Clear the data when a request fails, so earlier data never reads as this request's answer */
  resetOnError?: boolean;
  label?: string;
}


/** Aborts superseded requests. The identity of `load` is ignored, so
 * `deps` must include every reactive value it reads. */
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
