import type { ReactNode } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Card, CardHeader } from '../components/ui/Card';
import { DataStamp } from '../components/ui/DataStamp';
import { Icon } from '../components/ui/Icon';
import { KeyChip } from '../components/ui/KeyChip';
import { Kpi } from '../components/ui/Kpi';
import { PageHeader } from '../components/ui/PageHeader';
import { EmptyState, ErrorState, LoadingBlock, Skeleton } from '../components/ui/States';
import { StatusPill } from '../components/ui/Status';
import { InfoTip } from '../components/ui/Tooltip';
import { formatUtcDate, formatUtcDateTime } from '../utils/time';
import { useManifoldGuardian } from '../features/manifold/hooks';
import { concentration } from '../features/manifold/lib/concentration';
import { cheapestPlanMsats, formatPriceMsats, formatSizes } from '../features/manifold/lib/directory';
import { FreshnessPill, PlanList, TrustEvidence, VerificationBadge } from '../features/manifold/components/TrustBits';
import type { AdObservationDay, ManifoldGuardianProfile, MgPlan, OperatedSeat } from '../features/manifold/types';

function groupSeats(seats: readonly OperatedSeat[]) {
  const groups = new Map<string, OperatedSeat[]>();
  for (const seat of seats) groups.set(seat.federation_id, [...(groups.get(seat.federation_id) ?? []), seat]);
  return [...groups.values()];
}

/** Presence over the days the observer polled; days before the first poll carry no evidence */
function presenceSummary(history: readonly AdObservationDay[]) {
  const polled = history.filter((day) => day.polls > 0);
  if (polled.length === 0) return null;
  const polls = polled.reduce((sum, day) => sum + day.polls, 0);
  const fresh = polled.reduce((sum, day) => sum + day.fresh_polls, 0);
  return { share: fresh / polls, polls, days: polled.length, firstPolled: polled[0].date };
}

function planLabel(plan: MgPlan) {
  return plan.kind === 'infinite_best_effort' ? 'Best effort, no expiry' : `Subscription (${plan.period})`;
}

function plural(count: number, one: string, many = `${one}s`) {
  return `${count.toLocaleString('en-US')} ${count === 1 ? one : many}`;
}

function Summary({ profile }: { profile: ManifoldGuardianProfile }) {
  const ad = profile.advertisement;
  const cheapest = ad ? cheapestPlanMsats(ad.plans) : null;
  const presence = presenceSummary(profile.observation_history);
  return (
    <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
      <Kpi
        label="Seats"
        value={profile.seat_count.toLocaleString('en-US')}
        detail={profile.seat_count === 0 ? 'No verified bindings' : `in ${plural(profile.federation_count, 'federation')}`}
        help="From federations' verified seat-binding directories; an advertisement alone never counts."
      />
      <Kpi
        label="Setup price"
        value={cheapest === null ? '—' : formatPriceMsats(cheapest)}
        detail={!ad ? 'No current advertisement' : ad.plans.length === 1 ? planLabel(ad.plans[0]) : ad.plans.length === 0 ? 'No plans advertised' : `Cheapest of ${ad.plans.length} plans`}
        help="Advertised by the guardian; not verified."
      />
      <Kpi
        label="Presence"
        value={presence ? `${(presence.share * 100).toFixed(1)}%` : '—'}
        detail={presence ? `${plural(presence.polls, 'poll')} · ${plural(presence.days, 'day')}` : 'Not polled yet'}
        help="Share of the observer's own relay polls that found a fresh advertisement."
      />
      <Kpi
        label="Fedimint"
        value={<span className="font-mono text-lg">{ad?.availability.fedimintd_version ?? '—'}</span>}
        detail={ad?.software ? `MG ${ad.software.mg_version} · API ${ad.software.api_version}` : ad ? 'MG version not reported' : 'No current advertisement'}
      />
    </div>
  );
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
  const days = profile.observation_history;
  const presence = presenceSummary(days);
  if (!presence) return <EmptyState icon="clock" title="Not polled yet">The observer has not polled the relays for this guardian yet.</EmptyState>;
  const share = (day: AdObservationDay) => (day.polls > 0 ? day.fresh_polls / day.polls : null);
  return (
    <div className="flex flex-1 flex-col">
      <div className="flex min-h-10 flex-1 items-end gap-[2px] lg:max-h-20" role="img" aria-label={`Daily share of polls that found a fresh advertisement, ${days.length} days`}>
        {days.map((day) => {
          const value = share(day);
          return (
            <div key={day.date} className="flex h-full min-w-[3px] flex-1 items-end" title={`${formatUtcDate(day.date)}: ${value === null ? 'not polled' : `${day.fresh_polls} of ${day.polls} polls found a fresh advertisement`}`}>
              {value === null
                ? <div className="cell-empty h-full w-full rounded-[2px]" />
                : <div className={`w-full rounded-t-[2px] ${value >= 0.95 ? 'bg-manifold' : 'bg-warn'}`} style={{ height: `${Math.max(8, value * 100)}%` }} />}
            </div>
          );
        })}
      </div>
      <div className="mt-1.5 flex justify-between text-xs text-ink-3">
        <span>{formatUtcDate(days[0].date, { month: 'short', day: 'numeric' })}</span>
        <span>Today</span>
      </div>
      <p className="mt-3 text-xs text-ink-3">
        Polled since {formatUtcDate(presence.firstPolled, { month: 'short', day: 'numeric' })}; hatched days were not polled. This shows the guardian kept offering seats, not how its federations' guardians performed.
      </p>
    </div>
  );
}

