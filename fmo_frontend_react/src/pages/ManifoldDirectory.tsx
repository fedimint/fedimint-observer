import { useMemo, useState, type ReactNode } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { Card } from '../components/ui/Card';
import { DataStamp } from '../components/ui/DataStamp';
import { Icon } from '../components/ui/Icon';
import { KeyChip } from '../components/ui/KeyChip';
import { PageHeader } from '../components/ui/PageHeader';
import { EmptyState, ErrorState, LoadingBlock } from '../components/ui/States';
import { InfoTip } from '../components/ui/Tooltip';
import { useManifoldGuardians } from '../features/manifold/hooks';
import {
  DEFAULT_QUERY,
  cheapestPlanMsats,
  formatPriceMsats,
  formatSizes,
  type DirectoryQuery,
  type DirectorySort,
  type VerificationFilter,
} from '../features/manifold/lib/directory';
import { CollectorNote } from '../features/manifold/components/CollectorNote';
import { CursorPager } from '../components/ui/CursorPager';
import { usePageSize } from '../hooks/usePageSize';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { errorMessage } from '../utils/errors';
import { FreshnessPill, VerificationBadge } from '../features/manifold/components/TrustBits';
import type { ManifoldGuardianSummary } from '../features/manifold/types';


const selectClass = 'h-9 rounded-lg border border-line bg-surface-2 py-0 pr-8 pl-2.5 text-xs text-ink focus:border-manifold focus:ring-0';

function Select({ label, value, onChange, children }: { label: string; value: string; onChange: (value: string) => void; children: ReactNode }) {
  return (
    <label className="flex min-w-0 flex-col gap-1">
      <span className="text-[0.68rem] font-medium uppercase tracking-wide text-ink-3">{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)} className={selectClass}>{children}</select>
    </label>
  );
}

function Price({ guardian }: { guardian: ManifoldGuardianSummary }) {
  const price = cheapestPlanMsats(guardian.advertisement?.plans ?? []);
  return price === null ? <span className="text-ink-3">—</span> : <span className="tabular">{formatPriceMsats(price)}</span>;
}

function Seats({ guardian }: { guardian: ManifoldGuardianSummary }) {
  if (guardian.seat_count === 0) return <span className="text-ink-3">None verified</span>;
  return (
    <span className="tabular" title="From verified fedi:fman_seat_bindings directories">
      {guardian.seat_count} {guardian.seat_count === 1 ? 'seat' : 'seats'} · {guardian.federation_count} {guardian.federation_count === 1 ? 'federation' : 'federations'}
    </span>
  );
}

