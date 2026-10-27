const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * How near the shutdown its guardians announced is. The badge colors follow it,
 * so a date far off informs without alarming.
 */
export type ShutdownStage = 'planned' | 'soon' | 'imminent' | 'over';

export interface Shutdown {
  stage: ShutdownStage;
  /** When the federation shuts down, in milliseconds */
  at: number;
  /** Days until then, counting a part of a day as one; 0 once over */
  daysLeft: number;
}

export function shutdownOf(expiryTimestamp: number | null, now: number = Date.now()): Shutdown | null {
  if (expiryTimestamp === null) return null;
  const at = expiryTimestamp * 1000;
  const daysLeft = Math.max(0, Math.ceil((at - now) / DAY_MS));
  const stage: ShutdownStage =
    at <= now ? 'over' : daysLeft <= 7 ? 'imminent' : daysLeft <= 30 ? 'soon' : 'planned';
  return { stage, at, daysLeft };
}

/** Whether a federation is past the shutdown its guardians announced */
export function isShutDown(expiryTimestamp: number | null, now: number = Date.now()): boolean {
  return shutdownOf(expiryTimestamp, now)?.stage === 'over';
}

export function shutdownDate(shutdown: Shutdown, month: 'short' | 'long' = 'short'): string {
  return new Date(shutdown.at).toLocaleDateString('en-US', { month, day: 'numeric', year: 'numeric' });
}

/** "in 3 days", "within a day" */
export function timeUntilShutdown(shutdown: Shutdown): string {
  return shutdown.daysLeft <= 1 ? 'within a day' : `in ${shutdown.daysLeft} days`;
}

export const SHUTDOWN_STYLES: Record<ShutdownStage, { badge: string; notice: string }> = {
  planned: {
    badge: 'bg-gray-100 text-gray-700 dark:bg-gray-700 dark:text-gray-200',
    notice: 'border-gray-300 bg-gray-50 text-gray-800 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-200',
  },
  soon: {
    badge: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-300',
    notice: 'border-yellow-300 bg-yellow-50 text-yellow-900 dark:border-yellow-800 dark:bg-yellow-950/40 dark:text-yellow-200',
  },
  imminent: {
    badge: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-300',
    notice: 'border-red-300 bg-red-50 text-red-900 dark:border-red-800 dark:bg-red-950/40 dark:text-red-200',
  },
  over: {
    badge: 'bg-gray-200 text-gray-600 dark:bg-gray-600 dark:text-gray-200',
    notice: 'border-gray-300 bg-gray-100 text-gray-800 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-200',
  },
};
