import type { ReactNode } from 'react';
import { Copyable } from '../../../components/ui/Copyable';
import { ListMessage } from '../../../components/ui/ListCard';
import { RetryButton } from '../../../components/ui/RetryButton';
import type { NostrFederation } from '../lib/federationNames';

const mobileLabel = 'text-xs sm:hidden text-ink-3 block mb-1';

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
    <div role="alert" className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-crit-ink bg-crit-soft">
      <span className="font-medium">Error: </span>{message}{' '}
      <RetryButton onClick={onRetry} />
    </div>
  );
}

/** Column headings over the rows; phones show each row's labels instead */
export function NostrListColumns({ nameLabel }: { nameLabel: string }) {
  return (
    <div className="hidden sm:grid bg-surface-2 px-3 sm:px-6 py-3 text-xs text-ink-2 font-medium grid-cols-2 gap-2 border-y border-line">
      <div>{nameLabel}</div>
      <div>Invite code</div>
    </div>
  );
}

/** Rows of federations with their invite codes, or the list's loading, error or empty state */
export function NostrFederationList({ federations, loading, error, onRetry, emptyText, nameLabel, renderName }: NostrFederationListProps) {
  return (
    <div className="divide-y divide-line">
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
            className="bg-surface hover:bg-surface-2 px-3 sm:px-6 py-3 sm:py-4 grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-2 text-xs sm:text-sm"
          >
            <div className="font-medium text-ink break-all">
              <span className={mobileLabel}>{nameLabel}</span>
              {renderName(fed)}
            </div>
            <div className='bg-transparent'>
              <span className={mobileLabel}>Invite code</span>
              <Copyable text={fed.invite} />
            </div>
          </div>
        ))
      )}
    </div>
  );
}