function DirectoryTable({ guardians }: { guardians: readonly ManifoldGuardianSummary[] }) {
  const navigate = useNavigate();
  const profile = (guardian: ManifoldGuardianSummary) => `/manifold/guardians/${guardian.fman_pubkey}`;
  return (
    <>
      <div className="hidden overflow-x-auto md:block scrollbar-thin">
        <table className="w-full min-w-[920px] text-sm">
          <caption className="sr-only">Manifold Guardians announced on Nostr. Select a row for its profile.</caption>
          <thead className="border-b border-line text-left text-[0.7rem] uppercase tracking-wide text-ink-3">
            <tr>
              <th scope="col" className="px-3 py-2.5 font-medium">Manifold Guardian</th>
              <th scope="col" className="px-3 py-2.5 font-medium">Trust credential</th>
              <th scope="col" className="px-3 py-2.5 font-medium">Advertisement</th>
              <th scope="col" className="px-3 py-2.5 font-medium">Fedimint</th>
              <th scope="col" className="px-3 py-2.5 font-medium">Sizes</th>
              <th scope="col" className="px-3 py-2.5 text-right font-medium">Setup from</th>
              <th scope="col" className="px-3 py-2.5 font-medium">Operates</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line">
            {guardians.map((guardian) => {
              const ad = guardian.advertisement;
              return (
                <tr key={guardian.fman_pubkey} onClick={() => navigate(profile(guardian))} className="cursor-pointer hover:bg-surface-2">
                  <td className="px-3 py-2.5">
                    <Link to={profile(guardian)} onClick={(event) => event.stopPropagation()} className="block font-mono text-sm font-medium text-manifold-ink hover:underline">
                      {guardian.display_name}
                    </Link>
                    <span onClick={(event) => event.stopPropagation()} className="mt-1 inline-block">
                      <KeyChip value={guardian.fman_pubkey} label={`public key of ${guardian.display_name}`} />
                    </span>
                  </td>
                  <td className="px-3 py-2.5"><VerificationBadge state={guardian.credential.state} /></td>
                  <td className="px-3 py-2.5"><FreshnessPill freshness={guardian.freshness} issuedAt={ad?.issued_at} /></td>
                  <td className="px-3 py-2.5 font-mono text-xs text-ink-2">{ad?.availability.fedimintd_version ?? '—'}</td>
                  <td className="px-3 py-2.5 text-xs text-ink-2 tabular">{ad ? formatSizes(ad.availability.federation_sizes) : '—'}</td>
                  <td className="px-3 py-2.5 text-right text-xs text-ink"><Price guardian={guardian} /></td>
                  <td className="px-3 py-2.5 text-xs text-ink-2"><Seats guardian={guardian} /></td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <ul className="space-y-2 md:hidden">
        {guardians.map((guardian) => {
          const ad = guardian.advertisement;
          return (
            <li key={guardian.fman_pubkey} className="rounded-lg border border-line bg-surface-2 p-3">
              <div className="flex items-start justify-between gap-2">
                <Link to={profile(guardian)} className="min-w-0 truncate font-mono text-sm font-medium text-manifold-ink hover:underline">
                  {guardian.display_name}
                </Link>
                <VerificationBadge state={guardian.credential.state} />
              </div>
              <div className="mt-1.5"><KeyChip value={guardian.fman_pubkey} label={`public key of ${guardian.display_name}`} /></div>
              <dl className="mt-3 grid grid-cols-2 gap-2 text-xs">
                <div className="col-span-2"><dt className="sr-only">Advertisement</dt><dd><FreshnessPill freshness={guardian.freshness} issuedAt={ad?.issued_at} /></dd></div>
                <div><dt className="text-ink-3">Fedimint</dt><dd className="mt-0.5 font-mono text-ink">{ad?.availability.fedimintd_version ?? '—'}</dd></div>
                <div><dt className="text-ink-3">Setup from</dt><dd className="mt-0.5 text-ink"><Price guardian={guardian} /></dd></div>
                <div><dt className="text-ink-3">Sizes</dt><dd className="mt-0.5 text-ink tabular">{ad ? formatSizes(ad.availability.federation_sizes) : '—'}</dd></div>
                <div><dt className="text-ink-3">Operates</dt><dd className="mt-0.5 text-ink"><Seats guardian={guardian} /></dd></div>
              </dl>
              <Link to={profile(guardian)} className="mt-3 inline-flex items-center gap-1 text-xs font-medium text-manifold-ink">
                View profile <Icon name="chevronRight" className="size-3.5" />
              </Link>
            </li>
          );
        })}
      </ul>
    </>
  );
}

export function ManifoldDirectory() {
  const [query, setQuery] = useState<DirectoryQuery>(DEFAULT_QUERY);
  const [pageSize, setPageSize] = usePageSize('manifold-guardians');
  // The backend searches; it is asked once typing pauses, not on every key
  const search = useDebouncedValue(query.search);
  const { verification, version, size, freshOnly, sort } = query;
  const sent = useMemo<DirectoryQuery>(
    () => ({ search, verification, version, size, freshOnly, sort }),
    [search, verification, version, size, freshOnly, sort],
  );
  const pages = useManifoldGuardians(sent, pageSize);
  // Page 1 carries the provenance and facets; later pages keep showing them
  const head = pages.latest ?? pages.page;
  const rows = pages.page?.data ?? [];
  const facets = head?.facets ?? { guardians: 0, versions: [], sizes: [] };
  const matched = pages.page?.page?.total ?? 0;
  const update = (patch: Partial<DirectoryQuery>) => setQuery((current) => ({ ...current, ...patch }));
  const filtered = JSON.stringify({ ...query, sort: DEFAULT_QUERY.sort }) !== JSON.stringify({ ...DEFAULT_QUERY });

  return (
    <div>
      <PageHeader
        eyebrow={<span className="text-manifold-ink">Manifold</span>}
        title="Guardian directory"
        description="Manifold Guardians that announce themselves on Nostr (kinds 37701 and 37709) as available to operate federation seats."
        actions={head && <DataStamp kind="live" at={head.observed_at} staleAfterSeconds={900} />}
      />

      <div className="space-y-4 sm:space-y-5">
        {head && <CollectorNote relays={head.relays} />}


        <Card padding="p-0">
          <div className="grid gap-3 border-b border-line p-4 sm:grid-cols-2 lg:grid-cols-[minmax(0,1.6fr)_repeat(4,minmax(0,1fr))] sm:p-5">
            <label className="flex min-w-0 flex-col gap-1 sm:col-span-2 lg:col-span-1">
              <span className="text-[0.68rem] font-medium uppercase tracking-wide text-ink-3">Search</span>
              <span className="relative">
                <Icon name="filter" className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-ink-3" />
                <input
                  type="search"
                  value={query.search}
                  onChange={(event) => update({ search: event.target.value })}
                  placeholder="Name, guardian key, service or holder key"
                  className="h-9 w-full rounded-lg border border-line bg-surface-2 pr-2 pl-8 text-xs text-ink placeholder:text-ink-3 focus:border-manifold focus:ring-0"
                />
              </span>
            </label>
            <Select label="Verification" value={query.verification} onChange={(value) => update({ verification: value as VerificationFilter })}>
              <option value="all">Any status</option>
              <option value="verified">Verified</option>
              <option value="unavailable">Unverified (checks incomplete)</option>
              <option value="failed">Verification failed</option>
              <option value="none">No credential</option>
            </Select>
            <Select label="Fedimint version" value={query.version} onChange={(value) => update({ version: value })}>
              <option value="all">Any version</option>
              {facets.versions.map((version) => <option key={version} value={version}>{version}.x</option>)}
            </Select>
            <Select label="Federation size" value={String(query.size)} onChange={(value) => update({ size: value === 'all' ? 'all' : Number(value) })}>
              <option value="all">Any size</option>
              {facets.sizes.map((size) => <option key={size} value={size}>{size} guardians</option>)}
            </Select>
            <Select label="Sort by" value={query.sort} onChange={(value) => update({ sort: value as DirectorySort })}>
              <option value="freshness">Freshest advertisement</option>
              <option value="price_asc">Setup price, low to high</option>
              <option value="price_desc">Setup price, high to low</option>
              <option value="name">Name</option>
            </Select>
          </div>
          <div className="flex flex-wrap items-center justify-between gap-2 px-4 pt-3 sm:px-5">
            <label className="inline-flex items-center gap-2 text-xs text-ink-2">
              <input
                type="checkbox"
                checked={query.freshOnly}
                onChange={(event) => update({ freshOnly: event.target.checked })}
                className="size-3.5 rounded border-line-strong text-manifold focus:ring-manifold"
              />
              Fresh advertisements only
              <InfoTip label="About freshness">
                Manifold clients use an advertisement only while it is unexpired, issued no more than 2 hours ago and not dated in the future. A guardian that stops taking seats simply lets its advertisement expire.
              </InfoTip>
            </label>
            <div className="flex items-center gap-3 text-xs text-ink-3">
              <span role="status">{head ? `${matched} of ${facets.guardians} guardians` : ''}</span>
              {filtered && (
                <button type="button" onClick={() => setQuery({ ...DEFAULT_QUERY, sort: query.sort })} className="font-medium text-manifold-ink hover:underline">
                  Clear filters
                </button>
              )}
            </div>
          </div>
          <div className="p-2 sm:p-3">
            {pages.error && !pages.page ? (
              <div className="p-2"><ErrorState title="Could not load the directory" message={errorMessage(pages.error.cause)} onRetry={pages.restart} /></div>
            ) : !pages.page ? (
              <LoadingBlock label="Loading Manifold Guardians" rows={6} className="p-2" />
            ) : facets.guardians === 0 ? (
              <div className="p-2">
                <EmptyState icon="directory" title={pages.page.observed_at ? 'No Manifold Guardian advertisements found' : 'Not synced yet'}>
                  {pages.page.observed_at
                    ? 'None of the relays carried a valid advertisement in the last 7 days.'
                    : 'The observer collects advertisements every 5 minutes; the first sync has not completed.'}
                </EmptyState>
              </div>
            ) : rows.length === 0 ? (
              <div className="p-2"><EmptyState icon="filter" title="No guardian matches these filters" /></div>
            ) : (
              <DirectoryTable guardians={rows} />
            )}
          </div>
          <div className="border-t border-line px-4 py-3 sm:px-5">
            <CursorPager
              label="Guardian directory pages"
              page={pages.pageNumber}
              hasNext={pages.hasNext}
              loading={pages.loadingPage}
              onPrevious={pages.previous}
              onNext={pages.next}
              pageSize={pageSize}
              onPageSizeChange={setPageSize}
              total={pages.page?.page?.total}
            />
            {pages.error && pages.page && (
              <p role="alert" className="mt-2 text-xs text-warn-ink">Could not load that page: {errorMessage(pages.error.cause)}</p>
            )}
          </div>
        </Card>

        {head && (
          <p className="text-xs text-ink-3">
            Advertised prices and versions are what each guardian claims about its offer. Self-reported uptime, where shown on a profile, is asserted by the guardian itself and is not an observation.
          </p>
        )}
      </div>
    </div>
  );
}
