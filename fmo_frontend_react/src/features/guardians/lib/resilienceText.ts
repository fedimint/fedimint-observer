import type { Tone } from '../../../components/ui/Status';
import type { Resilience } from './guardianModel';

export const RESILIENCE: Record<Resilience['level'], { tone: Tone; label: string }> = {
  operational: { tone: 'ok', label: 'Above threshold' },
  at_threshold: { tone: 'warn', label: 'At threshold' },
  below_threshold: { tone: 'crit', label: 'Below threshold' },
  unknown: { tone: 'neutral', label: 'Not determined' },
};

/** The sentence under the reachability figure: answers to the observer against the consensus threshold */
export function resilienceDetail(res: Resilience): string {
  if (res.level === 'unknown') return 'Not enough recent polls to tell';
  if (res.total === 1) return 'Single-guardian federation';
  const spare = res.online - res.threshold;
  if (spare > 0) return `${spare} more can stop answering before it drops below`;
  if (spare === 0) return 'One more not answering drops it below';
  return `${res.threshold - res.online} short of the threshold`;
}
