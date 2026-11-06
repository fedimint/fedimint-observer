export type HistoryContent = 'loading' | 'unavailable' | 'empty' | 'ready';

/** Which request failed: the latest page, or an older page */
export type HistoryError = 'refresh' | 'older';

/**
 * What the wallet history sections show. Once a page has loaded, later refresh
 * or pagination errors are reported beside it and never hide it.
 */
export function historyContent(loaded: boolean, error: HistoryError | null, count: number): HistoryContent {
  if (!loaded) return error ? 'unavailable' : 'loading';
  return count === 0 ? 'empty' : 'ready';
}

export function historyAlert(loaded: boolean, error: HistoryError | null): string | null {
  switch (error) {
    case null:
      return null;
    case 'older':
      // The loaded operations are still current; only the next page is missing
      return 'Older operations could not load. Try again with the button below.';
    case 'refresh':
      return loaded
        ? 'Wallet history could not refresh. The data below is the last successful observation.'
        : 'Wallet history could not load. Retrying automatically.';
  }
}
