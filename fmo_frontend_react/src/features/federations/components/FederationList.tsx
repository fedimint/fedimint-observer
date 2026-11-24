import type { ReactNode } from 'react';
import type { FederationSummary } from '../../../types/api';
import { Alert } from '../../../components/ui/Alert';
import { ListMessage } from '../../../components/ui/ListCard';
import type { ActivityScale } from '../lib/federationList';
import { FederationRow } from './FederationRow';

interface FederationListProps {
  federations: FederationSummary[];

  loading: ReactNode | null;

  failed: boolean;
  emptyText: string;
  scale: ActivityScale;

  pager?: ReactNode;
  /** What the middle column represents */
  secondaryColumn?: 'assets' | 'operational';
}

/** Column headings over the rows; phones show each row's labels instead */
function FederationListHeader({ secondaryColumn }: { secondaryColumn: 'assets' | 'operational' }) {
  const shutDown = secondaryColumn === 'operational';
  return (
    <div className={`hidden md:grid bg-surface-2 px-3 sm:px-6 py-3 text-xs text-ink-2 font-medium gap-4 border-y border-line ${shutDown ? 'grid-cols-2' : 'grid-cols-3'}`}>
      <div>Name</div>
      <div title={shutDown ? 'Estimated times of the first and last sessions this observer recorded, not announced start or shutdown dates' : undefined}>
        {shutDown ? 'Observed session dates (UTC)' : 'Total assets'}
      </div>
      {!shutDown && (
        <div className="flex items-center gap-3">
          <span>Activity, last 7 days</span>
          {/* Same colours as the bars in CombinedMiniChart */}
          <span className="inline-flex items-center gap-1 font-normal"><span aria-hidden="true" className="size-2 rounded-[2px]" style={{ backgroundColor: '#3b82f6' }} />Transactions</span>
          <span className="inline-flex items-center gap-1 font-normal"><span aria-hidden="true" className="size-2 rounded-[2px] bg-ok" />Volume</span>
        </div>
      )}
    </div>
  );
}

/** Without this, a failed request would read as an empty list */
function FederationsLoadError() {
  return (
    <div className="px-3 sm:px-6 pt-4 bg-surface">
      <Alert level="error" message="Could not load the federations. Please try again later." />
    </div>
  );
}

/** Placeholder rows the height of real ones, so the page does not jump when they load */
export function FederationListSkeleton({ rows = 6 }: { rows?: number }) {
  return (
    <div role="status" aria-label="Loading federations">
      <span className="sr-only">Loading federations…</span>
      {Array.from({ length: rows }, (_, index) => <div key={index} aria-hidden="true" className="grid h-32 grid-cols-2 items-center gap-4 border-t border-line px-3 py-4 sm:px-6 md:h-22 md:grid-cols-3 xl:h-24">
        <div className="h-4 w-32 max-w-full rounded-sm bg-surface-3" />
        <div className="h-4 w-24 max-w-full rounded-sm bg-surface-3" />
        <div className="h-10 rounded-sm bg-surface-3 md:h-14 xl:h-16" />
      </div>)}
    </div>
  );
}

/** A headed list of federation rows, or its loading, error or empty state */
export function FederationList({ federations, loading, failed, emptyText, scale, pager, secondaryColumn = 'assets' }: FederationListProps) {
  return (
    <>
      <FederationListHeader secondaryColumn={secondaryColumn} />
      <div className="divide-y divide-line">
        {loading ? (
          loading
        ) : failed ? (
          <FederationsLoadError />
        ) : federations.length === 0 ? (
          <ListMessage>{emptyText}</ListMessage>
        ) : (
          federations.map((fed) => (
            <FederationRow
              key={fed.id}
              id={fed.id}
              name={fed.name || 'Unnamed'}
              totalAssets={fed.deposits}
              firstSessionTimestamp={fed.first_session_timestamp}
              lastSessionTimestamp={fed.last_session_timestamp}
              secondaryColumn={secondaryColumn}
              health={fed.health}
              expiryTimestamp={fed.expiry_timestamp}
              successorId={fed.successor_id}
              successorName={fed.successor_name ?? null}
              activityData={fed.last_7d_activity}
              maxTransaction={scale.maxTransaction}
              maxVolume={scale.maxVolume}
            />
          ))
        )}
      </div>
      {pager && <div className="border-t border-line bg-surface px-3 py-3 sm:px-6">{pager}</div>}
    </>
  );
}
