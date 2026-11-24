type HistoryContent = 'loading' | 'unavailable' | 'empty' | 'ready';


export type HistoryError = 'refresh' | 'older';


export function historyContent(loaded: boolean, error: HistoryError | null, count: number): HistoryContent {
  if (!loaded) return error ? 'unavailable' : 'loading';
  return count === 0 ? 'empty' : 'ready';
}

export function historyAlert(loaded: boolean, error: HistoryError | null): string | null {
  switch (error) {
    case null:
      return null;
    case 'older':
      return 'That page of operations could not load. Try the page arrows again.';
    case 'refresh':
      return loaded
        ? 'Wallet history could not refresh. The data below is the last successful observation.'
        : 'Wallet history could not load. Retrying automatically.';
  }
}
