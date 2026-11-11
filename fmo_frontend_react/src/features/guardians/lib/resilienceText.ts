import type { Tone } from '../../../components/ui/Status';
import type { Resilience } from './guardianModel';

export const RESILIENCE: Record<Resilience['level'], { tone: Tone; label: string }> = {
  operational: { tone: 'ok', label: 'Consensus healthy' },
  at_threshold: { tone: 'warn', label: 'At threshold' },
  below_threshold: { tone: 'crit', label: 'Below threshold' },
  unknown: { tone: 'neutral', label: 'Not determined' },
};

/** The sentence under the resilience figure */
export function resilienceDetail(res: Resilience): string {
  if (res.level === 'unknown') return 'Not enough recent polls to tell';
  if (res.total === 1) return 'Single-guardian federation';
  const spare = res.online - res.threshold;
  if (spare > 0) return `${spare} more can fail before consensus stops`;
  if (spare === 0) return 'No further guardian can fail';
  return `${res.threshold - res.online} more needed for consensus`;
}
