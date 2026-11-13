import { useState } from 'react';
import { ListCard } from '../components/ui/ListCard';
import { RetryButton } from '../components/ui/RetryButton';
import { InspectFederationCard } from '../features/nostr/components/InspectFederationCard';
import { NostrFederationList, NostrListColumns } from '../features/nostr/components/NostrFederationList';
import { useNostrFederations } from '../features/nostr/hooks/useNostrFederations';

export function Nostr() {
  const nostr = useNostrFederations();
  const [showOffline, setShowOffline] = useState(false);
  const { observedById } = nostr;
  const listProps = { loading: nostr.loading, error: nostr.error, onRetry: nostr.retry };

  return (
    <div className="pb-4">
      <InspectFederationCard />

      <ListCard className="mt-8">
        <div className="p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800">
          Online Nostr Federations
          <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
            Federations announced via Nostr that are currently online
          </p>
        </div>
        <NostrListColumns nameLabel="Name" />
        {nostr.observedError && !nostr.error && (
          <div role="alert" className="px-3 sm:px-6 py-3 text-xs sm:text-sm text-yellow-800 bg-yellow-50 dark:bg-gray-800 dark:text-yellow-300 border-t border-gray-200 dark:border-gray-700">
            <span className="font-bold">Warning: </span>
            Could not load the observer's federations, so health and observed status are unknown and federations without a name are listed as offline.{' '}
            <RetryButton onClick={nostr.retryObserved} />
          </div>
        )}
        <NostrFederationList
          {...listProps}
          federations={nostr.online}
          emptyText="No online Nostr federations found"
          nameLabel="Name"
          renderName={(fed) => (
            <div className="flex items-center gap-2 flex-wrap">
              {fed.name}
              {observedById && !observedById.has(fed.id) && (
                <span className="bg-blue-100 text-blue-800 text-xs font-medium px-2.5 py-0.5 rounded-sm dark:bg-blue-900 dark:text-blue-300">
                  Unobserved
                </span>
              )}
            </div>
          )}
        />
      </ListCard>

      <ListCard className="mt-8">
        <button
          onClick={() => setShowOffline(!showOffline)}
          className="w-full p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 flex items-center justify-between"
        >
          <div>
            <div className="flex items-center gap-2">
              <svg
                className={`w-4 h-4 transition-transform ${showOffline ? 'rotate-180' : ''}`}
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                viewBox="0 0 24 24"
              >
                <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
              </svg>
              Offline Nostr Federations
            </div>
            <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
              Federations announced via Nostr that are currently offline or unreachable
            </p>
          </div>
        </button>

        {showOffline && (
          <>
            <NostrListColumns nameLabel="Name / ID" />
            <NostrFederationList
              {...listProps}
              federations={nostr.offline}
              emptyText="No offline Nostr federations"
              nameLabel="Name / ID"
              renderName={(fed) => fed.name || fed.id}
            />
          </>
        )}
      </ListCard>
    </div>
  );
}
