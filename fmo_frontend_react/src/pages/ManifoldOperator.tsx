import type { ReactNode } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Card, CardHeader } from '../components/ui/Card';
import { DataStamp } from '../components/ui/DataStamp';
import { Icon } from '../components/ui/Icon';
import { KeyChip } from '../components/ui/KeyChip';
import { PageHeader } from '../components/ui/PageHeader';
import { EmptyState, ErrorState, LoadingBlock, Skeleton } from '../components/ui/States';
import { StatusPill } from '../components/ui/Status';
import { InfoTip } from '../components/ui/Tooltip';
import { formatUtcDate, formatUtcDateTime } from '../utils/time';
import { useManifoldGuardian } from '../features/manifold/hooks';
import { concentration } from '../features/manifold/lib/concentration';
import { formatSizes } from '../features/manifold/lib/directory';
import { FreshnessPill, PlanList, TrustEvidence, VerificationBadge } from '../features/manifold/components/TrustBits';
import type { ManifoldGuardianProfile, OperatedSeat } from '../features/manifold/types';

function groupSeats(seats: readonly OperatedSeat[]) {
  const groups = new Map<string, OperatedSeat[]>();
  for (const seat of seats) groups.set(seat.federation_id, [...(groups.get(seat.federation_id) ?? []), seat]);
  return [...groups.values()];
}

function FederationsOperated({ profile }: { profile: ManifoldGuardianProfile }) {
  if (profile.seats.length === 0) {
    return (
      <EmptyState icon="key" title="No verified seats">
        No observed federation's fedi:fman_seat_bindings names this Manifold Guardian. An advertisement alone is never taken as membership.
      </EmptyState>
    );
  }
  return (
    <ul className="divide-y divide-line">
      {groupSeats(profile.seats).map((seats) => {
        const first = seats[0];
        const risk = concentration(seats.length, first.federation_guardians);
        const name = first.federation_name ?? 'Unnamed federation';
        return (
          <li key={first.federation_id} className="flex flex-col gap-2 py-3 first:pt-0 last:pb-0 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0">
              <Link to={`/federations/${encodeURIComponent(first.federation_id)}`} className="text-sm font-medium text-accent-ink hover:underline">{name}</Link>
              <p className="mt-0.5 text-xs text-ink-3">
                {seats.length === 1 ? 'Seat' : 'Seats'} {seats.map((seat) => seat.peer_id).join(', ')} · bound {formatUtcDate(Math.min(...seats.map((s) => s.bound_at)) * 1000)} · verified {formatUtcDateTime(first.verified_at)}
              </p>
            </div>
            <span title={risk.detail}><StatusPill tone={risk.tone}>{risk.label} · {seats.length}/{first.federation_guardians}</StatusPill></span>
          </li>
        );
      })}
    </ul>
  );
}

function PresenceHistory({ profile }: { profile: ManifoldGuardianProfile }) {
  // Only days since the observer started polling: before that there is no evidence either way
  const firstPolled = profile.observation_history.findIndex((day) => day.polls > 0);
  if (firstPolled === -1) return <EmptyState icon="clock" title="Not polled yet">The observer has not polled the relays for this guardian yet.</EmptyState>;
  const days = profile.observation_history.slice(firstPolled);
  const polled = days.filter((day) => day.polls > 0);
  const totalPolls = polled.reduce((sum, day) => sum + day.polls, 0);
  const average = polled.reduce((sum, day) => sum + day.fresh_polls, 0) / totalPolls;
  const share = (day: (typeof days)[number]) => (day.polls > 0 ? day.fresh_polls / day.polls : null);
  return (
    <div>
      <p className="text-sm text-ink">
        <span className="text-lg font-semibold tabular">{(average * 100).toFixed(1)}%</span>
        <span className="text-ink-3"> of {totalPolls.toLocaleString('en-US')} relay {totalPolls === 1 ? 'poll' : 'polls'} over {polled.length} {polled.length === 1 ? 'day' : 'days'} found a fresh advertisement</span>
      </p>
      {days.length > 1 && (
        <>
          <div className="mt-3 flex h-16 items-end gap-[2px]" role="img" aria-label={`Daily share of polls that found a fresh advertisement, ${days.length} days`}>
            {days.map((day) => {
              const value = share(day);
              return (
                <div key={day.date} className="flex h-full min-w-[3px] max-w-6 flex-1 items-end" title={`${formatUtcDate(day.date)}: ${value === null ? 'not polled' : `${day.fresh_polls} of ${day.polls} polls found a fresh advertisement`}`}>
                  {value === null
                    ? <div className="cell-empty h-full w-full rounded-[2px]" />
                    : <div className={`w-full rounded-t-[3px] ${value >= 0.95 ? 'bg-manifold' : 'bg-warn'}`} style={{ height: `${Math.max(4, value * 100)}%` }} />}
                </div>
              );
            })}
          </div>
          <div className="mt-1.5 flex justify-between text-[0.65rem] text-ink-3">
            <span>{formatUtcDate(days[0].date, { month: 'short', day: 'numeric' })}</span>
            <span>Today</span>
          </div>
        </>
      )}
      <p className="mt-2 text-xs text-ink-3">
        This shows the guardian kept offering seats; it says nothing about how its federations' guardians performed.
      </p>
    </div>
  );
}

