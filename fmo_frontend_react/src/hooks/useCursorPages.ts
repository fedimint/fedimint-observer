import { useEffect, useRef, useState } from 'react';
import { PageRequests } from '../utils/pageRequests';
import { startAutoRefresh } from '../utils/autoRefresh';

const NO_CURSORS: never[] = [];

interface CursorPageError {
  kind: 'refresh' | 'page';
  cause: unknown;
}

interface CursorPagesOptions<P, C> {
  /** Loads one page. Keep callback identity stable for the same query and change it for a new query. */
  fetchPage: (cursor: C | undefined, signal: AbortSignal) => Promise<P>;
  nextCursor: (page: P) => C | null | undefined;
  refreshMs: number;
  enabled?: boolean;
  clearErrorOnRestart?: boolean;
}

/** Pages through a cursor-based list. Page one refreshes only while it is shown. */
export function useCursorPages<P, C>({
  fetchPage,
  nextCursor,
  refreshMs,
  enabled = true,
  clearErrorOnRestart = false,
}: CursorPagesOptions<P, C>) {
  // Tags state by source so late responses cannot cross query boundaries.
  const [latest, setLatest] = useState<{ source: unknown; data: P } | null>(null);
  const [later, setLater] = useState<{ source: unknown; cursors: C[]; data: P } | null>(null);
  const [error, setError] = useState<{ source: unknown; error: CursorPageError } | null>(null);
  const [loadingSource, setLoadingSource] = useState<unknown>(null);
  const [restarts, setRestarts] = useState(0);
  const requests = useRef(new PageRequests());
  const lastSuccess = useRef<number | null>(null);
  const pageRequest = useRef<AbortController | null>(null);
  const laterPage = later?.source === fetchPage ? later : null;
  const cursors = laterPage?.cursors ?? NO_CURSORS as C[];
  const onLaterPage = cursors.length > 0;
  const latestPage = latest?.source === fetchPage ? latest.data : null;
  const page = laterPage ? laterPage.data : latestPage;
  const shownError = error?.source === fetchPage ? error.error : null;
  const loadingPage = loadingSource !== null && loadingSource === fetchPage;

  useEffect(() => () => pageRequest.current?.abort(), []);

  useEffect(() => {
    // The old list's page request must not land in the new one
    pageRequest.current?.abort();
    pageRequest.current = null;
    requests.current.reset();
    lastSuccess.current = null;
  }, [fetchPage]);

  useEffect(() => {
    if (!enabled || onLaterPage) return;
    const controller = new AbortController();
    const gate = requests.current;
    let inFlight = false;
    const refresh = async () => {
      if (inFlight) return;
      const generation = gate.refresh();
      if (generation === null) return;
      inFlight = true;
      try {
        const data = await fetchPage(undefined, controller.signal);
        if (!controller.signal.aborted && gate.acceptsRefresh(generation)) {
          lastSuccess.current = Date.now();
          setLatest({ source: fetchPage, data });
          setError(null);
        }
      } catch (cause) {
        if (!controller.signal.aborted && gate.acceptsRefresh(generation)) setError({ source: fetchPage, error: { kind: 'refresh', cause } });
      } finally {
        inFlight = false;
      }
    };
    const stop = startAutoRefresh(() => { void refresh(); }, refreshMs, { lastSuccessAt: () => lastSuccess.current });
    return () => {
      stop();
      controller.abort();
    };
  }, [fetchPage, refreshMs, enabled, onLaterPage, restarts]);

  const showLaterPage = async (cursor: C, nextCursors: C[]) => {
    if (loadingPage) return;
    const generation = requests.current.beginPage();
    if (generation === null) return;
    const source = fetchPage;
    const controller = new AbortController();
    pageRequest.current = controller;
    setLoadingSource(() => source);
    try {
      const next = await fetchPage(cursor, controller.signal);
      if (!controller.signal.aborted && requests.current.finishPage(generation, true)) {
        setLater({ source, cursors: nextCursors, data: next });
        setError(null);
      }
    } catch (cause) {
      if (!controller.signal.aborted && requests.current.finishPage(generation, false)) setError({ source, error: { kind: 'page', cause } });
    } finally {
      if (pageRequest.current === controller) {
        pageRequest.current = null;
        setLoadingSource(null);
      }
    }
  };

  /** Back to page 1, fetched again now however recent, with the refresh resumed */
  const restart = () => {
    pageRequest.current?.abort();
    pageRequest.current = null;
    requests.current.reset();
    lastSuccess.current = null;
    setLoadingSource(null);
    setLater(null);
    if (clearErrorOnRestart) setError(null);
    setRestarts((count) => count + 1);
  };

  const next = () => {
    const cursor = page === null ? null : nextCursor(page);
    if (cursor == null) return;
    void showLaterPage(cursor, [...cursors, cursor]);
  };

  const previous = () => {
    if (cursors.length <= 1) restart();
    else void showLaterPage(cursors[cursors.length - 2], cursors.slice(0, -1));
  };

  return {
    page,
    /** Last loaded page one, which can be stale while a later page is shown. */
    latest: latestPage,
    pageNumber: cursors.length + 1,
    onLaterPage,
    hasNext: page !== null && nextCursor(page) != null,
    loadingPage,
    error: shownError,
    next,
    previous,
    restart,
  };
}
