import { Card } from '../components/ui/Card';
import { DataStamp } from '../components/ui/DataStamp';
import { Icon } from '../components/ui/Icon';
import { KeyChip } from '../components/ui/KeyChip';
import { PageHeader } from '../components/ui/PageHeader';
import { EmptyState, ErrorState, LoadingBlock } from '../components/ui/States';
import { StatusPill } from '../components/ui/Status';
import { formatUtcDateTime } from '../utils/time';
import { useFlipProviders } from '../features/manifold/hooks';
import { FreshnessPill, VerificationBadge } from '../features/manifold/components/TrustBits';
import type { FlipProvider, FlipSource } from '../features/manifold/types';
import { CollectorNote } from '../features/manifold/components/CollectorNote';
import { CursorPager } from '../components/ui/CursorPager';
import { usePageSize } from '../hooks/usePageSize';
import { errorMessage } from '../utils/errors';

const SOURCE_LABEL: Record<string, string> = {
  gateway: 'Lightning gateway liquidity',
  stability_pool: 'Stability pool liquidity',
};
const sourceLabel = (source: FlipSource) => SOURCE_LABEL[source] ?? source;

const REQUIREMENT_LABEL: Record<string, string> = {
  all_trusted: 'Every guardian must be trusted',
  majority_trusted: 'A consensus majority must be trusted',
};

/** Attester keys grouped by what they require, so each requirement is stated once */
function AttesterPolicies({ policies }: { policies: FlipProvider['accepted_attester_policies'] }) {
  if (policies.length === 0) return <p className="text-ink-3">None advertised</p>;
  const groups = new Map<string, string[]>();
  for (const policy of policies) {
    groups.set(policy.verification_requirement, [...(groups.get(policy.verification_requirement) ?? []), policy.attester_pubkey]);
  }
  return (
    <div className="space-y-3">
      {[...groups].map(([requirement, keys]) => (
        <div key={requirement}>
          <p className="mb-1.5 text-ink-2">
            {REQUIREMENT_LABEL[requirement] ?? requirement}
            <span className="text-ink-3"> · {keys.length} {keys.length === 1 ? 'attester' : 'attesters'}</span>
          </p>
          <ul className="flex flex-wrap gap-1.5">
            {keys.map((key) => <li key={key}><KeyChip value={key} label="attester key" /></li>)}
          </ul>
        </div>
      ))}
    </div>
  );
}

