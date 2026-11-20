import { useMemo } from 'react';
import { Link } from 'react-router-dom';
import { ListCard } from '../components/ui/ListCard';
import { SearchInput } from '../components/ui/SearchInput';
import { formatUtcTime } from '../utils/time';
import { federationNames } from '../utils/migration';
import { Totals } from '../features/federations/components/Totals';
import { FederationSortControl } from '../features/federations/components/FederationSortControl';
import { FederationList, FederationListSkeleton } from '../features/federations/components/FederationList';
import { CollapsibleFederations } from '../features/federations/components/CollapsibleFederations';
import { useFederationList } from '../features/federations/hooks/useFederationList';
import { useFederationSort } from '../features/federations/hooks/useFederationSort';
import { useFederationSearch } from '../features/federations/hooks/useFederationSearch';
import { activityScale, groupFederations } from '../features/federations/lib/federationList';

export function Home() {
  const { federations, loading, failed, updatedAt, refreshFailed } = useFederationList();
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

  const groups = useMemo(
    () => groupFederations(federations, searchQuery, sortKey, sortDirection, updatedAt, resolvedFederationId),
    [federations, searchQuery, sortKey, sortDirection, updatedAt, resolvedFederationId],
  );
  // Names of every federation, so a migrating one can name a successor the observer tracks
  const successorNames = useMemo(() => federationNames(federations), [federations]);
  // Global maxes across all federations for a consistent chart scale
  const scale = useMemo(() => activityScale(federations), [federations]);
  const listProps = { loading, failed, scale, successorNames };

  const hasLocalMatches = groups.active.length > 0 || groups.offline.length > 0 || groups.shutDown.length > 0;

  // Only a loaded list proves a federation is unobserved
  const unobservedFederationId = useMemo(() => {
    if (!resolvedFederationId || loading || failed) return null;
    const exists = federations.some(
      (f) => f.id.toLowerCase() === resolvedFederationId.toLowerCase(),
    );
    return exists ? null : resolvedFederationId;
  }, [resolvedFederationId, loading, failed, federations]);

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
          label="Search federations by name or invite code"
          clearLabel="Clear federation search"
          placeholder="Search federations by name or invite code..."
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
        {isInvite && !resolvedFederationId && !isResolvingInvite && !hasLocalMatches && (
          <div className="flex items-center justify-between p-3 mt-3 text-sm rounded-lg bg-gray-50 border border-gray-200 dark:bg-gray-800 dark:border-gray-700">
            <span className="text-gray-700 dark:text-gray-300">
              Non-matching invite code. Look up federation ID on Observer API?
            </span>
            <button
              type="button"
              onClick={resolveInvite}
              className="px-3 py-1.5 text-xs font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 focus:ring-2 focus:ring-blue-300 dark:focus:ring-blue-800 cursor-pointer"
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
            {refreshFailed && updatedAt !== null && (
              <p role="status" className="mt-1 text-xs font-normal text-amber-700 dark:text-amber-400">
                Could not refresh the list; showing data from {formatUtcTime(updatedAt)}. Retrying automatically.
              </p>
            )}
          </div>
          <FederationSortControl sortKey={sortKey} sortDirection={sortDirection} onChange={setSort} />
        </div>
        <FederationList
          {...listProps}
          federations={groups.active}
          loading={loading ? <FederationListSkeleton /> : null}
          emptyText={searchQuery ? 'No federations match your search' : 'No active federations found'}
        />
      </ListCard>

      <CollapsibleFederations
        {...listProps}
        id="offline-federations"
        title="Offline Federations"
        description="Federations with too few guardians answering in the last 5 minutes to reach consensus"
        emptyText={searchQuery ? 'No offline federations match your search' : 'No offline federations'}
        federations={groups.offline}
      />

      <CollapsibleFederations
        {...listProps}
        id="shut-down-federations"
        title="Shut Down Federations"
        description="Federations whose guardians announced an expiry date that has passed"
        emptyText={searchQuery ? 'No shut down federations match your search' : 'No shut down federations'}
        federations={groups.shutDown}
      />
    </div>
  );
}
