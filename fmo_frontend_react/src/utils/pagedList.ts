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
  /** Cursors of a page asked for that has not arrived; null when none */
  pending: readonly string[] | null;
  /** Changes on every navigation, so asking for the same page again requests it again */
  attempt: number;
}

export type PagedListAction<T> =
  /** Another query is another list: back to its page 1 */
  | { type: 'query'; key: string }
  | { type: 'next'; cursor: string }
  | { type: 'previous' }
  /** A page arrived for the list and cursors it was requested with */
  | { type: 'loaded'; key: string; cursors: readonly string[]; data: T };

const FIRST_PAGE: readonly string[] = [];

export function initialPagedList<T>(key: string): PagedList<T> {
  return { key, shown: FIRST_PAGE, data: null, pending: null, attempt: 0 };
}

/** The cursors of the page to request: the one asked for, else the one shown */
export function requestedCursors<T>(state: PagedList<T>): readonly string[] {
  return state.pending ?? state.shown;
}

/** 1-based number of the page shown */
export function shownPageNumber<T>(state: PagedList<T>): number {
  return state.shown.length + 1;
}

function sameCursors(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((cursor, i) => cursor === b[i]);
}

export function pagedListReducer<T>(state: PagedList<T>, action: PagedListAction<T>): PagedList<T> {
  switch (action.type) {
    case 'query':
      if (action.key === state.key) return state;
      return { key: action.key, shown: FIRST_PAGE, data: state.data, pending: null, attempt: state.attempt + 1 };
    case 'next':
      return { ...state, pending: [...state.shown, action.cursor], attempt: state.attempt + 1 };
    case 'previous':
      if (state.shown.length === 0) {
        // Already on page 1: only a pending later page is dropped
        return state.pending === null ? state : { ...state, pending: null, attempt: state.attempt + 1 };
      }
      return { ...state, pending: state.shown.slice(0, -1), attempt: state.attempt + 1 };
    case 'loaded': {
      const requested = requestedCursors(state);
      // An answer for another list or another page is late: the reader moved on
      if (action.key !== state.key || !sameCursors(action.cursors, requested)) return state;
      // Keeps the requested array itself, so the request does not repeat
      return { ...state, shown: requested, data: action.data, pending: null };
    }
  }
}
