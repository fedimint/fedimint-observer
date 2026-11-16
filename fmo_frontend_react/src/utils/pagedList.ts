// Where a cursor-paged list is, and the page it shows. Pure, so every order of
// clicks, answers and failures can be tested without rendering.
//
// The position only moves once the page asked for has arrived: a failed
// request leaves the page number on the rows still shown, and asking for the
// same page again requests it again.

export interface PagedList<T> {
  /** The list (query) the cursors and the page belong to */
  key: string;
  /** Cursors leading to the page shown; page 1 has none */
  shown: readonly string[];
  /** The page shown, or the previous query's page while the new one loads */
  data: T | null;
  /** Cursors of a page navigated to that has not arrived; null when none */
  pending: readonly string[] | null;
  /**
   * Cursors of the page last asked for: `pending` while navigating, else the
   * page shown. A failed navigation clears `pending` but keeps this, so the
   * failure does not start another request and Try again can repeat it.
   */
  request: readonly string[];
  /** Changes on every request asked for, so asking for the same page again requests it again */
  attempt: number;
}

export type PagedListAction<T> =
  /** Another query is another list: back to its page 1 */
  | { type: 'query'; key: string }
  | { type: 'next'; cursor: string }
  | { type: 'previous' }
  /** A background refresh: reload the page shown, giving up a failed navigation */
  | { type: 'refresh' }
  /** Try again: repeat the last request, a failed navigation included */
  | { type: 'retry' }
  /** A page arrived for the list and cursors it was requested with */
  | { type: 'loaded'; key: string; cursors: readonly string[]; data: T }
  /** The request for the list and cursors given failed */
  | { type: 'failed'; key: string; cursors: readonly string[] };

const FIRST_PAGE: readonly string[] = [];

export function initialPagedList<T>(key: string): PagedList<T> {
  return { key, shown: FIRST_PAGE, data: null, pending: null, request: FIRST_PAGE, attempt: 0 };
}

/** The cursors of the page to request */
export function requestedCursors<T>(state: PagedList<T>): readonly string[] {
  return state.request;
}

/** 1-based number of the page shown */
export function shownPageNumber<T>(state: PagedList<T>): number {
  return state.shown.length + 1;
}

function sameCursors(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((cursor, i) => cursor === b[i]);
}

/** Asks for `target`: the page shown again, or another page to navigate to */
function ask<T>(state: PagedList<T>, target: readonly string[]): PagedList<T> {
  const navigating = !sameCursors(target, state.shown);
  return {
    ...state,
    pending: navigating ? target : null,
    // The page shown is requested with its own array, so an answer keeps `shown` as it is
    request: navigating ? target : state.shown,
    attempt: state.attempt + 1,
  };
}

export function pagedListReducer<T>(state: PagedList<T>, action: PagedListAction<T>): PagedList<T> {
  switch (action.type) {
    case 'query':
      if (action.key === state.key) return state;
      return { key: action.key, shown: FIRST_PAGE, data: state.data, pending: null, request: FIRST_PAGE, attempt: state.attempt + 1 };
    case 'next':
      return ask(state, [...state.shown, action.cursor]);
    case 'previous':
      if (state.shown.length === 0) {
        // Already on page 1: only a pending later page is dropped
        return state.pending === null ? state : ask(state, state.shown);
      }
      return ask(state, state.shown.slice(0, -1));
    case 'refresh':
      return ask(state, state.shown);
    case 'retry':
      return ask(state, state.request);
    case 'loaded':
      // An answer for another list or another page is late: the reader moved on
      if (action.key !== state.key || !sameCursors(action.cursors, state.request)) return state;
      return { ...state, shown: state.request, data: action.data, pending: null };
    case 'failed':
      if (action.key !== state.key || !sameCursors(action.cursors, state.request) || state.pending === null) return state;
      // The page shown stays, and so does what the next refresh requests: the
      // page shown, not the one that failed. Next asks for it again.
      return { ...state, pending: null };
  }
}
