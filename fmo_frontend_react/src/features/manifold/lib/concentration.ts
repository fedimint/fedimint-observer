import { guardianThreshold } from '../../../utils/config.ts';

export type ConcentrationTone = 'ok' | 'warn' | 'crit';

/**
 * What holding `held` of `total` seats lets one Manifold Guardian identity do
 * on its own. Several identities may share one operator, so a low share is not
 * proof of independence.
 */
export function concentration(held: number, total: number): { tone: ConcentrationTone; label: string; detail: string } {
  const threshold = guardianThreshold(total);
  const tolerance = total - threshold;
  if (held >= threshold) {
    return { tone: 'crit', label: 'Controls consensus', detail: `Holds ${held} of ${total} seats, at least the threshold of ${threshold}: this Manifold Guardian alone can reach consensus.` };
  }
  if (held > tolerance) {
    return { tone: 'warn', label: 'Can halt consensus', detail: `Holds ${held} of ${total} seats; consensus needs ${threshold}, so this Manifold Guardian alone can stop it by going offline.` };
  }
  return { tone: 'ok', label: 'Cannot halt alone', detail: `Holds ${held} of ${total} seats; the federation tolerates ${tolerance} faulty ${tolerance === 1 ? 'guardian' : 'guardians'}. Another identity may share its operator.` };
}
