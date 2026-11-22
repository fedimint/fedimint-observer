import { Link } from 'react-router-dom';
import { ListCard } from '../components/ui/ListCard';
import { SearchInput } from '../components/ui/SearchInput';
import { CursorPager } from '../components/ui/CursorPager';
import { Totals } from '../features/federations/components/Totals';
import { FederationSortControl } from '../features/federations/components/FederationSortControl';
import { FederationList, FederationListSkeleton } from '../features/federations/components/FederationList';
import { CollapsibleFederations } from '../features/federations/components/CollapsibleFederations';
import { useFederationGroup } from '../features/federations/hooks/useFederationGroup';
import { useFederationSort } from '../features/federations/hooks/useFederationSort';
import { useFederationSearch } from '../features/federations/hooks/useFederationSearch';
import { useDebouncedValue } from '../hooks/useDebouncedValue';
import { usePageSize } from '../hooks/usePageSize';
import type { PageSize } from '../utils/pageSize';

type GroupPages = ReturnType<typeof useFederationGroup>;

function GroupPager({ group, label, pageSize, onPageSizeChange }: { group: GroupPages; label: string; pageSize: PageSize; onPageSizeChange: (size: PageSize) => void }) {
  if (group.page === null) return null;
  return (
    <CursorPager
      label={label}
      page={group.pageNumber}
      hasNext={group.hasNext}
      loading={group.loadingPage}
      onPrevious={group.previous}
      onNext={group.next}
      pageSize={pageSize}
      onPageSizeChange={onPageSizeChange}
      total={group.total ?? undefined}
    />
  );
}

export function Home() {
  const { sortKey, sortDirection, setSort } = useFederationSort();
  const {
    searchQuery,
    setSearchQuery,
    isInvite,
    resolveInvite,
    resolvedFederationId,
    isResolvingInvite,
    lookupError,
  } = useFederationSearch();
  const [pageSize, setPageSize] = usePageSize('federations', 25);
  // An invite resolved to its federation is searched by that federation's ID
  const wanted = resolvedFederationId ?? searchQuery;
  // The backend searches; it is asked once typing pauses, not on every key
  const search = useDebouncedValue(wanted);
  // Each group is its own paged list; the backend groups, searches and orders
  const active = useFederationGroup('active', search, sortKey, sortDirection, pageSize);
  const offline = useFederationGroup('offline', search, sortKey, sortDirection, pageSize);
  const shutDown = useFederationGroup('shut_down', search, sortKey, sortDirection, pageSize);
  const pagerProps = { pageSize, onPageSizeChange: setPageSize };

  // Every group loaded for the current search, and none has a match
  const groups = [active, offline, shutDown];
  const noMatches =
    search === wanted && groups.every((g) => g.page !== null && g.error === null && g.total === 0);
  // Only a loaded, empty result proves a federation is unobserved
  const unobservedFederationId = resolvedFederationId && noMatches ? resolvedFederationId : null;

  return (
    <div className="pb-4">
      <div className="my-8 sm:my-16">
        <Totals />
      </div>

      <div className="mb-6">
        <SearchInput
          value={searchQuery}
          onChange={setSearchQuery}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && isInvite && !resolvedFederationId && !isResolvingInvite) {
              resolveInvite();
            }
          }}
          label="Search federations by name, invite code or ID"
          clearLabel="Clear federation search"
          placeholder="Search federations by name, invite code or ID..."
        />
        {isResolvingInvite && (
          <div role="status" className="flex items-center gap-2 mt-2 text-xs text-blue-600 dark:text-blue-400">
            <svg className="w-3.5 h-3.5 animate-spin" aria-hidden="true" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"></circle>
              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
            </svg>
            <span>Looking up federation on Observer API…</span>
          </div>
        )}
        {lookupError && (
          <p role="alert" className="mt-2 text-xs text-red-600 dark:text-red-400">
            {lookupError}
          </p>
        )}
        {isInvite && !resolvedFederationId && !isResolvingInvite && noMatches && (
          <div className="flex items-center justify-between gap-3 p-3 mt-3 text-sm rounded-lg bg-gray-50 border border-gray-200 dark:bg-gray-800 dark:border-gray-700">
            <span className="text-gray-700 dark:text-gray-300">
              Non-matching invite code. Look up federation ID on Observer API?
            </span>
            <button
              type="button"
              onClick={resolveInvite}
              className="shrink-0 px-3 py-1.5 text-xs font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 focus:ring-2 focus:ring-blue-300 dark:focus:ring-blue-800 cursor-pointer"
            >
              Look up on Observer API
            </button>
          </div>
        )}
      </div>

      {unobservedFederationId && (
        <div
          role="status"
          className="mb-6 p-4 text-sm text-blue-800 rounded-lg bg-blue-50 dark:bg-gray-800 dark:text-blue-300 border border-blue-200 dark:border-blue-700"
        >
          <span className="font-semibold">Valid federation invite: </span>
          Federation <code className="font-mono">{unobservedFederationId.slice(0, 8)}…{unobservedFederationId.slice(-8)}</code> is not currently observed by this instance.
          You can{' '}
          <Link
            to="/nostr"
            className="underline font-medium hover:text-blue-900 dark:hover:text-blue-200"
          >
            inspect active federations on the Nostr tab
          </Link>
          .
        </div>
      )}

      <ListCard>
        <div className="p-4 sm:p-5 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between bg-white dark:bg-gray-800">
          <div className="text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 dark:text-white">
            <h1>Observed Federations</h1>
            <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
              List of all active federations this instance is collecting statistics on
            </p>
            {active.error && active.page && (
              <p role="status" className="mt-1 text-xs font-normal text-amber-700 dark:text-amber-400">
                {active.error.kind === 'refresh'
                  ? 'Could not refresh the list; showing the last data that loaded. Retrying automatically.'
                  : 'Could not load that page. Try again.'}
              </p>
            )}
          </div>
          <FederationSortControl sortKey={sortKey} sortDirection={sortDirection} onChange={setSort} />
        </div>
        <FederationList
          federations={active.federations}
          failed={active.failed}
          scale={active.scale}
          loading={active.loading ? <FederationListSkeleton /> : null}
          emptyText={search ? 'No federations match your search' : 'No active federations found'}
          pager={<GroupPager group={active} label="Active federation pages" {...pagerProps} />}
        />
      </ListCard>

      <CollapsibleFederations
        id="offline-federations"
        title="Offline Federations"
        description="Federations where fewer guardians answered the observer in the last 5 minutes than consensus needs"
        emptyText={search ? 'No offline federations match your search' : 'No offline federations'}
        federations={offline.federations}
        total={offline.total}
        loading={offline.loading}
        failed={offline.failed}
        scale={active.page ? active.scale : offline.scale}
        pager={<GroupPager group={offline} label="Offline federation pages" {...pagerProps} />}
      />

      <CollapsibleFederations
        id="shut-down-federations"
        title="Shut Down Federations"
        description="Federations whose guardians announced an expiry date that has passed"
        emptyText={search ? 'No shut down federations match your search' : 'No shut down federations'}
        federations={shutDown.federations}
        total={shutDown.total}
        loading={shutDown.loading}
        failed={shutDown.failed}
        scale={active.page ? active.scale : shutDown.scale}
        pager={<GroupPager group={shutDown} label="Shut down federation pages" {...pagerProps} />}
      />
    </div>
  );
}
