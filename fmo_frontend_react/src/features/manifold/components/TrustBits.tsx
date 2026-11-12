import { Icon } from '../../../components/ui/Icon';
import { KeyChip } from '../../../components/ui/KeyChip';
import { StatusPill, type Tone } from '../../../components/ui/Status';
import { formatRelative, formatUtcDateTime } from '../../../utils/time';
import { formatPriceMsats } from '../lib/directory';
import type { AdFreshness, CredentialCheck, CredentialState, MgPlan, TrustCredential } from '../types';

const CREDENTIAL: Record<CredentialState, { tone: Tone; label: string; title: string }> = {
  verified: { tone: 'manifold', label: 'Verified', title: 'Every trust-credential check passed' },
  failed: { tone: 'crit', label: 'Verification failed', title: 'A trust-credential check failed' },
  unavailable: { tone: 'warn', label: 'Unverified', title: 'The checks could not complete, so the credential is not trusted' },
  none: { tone: 'neutral', label: 'No credential', title: 'The advertisement presents no trust credential' },
};

/** The trust-credential outcome; "Verified" only when every check passed */
export function VerificationBadge({ state, size }: { state: CredentialState; size?: 'sm' | 'md' }) {
  const { tone, label, title } = CREDENTIAL[state];
  return (
    <StatusPill tone={tone} icon={state === 'verified' ? 'checkCircle' : undefined} size={size} title={title}>
      {label}
    </StatusPill>
  );
}

const FRESHNESS: Record<AdFreshness, { tone: Tone; label: string; title: string }> = {
  fresh: { tone: 'ok', label: 'Fresh', title: 'Issued within 2 hours and not expired' },
  stale: { tone: 'warn', label: 'Stale', title: 'Older than 2 hours; Manifold clients ignore it' },
  expired: { tone: 'neutral', label: 'Expired', title: 'Past its signed expiry; the guardian is not taking seats through it' },
  not_yet_valid: { tone: 'warn', label: 'Future-dated', title: 'Issued in the future; ignored until then' },
};

export function FreshnessPill({ freshness, issuedAt }: { freshness: AdFreshness | null; issuedAt?: number }) {
  if (!freshness) return <StatusPill tone="neutral">No advertisement</StatusPill>;
  const { tone, label, title } = FRESHNESS[freshness];
  return (
    <span className="inline-flex items-center gap-1.5">
      <StatusPill tone={tone} title={title} icon={freshness === 'fresh' ? 'clock' : undefined}>{label}</StatusPill>
      {issuedAt !== undefined && (
        <span className="text-xs text-ink-3" title={formatUtcDateTime(issuedAt * 1000)}>{formatRelative(issuedAt * 1000)}</span>
      )}
    </span>
  );
}

export function PlanList({ plans }: { plans: readonly MgPlan[] }) {
  if (plans.length === 0) return <span className="text-xs text-ink-3">No plans advertised</span>;
  return (
    <ul className="space-y-1.5">
      {plans.map((plan, index) => (
        <li key={index} className="flex items-center justify-between gap-3 rounded-lg border border-line bg-surface-2 px-3 py-2 text-xs">
          <span className="text-ink-2">{plan.kind === 'infinite_best_effort' ? 'Best effort, no expiry' : `Subscription (${plan.period})`}</span>
          <span className="text-right font-medium text-ink tabular">
            {plan.kind === 'infinite_best_effort'
              ? formatPriceMsats(plan.price_msats)
              : <>{formatPriceMsats(plan.initial_price_msats)} <span className="font-normal text-ink-3">then {formatPriceMsats(plan.renewal_price_msats)}</span></>}
          </span>
        </li>
      ))}
    </ul>
  );
}

const OUTCOMES = [
  { key: 'failed', title: 'Failed', icon: 'xCircle', tone: 'text-crit' },
  { key: 'skipped', title: 'Not run', icon: 'minusCircle', tone: 'text-ink-3' },
  { key: 'passed', title: 'Passed', icon: 'checkCircle', tone: 'text-ok' },
] as const;

/** The checks grouped by outcome, then by reason, so each reason is stated once */
function CheckGroups({ checks }: { checks: readonly CredentialCheck[] }) {
  return (
    <div className="space-y-3 border-t border-line pt-3">
      {OUTCOMES.map((outcome) => {
        const group = checks.filter((check) => (check.passed === true ? 'passed' : check.passed === false ? 'failed' : 'skipped') === outcome.key);
        if (group.length === 0) return null;
        const byReason = new Map<string, CredentialCheck[]>();
        for (const check of group) byReason.set(check.detail ?? '', [...(byReason.get(check.detail ?? '') ?? []), check]);
        return (
          <div key={outcome.key}>
            <h4 className="mb-1.5 flex items-center gap-1.5 text-[0.7rem] font-semibold uppercase tracking-wide text-ink-3">
              <Icon name={outcome.icon} className={`size-3.5 ${outcome.tone}`} />
              {outcome.title} · {group.length}
            </h4>
            <div className="space-y-2 pl-5">
              {[...byReason].map(([reason, items]) => (
                <div key={reason || 'none'}>
                  <ul className="space-y-0.5 text-xs" aria-label={`${outcome.title} checks`}>
                    {items.map((check) => (
                      <li key={check.id} className={outcome.key === 'skipped' ? 'text-ink-2' : 'text-ink'}>{check.label}</li>
                    ))}
                  </ul>
                  {reason && <p className="mt-0.5 text-[0.7rem] text-ink-3">{reason}</p>}
                </div>
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** Every credential check with its outcome, the evidence behind a verification badge */
export function TrustEvidence({ credential }: { credential: TrustCredential }) {
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <VerificationBadge state={credential.state} size="md" />
        {credential.checked_at && <span className="text-xs text-ink-3">Checked {formatRelative(credential.checked_at)}</span>}
      </div>
      {credential.reason && <p className="text-xs leading-relaxed text-ink-2">{credential.reason}</p>}
      {credential.state !== 'none' && (
        <dl className="grid grid-cols-[7.5rem_minmax(0,1fr)] gap-x-3 gap-y-2 text-xs">
          <dt className="text-ink-3">Issuer</dt>
          <dd className="min-w-0">
            {credential.issuer_label && <div className="mb-1 text-ink">{credential.issuer_label}</div>}
            {credential.issuer_pubkey ? <KeyChip value={credential.issuer_pubkey} label="credential issuer key" /> : '—'}
          </dd>
          <dt className="text-ink-3">Holder (operator)</dt>
          <dd className="min-w-0">{credential.holder_pubkey ? <KeyChip value={credential.holder_pubkey} label="credential holder key" /> : '—'}</dd>
          <dt className="text-ink-3">Trust level</dt>
          <dd className="text-ink tabular">
            {credential.trust_level ?? '—'} <span className="text-ink-3">(at least {credential.required_trust_level} required)</span>
          </dd>
          <dt className="text-ink-3">Schema</dt>
          <dd className="font-mono text-ink-2">{credential.schema ?? '—'}</dd>
        </dl>
      )}
      {credential.checks.length > 0 && <CheckGroups checks={credential.checks} />}
      <p className="text-[0.7rem] leading-relaxed text-ink-3">
        A valid Nostr signature alone does not establish trust. "Verified" needs a pinned trust-root issuer, both PeerBadge proofs, no revocation at any issuer location and a trust level of at least {credential.required_trust_level}; until every check has run and passed, the credential stays unverified.
      </p>
    </div>
  );
}
