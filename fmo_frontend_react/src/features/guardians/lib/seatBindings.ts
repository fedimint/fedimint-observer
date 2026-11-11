import type { ManifoldSeatBindingsReport } from '../../../types/api';
import type { Tone } from '../../../components/ui/Status';

const BINDINGS: Record<ManifoldSeatBindingsReport['status'], { tone: Tone; label: string }> = {
  verified: { tone: 'ok', label: 'Seat bindings verified' },
  invalid: { tone: 'warn', label: 'Seat bindings failed verification' },
  not_published: { tone: 'neutral', label: 'No seat bindings published' },
  not_observed: { tone: 'neutral', label: 'Seat bindings not observed' },
};

export function seatBindingsSummary(report: ManifoldSeatBindingsReport | null) {
  if (!report) return { tone: 'neutral' as Tone, label: 'Not checked' };
  if (report.status === 'not_observed' && !report.has_meta_module) return { tone: 'neutral' as Tone, label: 'No meta module' };
  return BINDINGS[report.status];
}

/** What a seat-binding status means, in a sentence */
export function seatBindingsExplanation(report: ManifoldSeatBindingsReport | null): string {
  if (!report) return 'The observer could not load its seat-binding check.';
  switch (report.status) {
    case 'verified':
      return `Every guardian seat has a binding signed by its Manifold Guardian, and each matches this federation's config. ${report.distinct_guardians} distinct Manifold Guardian ${report.distinct_guardians === 1 ? 'identity operates' : 'identities operate'} the seats. Bindings prove who operates a seat, not that its operator holds a trust credential.`;
    case 'invalid':
      return `The federation publishes fedi:fman_seat_bindings, but it fails the Manifold specification's checks${report.error ? `: ${report.error}` : ''}. An invalid directory binds no seat, so no Manifold identity is shown.`;
    case 'not_published':
      return 'The guardians\' consensus metadata has no fedi:fman_seat_bindings field, so no seat is bound to a Manifold Guardian. That is normal for federations not run through Manifold.';
    case 'not_observed':
      return report.has_meta_module
        ? 'The observer has not read this federation\'s consensus metadata yet.'
        : 'This federation has no meta module, which is where Manifold seat bindings are published.';
  }
}