function VersionHistory({ profile }: { profile: ManifoldGuardianProfile }) {
  if (profile.version_history.length === 0) return <EmptyState icon="layers" title="No version changes observed" />;
  return (
    <ol className="space-y-2.5">
      {profile.version_history.map((change, index) => (
        <li key={change.observed_at} className="flex items-start gap-3 text-xs">
          <span className={`mt-1 size-2 shrink-0 rounded-full ${index === 0 ? 'bg-manifold' : 'bg-line-strong'}`} aria-hidden="true" />
          <div className="min-w-0">
            <p className="font-mono text-ink">{change.fedimintd_version}{change.mg_version && <span className="text-ink-3"> · MG {change.mg_version}</span>}</p>
            <p className="text-ink-3">First seen by the observer {formatUtcDateTime(change.observed_at)}{index === 0 && ' · current'}</p>
          </div>
        </li>
      ))}
    </ol>
  );
}

function TechnicalRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-ink-3">{label}</dt>
      <dd className="min-w-0 text-ink-2">{children}</dd>
    </>
  );
}

export function ManifoldOperator() {
  const { pubkey = '' } = useParams<{ pubkey: string }>();
  const resource = useManifoldGuardian(pubkey);
  const profile = resource.data?.data ?? null;
  const trail = [
    { label: 'Manifold' },
    { label: 'Guardian directory', to: '/manifold/guardians' },
  ];

  if (resource.failed) {
    return <div><PageHeader title="Manifold Guardian" breadcrumbs={trail} /><ErrorState title="Could not load this Manifold Guardian" onRetry={resource.reload} /></div>;
  }
  if (resource.loading && !resource.data) {
    return (
      <div role="status" aria-label="Loading profile" className="space-y-4">
        <Skeleton className="h-4 w-32" />
        <Skeleton className="h-8 w-64" />
        <LoadingBlock label="Loading profile" rows={6} />
      </div>
    );
  }
  if (!profile) {
    return (
      <div>
        <PageHeader title="Manifold Guardian" breadcrumbs={trail} description={<KeyChip value={pubkey} label="requested key" />} />
        <EmptyState icon="directory" title="The observer has not seen this Manifold Guardian">
          No advertisement or verified seat binding names this key.
        </EmptyState>
      </div>
    );
  }

  const ad = profile.advertisement;
  return (
    <div>
      <PageHeader
        breadcrumbs={[...trail, { label: <span className="font-mono">{profile.display_name}</span> }]}
        title={<span className="font-mono">{profile.display_name}</span>}
        badges={<>
          <VerificationBadge state={profile.credential.state} size="md" />
          <FreshnessPill freshness={profile.freshness} />
        </>}
        description={
          <span className="inline-flex flex-wrap items-center gap-2">
            <KeyChip value={profile.fman_pubkey} label="Manifold Guardian public key" head={12} tail={10} />
            <InfoTip label="About the name" align="start">
              The name is derived from the key by Manifold's naming rule. It is for recognition only: two keys can share a name, so always compare keys.
            </InfoTip>
          </span>
        }
        actions={<DataStamp kind="live" at={resource.data!.observed_at} staleAfterSeconds={900} />}
      />

      <div className="space-y-4 sm:space-y-5">

        <div className="grid gap-4 sm:gap-5 lg:grid-cols-3">
          <div className="min-w-0 space-y-4 sm:space-y-5 lg:col-span-2">
            <Card aria-labelledby="operated-title">
              <CardHeader
                id="operated-title"
                title="Federations operated"
                description="Seats bound to this Manifold Guardian in federations' verified seat-binding directories, with how concentrated its control is"
              />
              <FederationsOperated profile={profile} />
            </Card>

            <Card aria-labelledby="offer-title">
              <CardHeader id="offer-title" title="Advertised offer" description="What the guardian says it offers; not verified" />
              {!ad ? <EmptyState title="No current advertisement" /> : (
                <div className="space-y-3 text-xs">
                  <dl className="grid grid-cols-[7.5rem_minmax(0,1fr)] gap-x-3 gap-y-2">
                    <dt className="text-ink-3">Fedimint</dt>
                    <dd className="font-mono text-ink">{ad.availability.fedimintd_version}</dd>
                    <dt className="text-ink-3">Federation sizes</dt>
                    <dd className="text-ink tabular">{formatSizes(ad.availability.federation_sizes)} guardians</dd>
                    {ad.software && <>
                      <dt className="text-ink-3">MG software</dt>
                      <dd className="font-mono text-ink">{ad.software.mg_version} <span className="text-ink-3">(API {ad.software.api_version})</span></dd>
                    </>}
                    <dt className="text-ink-3">Self-reported uptime</dt>
                    <dd className="text-ink">
                      {ad.self_reported_uptime_permille ? (
                        <span className="tabular">
                          {(['1d', '7d', '30d'] as const).filter((key) => ad.self_reported_uptime_permille?.[key] !== undefined)
                            .map((key) => `${key} ${(ad.self_reported_uptime_permille![key]! / 10).toFixed(1)}%`).join(' · ')}
                          <span className="block text-ink-3">Asserted by the guardian; not measured by the observer</span>
                        </span>
                      ) : <span className="text-ink-3">Not reported</span>}
                    </dd>
                  </dl>
                  <div>
                    <h3 className="mb-1.5 text-[0.68rem] font-medium uppercase tracking-wide text-ink-3">Setup plans</h3>
                    <PlanList plans={ad.plans} />
                  </div>
                </div>
              )}
            </Card>

            <Card aria-labelledby="presence-title">
              <CardHeader
                id="presence-title"
                title="Advertisement presence"
                description="Share of the observer's relay polls each day that found a fresh advertisement"
                meta={<DataStamp kind="historical" at={resource.data!.observed_at} scope="30 days" />}
              />
              <PresenceHistory profile={profile} />
            </Card>

            <Card aria-labelledby="versions-title">
              <CardHeader id="versions-title" title="Software version changes" description="Versions this guardian advertised since the observer started collecting, newest first" />
              <VersionHistory profile={profile} />
            </Card>

            <Card aria-labelledby="technical-title">
              <details className="group">
                <summary className="flex cursor-pointer list-none items-center justify-between gap-2">
                  <h2 id="technical-title" className="text-[0.95rem] font-semibold text-ink">Technical details</h2>
                  <Icon name="chevronRight" className="size-4 text-ink-3 transition-transform group-open:rotate-90" />
                </summary>
                <dl className="mt-3 grid grid-cols-[7rem_minmax(0,1fr)] gap-x-3 gap-y-2.5 text-xs">
                  <TechnicalRow label="Nostr identity"><KeyChip value={profile.fman_pubkey} label="Nostr public key" full /></TechnicalRow>
                  {ad && <>
                    <TechnicalRow label="Service key"><KeyChip value={ad.service_pubkey} label="service public key" /></TechnicalRow>
                    <TechnicalRow label="Iroh endpoints">
                      <ul className="space-y-1">{ad.api_endpoints.map((endpoint) => <li key={endpoint.url} className="break-all font-mono text-[0.7rem]">{endpoint.url}</li>)}</ul>
                    </TechnicalRow>
                    <TechnicalRow label="Advertisement">
                      Kind {ad.event_kind} {ad.event_kind === 37709 ? '(v2)' : '(legacy)'} · signature {ad.signature_valid ? 'valid' : 'invalid'}
                      <div className="mt-1"><KeyChip value={ad.event_id} label="advertisement event id" /></div>
                    </TechnicalRow>
                    <TechnicalRow label="Validity">{formatUtcDateTime(ad.issued_at * 1000)} → {formatUtcDateTime(ad.expires_at * 1000)}</TechnicalRow>
                    <TechnicalRow label="Authorizations">{ad.holder_authorization_count} holder {ad.holder_authorization_count === 1 ? 'authorization' : 'authorizations'}</TechnicalRow>
                  </>}
                  <TechnicalRow label="Relays">
                    <ul className="space-y-0.5">{profile.nostr_relays.map((relay) => <li key={relay} className="break-all font-mono text-[0.7rem]">{relay}</li>)}</ul>
                  </TechnicalRow>
                </dl>
              </details>
            </Card>

            <p className="px-1 text-[0.7rem] leading-relaxed text-ink-3">
              The observer is read-only. Replacing or transferring a guardian seat is done through Manifold by the federation's operators.
            </p>
          </div>

          <div className="min-w-0 space-y-4 sm:space-y-5">
            <Card aria-labelledby="credential-title">
              <CardHeader id="credential-title" title="Trust credential" description="PeerBadge credential presented in the advertisement" />
              <TrustEvidence credential={profile.credential} />
            </Card>

          </div>
        </div>
      </div>
    </div>
  );
}
