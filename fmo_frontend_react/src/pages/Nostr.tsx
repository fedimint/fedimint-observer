import { useState, type ReactNode } from 'react';
import { CollapsibleCard } from '../components/ui/CollapsibleCard';
import { ListCard } from '../components/ui/ListCard';
import { CursorPager } from '../components/ui/CursorPager';
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
        pageSize={pageSize}
      />
      {group.page && (
        <div className="border-t border-line bg-surface px-3 py-3 sm:px-6">
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
            <p role="alert" className="mt-2 text-xs text-warn-ink">Could not load that page: {errorMessage(group.error.cause)}</p>
          )}
        </div>
      )}
    </>
  );
}

/** A secondary group, collapsed until opened, as on the home page */
function CollapsibleGroup({ id, title, description, onOpenChange, ...props }: GroupProps & { id: string; title: string; description: string; onOpenChange: (open: boolean) => void }) {
  return (
    <CollapsibleCard id={id} title={title} description={description} count={props.group.total} onOpenChange={onOpenChange}>
      <GroupList {...props} />
    </CollapsibleCard>
  );
}

const nameOrId = (fed: NostrFederation) => fed.name || fed.id;

export function Nostr() {
  const [pageSize, setPageSize] = usePageSize('nostr-federations', 25);
  const [offlineOpen, setOfflineOpen] = useState(false);
  const [unobservedOpen, setUnobservedOpen] = useState(false);
  const online = useNostrFederations('online', pageSize);
  const offline = useNostrFederations('offline', pageSize, offlineOpen);
  const unobserved = useNostrFederations('unobserved', pageSize, unobservedOpen);
  const pagerProps = { pageSize, onPageSizeChange: setPageSize };

  return (
    <div className="pb-4">
      <InspectFederationCard />

      <ListCard className="mt-8">
        <div className="p-4 sm:p-5 text-base font-medium text-left rtl:text-right text-ink bg-surface">
          <h1>
            Online Nostr Federations
            {online.total !== null && <span className="ml-2 text-sm font-normal text-ink-3">({online.total})</span>}
          </h1>
          <p className="mt-0.5 text-xs sm:text-sm font-normal text-ink-3">
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
        title="Offline Nostr federations"
        description="Tracked federations where fewer guardians answered the observer in the last 5 minutes than consensus needs"
        group={offline}
        onOpenChange={setOfflineOpen}
        label="Offline Nostr federation pages"
        emptyText="No offline Nostr federations"
        nameLabel="Name"
        renderName={nameOrId}
        {...pagerProps}
      />

      <CollapsibleGroup
        id="unobserved-nostr-federations"
        title="Unobserved Nostr federations"
        description="Announced via Nostr but not tracked by this observer, so their health is unknown; names come from their guardians when they answer"
        group={unobserved}
        onOpenChange={setUnobservedOpen}
        label="Unobserved Nostr federation pages"
        emptyText="Every Nostr federation is tracked by this observer"
        nameLabel="Name / ID"
        renderName={nameOrId}
        {...pagerProps}
      />
    </div>
  );
}
