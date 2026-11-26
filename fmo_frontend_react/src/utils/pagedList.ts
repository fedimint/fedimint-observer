
export interface PagedList<T> {

  key: string;

  shown: readonly string[];

  data: T | null;

  pending: readonly string[] | null;

  request: readonly string[];

  attempt: number;
}

export type PagedListAction<T> =

  | { type: 'query'; key: string }
  | { type: 'next'; cursor: string }
  | { type: 'previous' }

  | { type: 'refresh' }

  | { type: 'retry' }

  | { type: 'loaded'; key: string; cursors: readonly string[]; data: T }

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
        return state.pending === null ? state : ask(state, state.shown);
      }
      return ask(state, state.shown.slice(0, -1));
    case 'refresh':
      return ask(state, state.shown);
    case 'retry':
      return ask(state, state.request);
    case 'loaded':
      if (action.key !== state.key || !sameCursors(action.cursors, state.request)) return state;
      return { ...state, shown: state.request, data: action.data, pending: null };
    case 'failed':
      if (action.key !== state.key || !sameCursors(action.cursors, state.request) || state.pending === null) return state;
      return { ...state, pending: null };
  }
}