function VersionHistory({ profile }: { profile: ManifoldGuardianProfile }) {
  const history = profile.version_history;
  if (history.length === 0) return <p className="text-ink-3">No versions observed yet</p>;
  if (history.length === 1) {
    return <p className="text-ink-2">Unchanged since first seen {formatUtcDateTime(history[0].observed_at)}</p>;
  }
  return (
    <ol className="space-y-2">
      {history.map((change, index) => (
        <li key={change.observed_at} className="flex items-start gap-2">
          <span className={`mt-1.5 size-1.5 shrink-0 rounded-full ${index === 0 ? 'bg-manifold' : 'bg-line-strong'}`} aria-hidden="true" />
          <div className="min-w-0">
            <span className="font-mono text-ink">{change.fedimintd_version}</span>
            {change.mg_version && <span className="font-mono text-ink-3"> · MG {change.mg_version}</span>}
            <span className="block text-ink-3">{formatUtcDateTime(change.observed_at)}{index === 0 && ' · current'}</span>
          </div>
        </li>
      ))}
    </ol>
  );
}

function OfferRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-ink-3">{label}</dt>
      <dd className="min-w-0 text-ink">{children}</dd>
    </>
  );
}

function AdvertisedOffer({ profile }: { profile: ManifoldGuardianProfile }) {
  const ad = profile.advertisement;
  const uptime = ad?.self_reported_uptime_permille;
  const uptimeKeys = (['1d', '7d', '30d'] as const).filter((key) => uptime?.[key] !== undefined);
  return (
    <dl className="grid grid-cols-[7.5rem_minmax(0,1fr)] gap-x-3 gap-y-2.5 text-xs">
      {ad ? <>
        <OfferRow label="Federation sizes"><span className="tabular">{formatSizes(ad.availability.federation_sizes)} guardians</span></OfferRow>
        <OfferRow label="Setup plans">
          {ad.plans.length <= 1
            ? ad.plans.length === 0
              ? <span className="text-ink-3">No plans advertised</span>
              : <span>{planLabel(ad.plans[0])} · <span className="font-medium tabular">{formatPriceMsats(cheapestPlanMsats(ad.plans)!)}</span></span>
            : <PlanList plans={ad.plans} />}
        </OfferRow>
        <OfferRow label="Self-reported uptime">
          {uptime && uptimeKeys.length > 0 ? (
            <span className="tabular">
              {uptimeKeys.map((key) => `${key} ${(uptime[key]! / 10).toFixed(1)}%`).join(' · ')}
              <span className="block text-ink-3">Asserted by the guardian; not measured by the observer</span>
            </span>
          ) : <span className="text-ink-3">Not reported</span>}
        </OfferRow>
      </> : (
        <OfferRow label="Advertisement"><span className="text-ink-3">No current advertisement</span></OfferRow>
      )}
      <OfferRow label="Version history"><VersionHistory profile={profile} /></OfferRow>
    </dl>
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
        <Summary profile={profile} />

        <div className="grid gap-4 sm:gap-5 lg:grid-cols-[minmax(0,1fr)_20rem] xl:grid-cols-[minmax(0,1fr)_24rem]">
          <div className="contents lg:flex lg:min-w-0 lg:flex-col lg:gap-5">
            <Card aria-labelledby="operated-title" className="order-1 min-w-0">
              <CardHeader
                id="operated-title"
                title="Federations operated"
                description="Verified seat bindings, and how much of each federation this guardian controls"
              />
              <FederationsOperated profile={profile} />
            </Card>

            <Card aria-labelledby="offer-title" className="order-3 min-w-0">
              <CardHeader id="offer-title" title="Advertised offer" description="What the guardian says it offers; not verified" />
              <AdvertisedOffer profile={profile} />
            </Card>

            <Card aria-labelledby="presence-title" className="order-4 flex min-w-0 flex-col lg:flex-1">
              <CardHeader
                id="presence-title"
                title="Advertisement presence"
                description="Share of the observer's relay polls each day that found a fresh advertisement"
                meta={<DataStamp kind="historical" at={resource.data!.observed_at} scope="30 days" />}
              />
              <PresenceHistory profile={profile} />
            </Card>
          </div>

          <div className="contents lg:flex lg:min-w-0 lg:flex-col">
            <Card aria-labelledby="credential-title" className="order-2 min-w-0 lg:flex-1">
              <CardHeader id="credential-title" title="Trust credential" description="PeerBadge credential presented in the advertisement" />
              <TrustEvidence credential={profile.credential} />
            </Card>
          </div>

          <Card aria-labelledby="technical-title" className="order-5 min-w-0 lg:col-span-full">
            <details className="group">
              <summary className="flex cursor-pointer list-none items-center justify-between gap-2">
                <h2 id="technical-title" className="text-base font-semibold text-ink">Technical details</h2>
                <Icon name="chevronRight" className="size-4 text-ink-3 transition-transform group-open:rotate-90" />
              </summary>
              <dl className="mt-3 grid grid-cols-[7rem_minmax(0,1fr)] gap-x-3 gap-y-3 text-xs">
                <TechnicalRow label="Nostr identity"><KeyChip value={profile.fman_pubkey} label="Nostr public key" full /></TechnicalRow>
                {ad && <>
                  <TechnicalRow label="Service key"><KeyChip value={ad.service_pubkey} label="service public key" /></TechnicalRow>
                  <TechnicalRow label="Iroh endpoints">
                    <ul className="space-y-1">{ad.api_endpoints.map((endpoint) => <li key={endpoint.url} className="break-all font-mono text-xs">{endpoint.url}</li>)}</ul>
                  </TechnicalRow>
                  <TechnicalRow label="Advertisement">
                    Kind {ad.event_kind} {ad.event_kind === 37709 ? '(v2)' : '(legacy)'} · signature {ad.signature_valid ? 'valid' : 'invalid'}
                    <div className="mt-1"><KeyChip value={ad.event_id} label="advertisement event id" /></div>
                  </TechnicalRow>
                  <TechnicalRow label="Validity">{formatUtcDateTime(ad.issued_at * 1000)} → {formatUtcDateTime(ad.expires_at * 1000)}</TechnicalRow>
                  <TechnicalRow label="Authorizations">{plural(ad.holder_authorization_count, 'holder authorization')}</TechnicalRow>
                </>}
                <TechnicalRow label="Relays">
                  <ul className="space-y-0.5">{profile.nostr_relays.map((relay) => <li key={relay} className="break-all font-mono text-xs">{relay}</li>)}</ul>
                </TechnicalRow>
              </dl>
            </details>
          </Card>

          <p className="order-6 px-1 text-xs leading-relaxed text-ink-3 lg:col-span-full">
            The observer is read-only. Replacing or transferring a guardian seat is done through Manifold by the federation's operators.
          </p>
        </div>
      </div>
    </div>
  );
}