function ProviderCard({ provider }: { provider: FlipProvider }) {
  const withdrawn = provider.expires_at <= provider.issued_at;
  const name = provider.display?.name;
  return (
    <Card as="article" aria-labelledby={`flip-${provider.provider_pubkey}`}>
      <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          <h2 id={`flip-${provider.provider_pubkey}`} className="text-[0.95rem] font-semibold text-ink">
            {name ? <>{name} <span className="text-xs font-normal text-ink-3">(self-described)</span></> : <span className="text-ink-2">Unnamed provider</span>}
          </h2>
          <div className="mt-1.5"><KeyChip value={provider.provider_pubkey} label="provider public key" head={10} tail={8} /></div>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <VerificationBadge state={provider.credential.state} />
          {withdrawn ? <StatusPill tone="neutral" title="Republished with expiry equal to issue time">Withdrawn</StatusPill> : <FreshnessPill freshness={provider.freshness} issuedAt={provider.issued_at} />}
        </div>
      </div>

      <div className="mt-4 grid gap-4 text-xs md:grid-cols-[minmax(0,1fr)_minmax(0,2fr)]">
        <div>
          <h3 className="mb-1.5 text-[0.68rem] font-medium uppercase tracking-wide text-ink-3">Capabilities</h3>
          {provider.supported_sources.length === 0 && <p className="text-ink-3">None advertised</p>}
          <ul className="space-y-1">
            {provider.supported_sources.map((item) => (
              <li key={item} className="flex items-center gap-1.5 text-ink-2"><Icon name="check" className="size-3 text-accent" />{sourceLabel(item)}</li>
            ))}
          </ul>
          <p className="mt-2 text-ink-3">Networks: <span className="text-ink-2">{provider.supported_networks.join(', ') || '—'}</span></p>
        </div>
        <div>
          <h3 className="mb-1.5 text-[0.68rem] font-medium uppercase tracking-wide text-ink-3">Accepted attesters</h3>
          <AttesterPolicies policies={provider.accepted_attester_policies} />
        </div>
      </div>

      <details className="group mt-4 border-t border-line pt-3">
        <summary className="flex cursor-pointer list-none items-center gap-1 text-xs font-medium text-ink-2 hover:text-ink">
          <Icon name="chevronRight" className="size-3.5 transition-transform group-open:rotate-90" />
          Technical details
        </summary>
        <dl className="mt-3 grid grid-cols-1 gap-x-3 gap-y-1 text-xs sm:grid-cols-[7rem_minmax(0,1fr)] sm:gap-y-2 [&>dt]:mt-2 sm:[&>dt]:mt-0">
          <dt className="text-ink-3">Endpoints</dt>
          <dd className="min-w-0">{provider.api_endpoints.length ? provider.api_endpoints.map((url) => <div key={url} className="break-all font-mono text-[0.7rem] text-ink-2">{url}</div>) : <span className="text-ink-3">None</span>}</dd>
          <dt className="text-ink-3">API versions</dt>
          <dd className="text-ink-2">{provider.api_versions.join(', ') || '—'}</dd>
          <dt className="text-ink-3">Relay hints</dt>
          <dd className="min-w-0">{provider.relay_hints.length ? provider.relay_hints.map((relay) => <div key={relay} className="break-all font-mono text-[0.7rem] text-ink-2">{relay}</div>) : <span className="text-ink-3">None</span>}</dd>
          <dt className="text-ink-3">Advertisement</dt>
          <dd className="min-w-0 break-words text-ink-2">Kind 37702 · signature {provider.signature_valid ? 'valid' : 'invalid'} · {formatUtcDateTime(provider.issued_at * 1000)} → {formatUtcDateTime(provider.expires_at * 1000)}</dd>
          {(provider.display?.website || provider.display?.contact) && <>
            <dt className="text-ink-3">Self-described</dt>
            <dd className="text-ink-2">{[provider.display?.website, provider.display?.contact].filter(Boolean).join(' · ')}</dd>
          </>}
        </dl>
      </details>
    </Card>
  );
}

export function FlipProviders() {
  const [pageSize, setPageSize] = usePageSize('flip-providers', 10);
  const pages = useFlipProviders(pageSize);
  const head = pages.latest ?? pages.page;
  const providers = pages.page?.data ?? [];

  return (
    <div>
      <PageHeader
        eyebrow={<span className="text-manifold-ink">Manifold</span>}
        title="FLIP liquidity providers"
        badges={<>
          <StatusPill tone="manifold" icon="flask" size="md">Preview</StatusPill>
        </>}
        description="Liquidity providers that announce a FLIP service on Nostr (kind 37702)."
        actions={head && <DataStamp kind="live" at={head.observed_at} staleAfterSeconds={900} />}
      />
      <div className="space-y-4 sm:space-y-5">
        {head && (
          <CollectorNote relays={head.relays}>
            FLIP advertisements never state capacity or balances, so nothing here is available liquidity, and no provider is confirmed to serve any federation yet.
          </CollectorNote>
        )}
        {pages.error && !pages.page ? (
          <ErrorState title="Could not load FLIP providers" message={errorMessage(pages.error.cause)} onRetry={pages.restart} />
        ) : !pages.page ? (
          <LoadingBlock label="Loading FLIP providers" rows={4} />
        ) : providers.length === 0 ? (
          <EmptyState icon="liquidity" title={pages.page.observed_at ? 'No FLIP provider advertisements found' : 'Not synced yet'}>
            {pages.page.observed_at ? 'None of the relays carried a valid FLIP advertisement in the last 7 days.' : 'The observer collects advertisements every 5 minutes; the first sync has not completed.'}
          </EmptyState>
        ) : (
          providers.map((provider) => <ProviderCard key={provider.provider_pubkey} provider={provider} />)
        )}
        {pages.page && (
          <CursorPager
            label="FLIP provider pages"
            page={pages.pageNumber}
            hasNext={pages.hasNext}
            loading={pages.loadingPage}
            onPrevious={pages.previous}
            onNext={pages.next}
            pageSize={pageSize}
            onPageSizeChange={setPageSize}
            total={pages.page.page?.total}
          />
        )}
      </div>
    </div>
  );
}
