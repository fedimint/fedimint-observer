import type { ReactNode } from 'react';
import { Copyable } from '../../../components/ui/Copyable';
import { ListMessage } from '../../../components/ui/ListCard';
import { RetryButton } from '../../../components/ui/RetryButton';
import type { NostrFederation } from '../lib/federationNames';

const mobileLabel = 'text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1';

interface NostrFederationListProps {
  federations: NostrFederation[];
  loading: boolean;
  error: string | null;
  onRetry: () => void;
  emptyText: string;
  /** Heading of the name column, e.g. "Name / ID" */
  nameLabel: string;
  renderName: (federation: NostrFederation) => ReactNode;
}

function ListError({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <div role="alert" className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-red-800 bg-red-50 dark:bg-gray-800 dark:text-red-400">
      <span className="font-bold">Error: </span>{message}{' '}
      <RetryButton onClick={onRetry} />
    </div>
  );
}

/** Column headings over the rows; phones show each row's labels instead */
export function NostrListColumns({ nameLabel }: { nameLabel: string }) {
  return (
    <div className="hidden sm:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-400 uppercase font-semibold grid-cols-2 gap-2 border-y border-gray-200 dark:border-gray-600">
      <div>{nameLabel}</div>
      <div>Invite Code</div>
    </div>
  );
}

/** Rows of federations with their invite codes, or the list's loading, error or empty state */
export function NostrFederationList({ federations, loading, error, onRetry, emptyText, nameLabel, renderName }: NostrFederationListProps) {
  return (
    <div className="divide-y divide-gray-200 dark:divide-gray-700">
      {loading ? (
        <ListMessage>Loading...</ListMessage>
      ) : error ? (
        <ListError message={`Could not load Nostr federations: ${error}`} onRetry={onRetry} />
      ) : federations.length === 0 ? (
        <ListMessage>{emptyText}</ListMessage>
      ) : (
        federations.map((fed) => (
          <div
            key={fed.id}
            className="bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 px-3 sm:px-6 py-3 sm:py-4 grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-2 text-xs sm:text-sm"
          >
            <div className="font-medium text-gray-900 dark:text-white break-all">
              <span className={mobileLabel}>{nameLabel}</span>
              {renderName(fed)}
            </div>
            <div className='bg-transparent'>
              <span className={mobileLabel}>Invite Code</span>
              <Copyable text={fed.invite} />
            </div>
          </div>
        ))
      )}
    </div>
  );
}
