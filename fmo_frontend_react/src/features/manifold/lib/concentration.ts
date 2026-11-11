import { guardianThreshold } from '../../../utils/config.ts';

export type ConcentrationTone = 'ok' | 'warn' | 'crit';

/** What holding `held` of `total` seats lets one operator do on its own */
export function concentration(held: number, total: number): { tone: ConcentrationTone; label: string; detail: string } {
  const threshold = guardianThreshold(total);
  const tolerance = total - threshold;
  if (held >= threshold) {
    return { tone: 'crit', label: 'Controls consensus', detail: `Holds ${held} of ${total} seats, at least the threshold of ${threshold}: this operator alone can reach consensus.` };
  }
  if (held > tolerance) {
    return { tone: 'warn', label: 'Can halt consensus', detail: `Holds ${held} of ${total} seats; consensus needs ${threshold}, so this operator alone can stop it by going offline.` };
  }
  return { tone: 'ok', label: 'Independent', detail: `Holds ${held} of ${total} seats; the federation tolerates ${tolerance} faulty ${tolerance === 1 ? 'guardian' : 'guardians'}.` };
}
