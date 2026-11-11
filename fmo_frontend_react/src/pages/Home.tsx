import { useState } from 'react';
import { ListCard } from '../components/ui/ListCard';
import { SearchInput } from '../components/ui/SearchInput';
import { CursorPager } from '../components/ui/CursorPager';
import { Totals } from '../features/federations/components/Totals';
import { FederationSortControl } from '../features/federations/components/FederationSortControl';
import { FederationList, FederationListSkeleton } from '../features/federations/components/FederationList';
import { CollapsibleFederations } from '../features/federations/components/CollapsibleFederations';
import { useFederationGroup } from '../features/federations/hooks/useFederationGroup';
import { useFederationSort } from '../features/federations/hooks/useFederationSort';
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
  const [searchQuery, setSearchQuery] = useState('');
  const [pageSize, setPageSize] = usePageSize('federations', 25);
  // The backend searches; it is asked once typing pauses, not on every key
  const search = useDebouncedValue(searchQuery);
  // Each group is its own paged list; the backend groups, searches and orders
  const active = useFederationGroup('active', search, sortKey, sortDirection, pageSize);
  const offline = useFederationGroup('offline', search, sortKey, sortDirection, pageSize);
  const shutDown = useFederationGroup('shut_down', search, sortKey, sortDirection, pageSize);
  const pagerProps = { pageSize, onPageSizeChange: setPageSize };

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
        description="Federations with too few guardians answering in the last 5 minutes to reach consensus"
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
