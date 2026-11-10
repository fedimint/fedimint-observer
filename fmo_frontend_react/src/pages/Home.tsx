import { useMemo, useState } from 'react';
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
import { activityScale, groupFederations } from '../features/federations/lib/federationList';

export function Home() {
  const { federations, loading, failed, updatedAt, refreshFailed } = useFederationList();
  const { sortKey, sortDirection, setSort } = useFederationSort();
  const [searchQuery, setSearchQuery] = useState('');

  const groups = useMemo(
    () => groupFederations(federations, searchQuery, sortKey, sortDirection, updatedAt),
    [federations, searchQuery, sortKey, sortDirection, updatedAt],
  );
  // Names of every federation, so a migrating one can name a successor the observer tracks
  const successorNames = useMemo(() => federationNames(federations), [federations]);
  // Global maxes across all federations for a consistent chart scale
  const scale = useMemo(() => activityScale(federations), [federations]);
  const listProps = { loading, failed, scale, successorNames };

  return (
    <div className="pb-4">
      <div className="my-8 sm:my-16">
        <Totals />
      </div>

      <div className="mb-6">
        <SearchInput
          value={searchQuery}
          onChange={setSearchQuery}
          label="Search federations by name or invite code"
          clearLabel="Clear federation search"
          placeholder="Search federations by name or invite code..."
        />
      </div>

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
