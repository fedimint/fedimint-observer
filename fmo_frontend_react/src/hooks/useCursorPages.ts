import { useEffect, useRef, useState } from 'react';
import { PageRequests } from '../utils/pageRequests';
import { startAutoRefresh } from '../utils/autoRefresh';

const NO_CURSORS: never[] = [];

interface CursorPageError {
  /** Which request failed: the refresh of page 1, or a later page */
  kind: 'refresh' | 'page';
  cause: unknown;
}

interface CursorPagesOptions<P, C> {
  /**
   * Loads page 1 (no cursor) or the page a cursor points to. Must be stable
   * (`useCallback`): a new function restarts the refresh.
   */
  fetchPage: (cursor: C | undefined, signal: AbortSignal) => Promise<P>;
  /** The cursor of the page after `page`, if there is one */
  nextCursor: (page: P) => C | null | undefined;
  /** How often page 1 refreshes while it is shown */
  refreshMs: number;
  /** False pauses the refresh, e.g. while the list is in a hidden tab */
  enabled?: boolean;
  /** Going back to page 1 also hides the error shown so far */
  clearErrorOnRestart?: boolean;
}

/**
 * A list the backend pages with a cursor, shown one page at a time. Page 1 is
 * kept current by an automatic refresh; that refresh pauses while a later page
 * is read, so the rows do not shift under the reader, and resumes on page 1.
 */
export function useCursorPages<P, C>({
  fetchPage,
  nextCursor,
  refreshMs,
  enabled = true,
  clearErrorOnRestart = false,
}: CursorPagesOptions<P, C>) {
  // Everything kept is tagged with the list (`fetchPage`) it came from, and only
  // the current list's is used: an answer for a query the reader has left can
  // never show up under the new one, whatever order it arrives in.
  // Page 1, kept current by the refresh, and the later page shown, if any
  const [latest, setLatest] = useState<{ source: unknown; data: P } | null>(null);
  const [later, setLater] = useState<{ source: unknown; cursors: C[]; data: P } | null>(null);
  const [error, setError] = useState<{ source: unknown; error: CursorPageError } | null>(null);
  // The list whose later page is loading, if any
  const [loadingSource, setLoadingSource] = useState<unknown>(null);
  const [restarts, setRestarts] = useState(0);
  const requests = useRef(new PageRequests());
  const lastSuccess = useRef<number | null>(null);
  const pageRequest = useRef<AbortController | null>(null);
  const laterPage = later?.source === fetchPage ? later : null;
  // The cursor of each later page on the way to the one shown; page 1 has none
  const cursors = laterPage?.cursors ?? NO_CURSORS as C[];
  const onLaterPage = cursors.length > 0;
  const latestPage = latest?.source === fetchPage ? latest.data : null;
  const page = laterPage ? laterPage.data : latestPage;
  const shownError = error?.source === fetchPage ? error.error : null;
  const loadingPage = loadingSource !== null && loadingSource === fetchPage;

  useEffect(() => () => pageRequest.current?.abort(), []);

  // Another query (a filter, the order, the page size) is another list: back
  // to its page 1. The old list's rows, cursors and error stop counting at once
  // through their tags.
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
    // A tab opened in the background loads when shown
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
    // `source` is a function: set it as a value, not as an updater
    setLoadingSource(() => source);
    try {
      const next = await fetchPage(cursor, controller.signal);
      // The position moves only now that the page has arrived
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
    /** The page shown */
    page,
    /** Page 1, which stays current while a later page is shown */
    latest: latestPage,
    /** 1-based number of the page shown */
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
