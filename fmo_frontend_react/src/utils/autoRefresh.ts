
interface VisibilitySource {
  readonly visibilityState: DocumentVisibilityState;
  addEventListener(type: 'visibilitychange', listener: () => void): void;
  removeEventListener(type: 'visibilitychange', listener: () => void): void;
}

interface IntervalTimer {
  setInterval(handler: () => void, timeout: number): number;
  clearInterval(id: number): void;
}

interface AutoRefreshOptions {
  /** When data last loaded (ms since epoch), or null if it never has */
  lastSuccessAt?: () => number | null;
  source?: VisibilitySource;
  timer?: IntervalTimer;
  now?: () => number;
}


const TICK_SLACK_MS = 1_000;


export function startAutoRefresh(refresh: () => void, intervalMs: number, options: AutoRefreshOptions = {}): () => void {
  const { lastSuccessAt, source = document, timer = window, now = Date.now } = options;
  const due = (slack: number) => {
    const last = lastSuccessAt?.() ?? null;
    return last === null || now() - last >= intervalMs - slack;
  };
  const run = (slack: number) => {
    if (source.visibilityState !== 'hidden' && due(slack)) refresh();
  };
  const onShow = () => run(0);
  run(0);
  const id = timer.setInterval(() => run(TICK_SLACK_MS), intervalMs);
  source.addEventListener('visibilitychange', onShow);
  return () => {
    timer.clearInterval(id);
    source.removeEventListener('visibilitychange', onShow);
  };
}
