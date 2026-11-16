import { useState, type ReactNode } from 'react';
import { ListCard } from '../components/ui/ListCard';
import { CursorPager } from '../components/ui/CursorPager';
import { ChevronUpIcon } from '../components/ui/icons';
import { InspectFederationCard } from '../features/nostr/components/InspectFederationCard';
import { NostrFederationList, NostrListColumns } from '../features/nostr/components/NostrFederationList';
import { useNostrFederations } from '../features/nostr/hooks/useNostrFederations';
import { usePageSize } from '../hooks/usePageSize';
import { errorMessage } from '../utils/errors';
import type { NostrFederation } from '../features/nostr/lib/federationNames';
import type { PageSize } from '../utils/pageSize';

type GroupPages = ReturnType<typeof useNostrFederations>;

interface GroupProps {
  group: GroupPages;
  label: string;
  emptyText: string;
  nameLabel: string;
  renderName: (federation: NostrFederation) => ReactNode;
  pageSize: PageSize;
  onPageSizeChange: (size: PageSize) => void;
}

/** One group's rows, its loading, error or empty state, and its pager */
function GroupList({ group, label, emptyText, nameLabel, renderName, pageSize, onPageSizeChange }: GroupProps) {
  return (
    <>
      <NostrListColumns nameLabel={nameLabel} />
      <NostrFederationList
        federations={group.federations}
        loading={group.loading}
        error={group.failed ? errorMessage(group.error?.cause) : null}
        onRetry={group.restart}
        emptyText={emptyText}
        nameLabel={nameLabel}
        renderName={renderName}
      />
      {group.page && (
        <div className="border-t border-gray-200 bg-white px-3 py-3 sm:px-6 dark:border-gray-700 dark:bg-gray-800">
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
          {group.error && (
            <p role="alert" className="mt-2 text-xs text-amber-700 dark:text-amber-400">Could not load that page: {errorMessage(group.error.cause)}</p>
          )}
        </div>
      )}
    </>
  );
}

/** A secondary group, collapsed until opened, as on the home page */
function CollapsibleGroup({ id, title, description, ...props }: GroupProps & { id: string; title: string; description: string }) {
  const [collapsed, setCollapsed] = useState(true);
  return (
    <ListCard className="mt-6">
      <button
        type="button"
        aria-expanded={!collapsed}
        aria-controls={id}
        onClick={() => setCollapsed(!collapsed)}
        className="w-full p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800"
      >
        <ChevronUpIcon className={`w-3 h-3 shrink-0 inline mr-2 transition-transform ${collapsed ? 'rotate-180' : ''}`} />
        <span>{title}</span>
        {props.group.total !== null && (
          <span className="ml-2 text-sm font-normal text-gray-500 dark:text-gray-400">({props.group.total})</span>
        )}
        <span className="mt-1 block text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">{description}</span>
      </button>
      <div id={id} hidden={collapsed}>
        {!collapsed && <GroupList {...props} />}
      </div>
    </ListCard>
  );
}

const nameOrId = (fed: NostrFederation) => fed.name || fed.id;

export function Nostr() {
  const [pageSize, setPageSize] = usePageSize('nostr-federations', 25);
  // Each group is its own paged list; the backend groups and orders them
  const online = useNostrFederations('online', pageSize);
  const offline = useNostrFederations('offline', pageSize);
  const unobserved = useNostrFederations('unobserved', pageSize);
  const pagerProps = { pageSize, onPageSizeChange: setPageSize };

  return (
    <div className="pb-4">
      <InspectFederationCard />

      <ListCard className="mt-8">
        <div className="p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800">
          <h1>
            Online Nostr Federations
            {online.total !== null && <span className="ml-2 text-sm font-normal text-gray-500 dark:text-gray-400">({online.total})</span>}
          </h1>
          <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
            Federations announced via Nostr that this observer tracks and that are currently online
          </p>
        </div>
        <GroupList
          group={online}
          label="Online Nostr federation pages"
          emptyText="No online Nostr federations found"
          nameLabel="Name"
          renderName={nameOrId}
          {...pagerProps}
        />
      </ListCard>

      <CollapsibleGroup
        id="offline-nostr-federations"
        title="Offline Nostr Federations"
        description="Tracked federations where fewer guardians answered the observer in the last 5 minutes than consensus needs"
        group={offline}
        label="Offline Nostr federation pages"
        emptyText="No offline Nostr federations"
        nameLabel="Name"
        renderName={nameOrId}
        {...pagerProps}
      />

      <CollapsibleGroup
        id="unobserved-nostr-federations"
        title="Unobserved Nostr Federations"
        description="Announced via Nostr but not tracked by this observer, so their health is unknown; names come from their guardians when they answer"
        group={unobserved}
        label="Unobserved Nostr federation pages"
        emptyText="Every Nostr federation is tracked by this observer"
        nameLabel="Name / ID"
        renderName={nameOrId}
        {...pagerProps}
      />
    </div>
  );
}
