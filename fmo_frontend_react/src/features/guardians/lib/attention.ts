import type { GuardianWarning, Resilience } from './guardianModel';

/** Live status older than this no longer describes "right now" (the collector polls every minute) */
export const LIVE_STALE_AFTER_MS = 3 * 60_000;

export type Attention =
  | { kind: 'warnings'; warnings: readonly GuardianWarning[]; note: string | null }
  | { kind: 'clear'; message: string }
  | { kind: 'unknown'; message: string };

interface AttentionInput {
  warnings: readonly GuardianWarning[];
  res: Resilience;
  /** When the live status shown was generated; null when none has loaded */
  liveGeneratedAt: string | null;
  /** The latest request for live status failed */
  liveFailed: boolean;
  now: number;
}

/**
 * What the "Attention" list says. Nothing to warn about is only an all-clear
 * when the evidence is complete and current: live status that failed to load,
 * is stale, or leaves guardians unpolled says "unknown", never a green check.
 */
export function attention({ warnings, res, liveGeneratedAt, liveFailed, now }: AttentionInput): Attention {
  const generated = liveGeneratedAt ? Date.parse(liveGeneratedAt) : NaN;
  let gap: string | null = null;
  if (Number.isNaN(generated)) {
    gap = liveFailed
      ? 'The live guardian status could not be loaded, so the observer cannot tell whether anything needs attention.'
      : 'No live guardian status yet, so the observer cannot tell whether anything needs attention.';
  } else if (now - generated > LIVE_STALE_AFTER_MS) {
    gap = `The latest live guardian status is ${Math.round((now - generated) / 60_000)} minutes old, so it may not show current problems.`;
  } else if (res.total === 0) {
    gap = 'The federation config lists no guardians to check.';
  } else if (res.unknown > 0) {
    gap = `${res.unknown} of ${res.total} ${res.total === 1 ? 'guardian has' : 'guardians have'} not been polled recently, so the picture is incomplete.`;
  }

  if (warnings.length > 0) return { kind: 'warnings', warnings, note: gap };
  if (gap) return { kind: 'unknown', message: gap };
  return { kind: 'clear', message: 'Nothing needs attention: every guardian answers the observer and none reports falling behind.' };
}
