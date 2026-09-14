import { useEffect, useState } from 'react';
import { api } from '../services/api';
import type { FederationSummary } from '../types/api';
import { Totals } from '../components/Totals';
import { FederationRow } from '../components/FederationRow';

export function Home() {
  const [federations, setFederations] = useState<FederationSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [collapseOffline, setCollapseOffline] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');

  useEffect(() => {
    api
      .getFederations()
      .then((data) => {
        // Order by assets under management, largest first
        setFederations([...data].sort((a, b) => b.deposits - a.deposits));
        setLoading(false);
      })
      .catch((err) => {
        console.error('Failed to fetch federations:', err);
        setLoading(false);
      });
  }, []);

  const activeFederations = federations.filter((fed) => fed.health !== 'offline');
  const offlineFederations = federations.filter((fed) => fed.health === 'offline');

  // Filter federations based on search query
  const filterFederations = (feds: FederationSummary[]) => {
    if (!searchQuery.trim()) return feds;
    
    const query = searchQuery.toLowerCase();
    return feds.filter((fed) => {
      const name = fed.name?.toLowerCase() || '';
      const invite = fed.invite?.toLowerCase() || '';
      return name.includes(query) || invite.includes(query);
    });
  };

  const filteredActiveFederations = filterFederations(activeFederations);
  const filteredOfflineFederations = filterFederations(offlineFederations);

  // Compute global maxes across all federations for consistent chart scale
  const globalMaxTransaction = federations.reduce((max, fed) => {
    const fedMax = Math.max(...fed.last_7d_activity.map(d => d.num_transactions));
    return Math.max(max, fedMax);
  }, 0);

  const globalMaxVolume = federations.reduce((max, fed) => {
    const fedMax = Math.max(...fed.last_7d_activity.map(d => d.amount_transferred / 100000000000)); // convert to BTC
    return Math.max(max, fedMax);
  }, 0);

  return (
    <div className="pb-4">
      <div className="my-8 sm:my-16">
        <Totals />
      </div>

      {/* Search Bar */}
      <div className="mb-6">
        <div className="relative">
          <div className="absolute inset-y-0 left-0 flex items-center pl-3 pointer-events-none">
            <svg
              className="w-4 h-4 text-gray-500 dark:text-gray-400"
              xmlns="http://www.w3.org/2000/svg"
              fill="none"
              viewBox="0 0 20 20"
            >
              <path
                stroke="currentColor"
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth="2"
                d="m19 19-4-4m0-7A7 7 0 1 1 1 8a7 7 0 0 1 14 0Z"
              />
            </svg>
          </div>
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="block w-full p-4 pl-10 text-sm text-gray-900 border border-gray-300 rounded-lg bg-white focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-800 dark:border-gray-600 dark:placeholder-gray-400 dark:text-white dark:focus:ring-blue-500 dark:focus:border-blue-500"
            placeholder="Search federations by name or invite code..."
          />
          {searchQuery && (
            <button
              onClick={() => setSearchQuery('')}
              className="absolute inset-y-0 right-0 flex items-center pr-3 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
            >
              <svg
                className="w-4 h-4"
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 24 24"
                stroke="currentColor"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth="2"
                  d="M6 18L18 6M6 6l12 12"
                />
              </svg>
            </button>
          )}
        </div>
      </div>

  <div className="relative overflow-x-auto bg-white shadow-md rounded-lg border border-gray-200 dark:bg-gray-800 dark:border-gray-700">
    <div className="p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800">
          Observed Federations
          <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
            List of all active federations this instance is collecting statistics on
          </p>
        </div>
        <div className="hidden md:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-400 uppercase font-semibold grid-cols-3 gap-4 border-y border-gray-200 dark:border-gray-600">
          <div>Name</div>
          <div>Total Assets</div>
          <div>Activity Charts (7d)</div>
        </div>
  <div className="divide-y divide-gray-200 dark:divide-gray-700">
          {loading ? (
            <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
              Loading...
            </div>
          ) : filteredActiveFederations.length === 0 ? (
            <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
              {searchQuery ? 'No federations match your search' : 'No active federations found'}
            </div>
          ) : (
            filteredActiveFederations.map((fed) => (
              <FederationRow
                key={fed.id}
                id={fed.id}
                name={fed.name || 'Unnamed'}
                totalAssets={fed.deposits}
                health={fed.health}
                activityData={fed.last_7d_activity}
                maxTransaction={globalMaxTransaction}
                maxVolume={globalMaxVolume}
              />
            ))
          )}
        </div>
      </div>

  <div className="relative overflow-x-auto bg-white shadow-md rounded-lg border border-gray-200 dark:bg-gray-800 dark:border-gray-700 mt-6">
        <div
          className="p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800 cursor-pointer"
          onClick={() => setCollapseOffline(!collapseOffline)}
        >
          <svg
            className={`w-3 h-3 shrink-0 inline mr-2 transition-transform ${collapseOffline ? 'rotate-180' : ''}`}
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 10 6"
          >
            <path
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
              d="M9 5 5 1 1 5"
            />
          </svg>
          <span>Shut Down Federations</span>
          <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
            List of federations that have ceased operations but were observed in the past
          </p>
        </div>
        {!collapseOffline && (
          <>
            <div className="hidden md:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-400 uppercase font-semibold grid-cols-3 gap-4 border-y border-gray-200 dark:border-gray-600">
              <div>Name</div>
              <div>Total Assets</div>
              <div>Activity Charts (7d)</div>
            </div>
            <div className="divide-y divide-gray-200 dark:divide-gray-700">
              {loading ? (
                <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                  Loading...
                </div>
              ) : filteredOfflineFederations.length === 0 ? (
                <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                  {searchQuery ? 'No offline federations match your search' : 'No offline federations'}
                </div>
              ) : (
                filteredOfflineFederations.map((fed) => (
                  <FederationRow
                    key={fed.id}
                    id={fed.id}
                    name={fed.name || 'Unnamed'}
                    totalAssets={fed.deposits}
                    health={fed.health}
                    activityData={fed.last_7d_activity}
                    maxTransaction={globalMaxTransaction}
                    maxVolume={globalMaxVolume}
                  />
                ))
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
