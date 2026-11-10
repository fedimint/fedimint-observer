import type { ReactNode } from 'react';
import type { FederationSummary } from '../../../types/api';
import { Alert } from '../../../components/ui/Alert';
import { ListMessage } from '../../../components/ui/ListCard';
import type { ActivityScale } from '../lib/federationList';
import { FederationRow } from './FederationRow';

interface FederationListProps {
  federations: FederationSummary[];
  /** Shown instead of the rows while the list loads */
  loading: ReactNode | null;
  /** Shown instead of the rows when the list failed to load */
  failed: boolean;
  emptyText: string;
  scale: ActivityScale;
  /** Names of all federations by ID, not only these, to name each successor */
  successorNames: Map<string, string>;
}

/** Column headings over the rows; phones show each row's labels instead */
function FederationListHeader() {
  return (
    <div className="hidden md:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-300 uppercase font-semibold grid-cols-3 gap-4 border-y border-gray-200 dark:border-gray-600">
      <div>Name</div>
      <div>Total Assets</div>
      <div>Activity Charts (7d)</div>
    </div>
  );
}

/** Without this, a failed request would read as an empty list */
export function FederationsLoadError() {
  return (
    <div className="px-3 sm:px-6 pt-4 bg-white dark:bg-gray-800">
      <Alert level="error" message="Could not load the federations. Please try again later." />
    </div>
  );
}

/** Placeholder rows the height of real ones, so the page does not jump when they load */
export function FederationListSkeleton({ rows = 6 }: { rows?: number }) {
  return (
    <div role="status" aria-label="Loading federations">
      <span className="sr-only">Loading federations…</span>
      {Array.from({ length: rows }, (_, index) => <div key={index} aria-hidden="true" className="grid h-32 grid-cols-2 items-center gap-4 border-t border-gray-200 px-3 py-4 dark:border-gray-700 sm:px-6 md:h-[88px] md:grid-cols-3 xl:h-24">
        <div className="h-4 w-32 max-w-full rounded-sm bg-gray-200 dark:bg-gray-700" />
        <div className="h-4 w-24 max-w-full rounded-sm bg-gray-200 dark:bg-gray-700" />
        <div className="h-10 rounded-sm bg-gray-100 dark:bg-gray-700 md:h-14 xl:h-16" />
      </div>)}
    </div>
  );
}

/** A headed list of federation rows, or its loading, error or empty state */
export function FederationList({ federations, loading, failed, emptyText, scale, successorNames }: FederationListProps) {
  return (
    <>
      <FederationListHeader />
      <div className="divide-y divide-gray-200 dark:divide-gray-700">
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
              health={fed.health}
              expiryTimestamp={fed.expiry_timestamp}
              successorId={fed.successor_id}
              successorName={fed.successor_id ? successorNames.get(fed.successor_id) : null}
              activityData={fed.last_7d_activity}
              maxTransaction={scale.maxTransaction}
              maxVolume={scale.maxVolume}
            />
          ))
        )}
      </div>
    </>
  );
}
