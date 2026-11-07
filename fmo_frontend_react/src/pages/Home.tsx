import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { useSearchParams } from 'react-router-dom';
import { api } from '../services/api';
import type { FederationSummary } from '../types/api';
import { Totals } from '../components/Totals';
import { FederationRow } from '../components/FederationRow';
import { Alert } from '../components/Alert';
import { startAutoRefresh } from '../utils/autoRefresh';
import {
  DEFAULT_SORT_KEY,
  SORT_OPTIONS,
  defaultDirectionFor,
  isSortKey,
  sortFederations,
  type SortDirection,
  type SortKey,
} from '../utils/sortFederations';

export function Home() {
  const [federations, setFederations] = useState<FederationSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  // The order lives in the URL, so it survives opening a federation and
  // coming back, and can be shared. Defaults are left out of the URL.
  const [params, setParams] = useSearchParams();
  const requestedSort = params.get('sort');
  const sortKey: SortKey = isSortKey(requestedSort) ? requestedSort : DEFAULT_SORT_KEY;
  const requestedOrder = params.get('order');
  const sortDirection: SortDirection = requestedOrder === 'asc' || requestedOrder === 'desc'
    ? requestedOrder
    : defaultDirectionFor(sortKey);
  const setSort = (key: SortKey, direction: SortDirection) => setParams((previous) => {
    const next = new URLSearchParams(previous);
    if (key === DEFAULT_SORT_KEY) next.delete('sort'); else next.set('sort', key);
    if (direction === defaultDirectionFor(key)) next.delete('order'); else next.set('order', direction);
    return next;
  }, { replace: true });

  // When the shown list last loaded; set while a later refresh is failing
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [refreshFailed, setRefreshFailed] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;
    let lastSuccess: number | null = null;
    let inFlight = false;
    const loadFederations = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const data = await api.getFederations(signal);
        if (signal.aborted) return;
        setFederations(data);
        lastSuccess = Date.now();
        setUpdatedAt(lastSuccess);
        setRefreshFailed(false);
        setFailed(false);
        setLoading(false);
      } catch (err) {
        if (signal.aborted) return;
        console.error('Failed to fetch federations:', err);
        if (lastSuccess === null) {
          setFailed(true);
          setLoading(false);
        } else {
          setRefreshFailed(true);
        }
      } finally {
        inFlight = false;
      }
    };
    // No polling in hidden tabs; showing one refreshes a list over a minute old
    const stop = startAutoRefresh(() => { void loadFederations(); }, 60_000, { lastSuccessAt: () => lastSuccess });
    return () => {
      stop();
      controller.abort();
    };
  }, []);

  // Search, then order, then split: a federation is shut down once the expiry
  // its guardians published has passed (as of the last load), whatever its
  // health; otherwise offline while too few guardians answer
  const [filteredActiveFederations, filteredOfflineFederations, filteredShutDownFederations] = useMemo(() => {
    const query = searchQuery.trim().toLowerCase();
    const matching = query
      ? federations.filter((fed) => (fed.name?.toLowerCase() ?? '').includes(query) || (fed.invite?.toLowerCase() ?? '').includes(query))
      : federations;
    const active: FederationSummary[] = [];
    const offline: FederationSummary[] = [];
    const shutDown: FederationSummary[] = [];
    for (const fed of sortFederations(matching, sortKey, sortDirection)) {
      if (fed.expiry_timestamp != null && updatedAt !== null && fed.expiry_timestamp * 1000 <= updatedAt) shutDown.push(fed);
      else if (fed.health === 'offline') offline.push(fed);
      else active.push(fed);
    }
    return [active, offline, shutDown];
  }, [federations, searchQuery, sortKey, sortDirection, updatedAt]);

  // Compute global maxes across all federations for consistent chart scale
  const { globalMaxTransaction, globalMaxVolume } = useMemo(() => {
    let maxTransaction = 0;
    let maxVolume = 0; // msats
    for (const fed of federations) {
      for (const day of fed.last_7d_activity) {
        maxTransaction = Math.max(maxTransaction, day.num_transactions);
        maxVolume = Math.max(maxVolume, day.amount_transferred);
      }
    }
    return { globalMaxTransaction: maxTransaction, globalMaxVolume: maxVolume };
  }, [federations]);

  // Without this, a failed request would read as an empty list
  const loadError = (
    <div className="px-3 sm:px-6 pt-4 bg-white dark:bg-gray-800">
      <Alert level="error" message="Could not load the federations. Please try again later." />
    </div>
  );

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
            aria-label="Search federations by name or invite code"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="block w-full p-4 pl-10 text-sm text-gray-900 border border-gray-300 rounded-lg bg-white focus:ring-blue-500 focus:border-blue-500 dark:bg-gray-800 dark:border-gray-600 dark:placeholder-gray-400 dark:text-white dark:focus:ring-blue-500 dark:focus:border-blue-500"
            placeholder="Search federations by name or invite code..."
          />
          {searchQuery && (
            <button
              type="button"
              aria-label="Clear federation search"
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
    <div className="p-4 sm:p-5 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between bg-white dark:bg-gray-800">
          <div className="text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 dark:text-white">
            <h1>Observed Federations</h1>
            <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
              List of all active federations this instance is collecting statistics on
            </p>
            {refreshFailed && updatedAt !== null && (
              <p role="status" className="mt-1 text-xs font-normal text-amber-700 dark:text-amber-400">
                Could not refresh the list; showing data from {new Date(updatedAt).toLocaleTimeString()}. Retrying automatically.
              </p>
            )}
          </div>
          <div className="flex items-center gap-2 shrink-0 self-start">
            <label htmlFor="federation-sort" className="text-xs sm:text-sm font-medium text-gray-500 dark:text-gray-400">
              Sort by
            </label>
            <select
              id="federation-sort"
              value={sortKey}
              // A new metric starts "best first"; the button then reverses it
              onChange={(e) => {
                const key = e.target.value as SortKey;
                setSort(key, defaultDirectionFor(key));
              }}
              className="text-xs sm:text-sm rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-gray-900 focus:border-blue-500 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
            >
              {SORT_OPTIONS.map((option) => (
                <option key={option.key} value={option.key}>{option.label}</option>
              ))}
            </select>
            <button
              type="button"
              onClick={() => setSort(sortKey, sortDirection === 'asc' ? 'desc' : 'asc')}
              aria-label={`Sorted ${sortDirection === 'asc' ? 'ascending' : 'descending'}; reverse order`}
              title={sortDirection === 'asc' ? 'Ascending' : 'Descending'}
              className="rounded-lg border border-gray-300 bg-white p-1.5 text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:bg-gray-700 dark:text-gray-300 dark:hover:bg-gray-600"
            >
              <svg
                aria-hidden="true"
                className={`w-3 h-3 transition-transform ${sortDirection === 'asc' ? '' : 'rotate-180'}`}
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 10 6"
              >
                <path stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M9 5 5 1 1 5" />
              </svg>
            </button>
          </div>
        </div>
        <div className="hidden md:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-300 uppercase font-semibold grid-cols-3 gap-4 border-y border-gray-200 dark:border-gray-600">
          <div>Name</div>
          <div>Total Assets</div>
          <div>Activity Charts (7d)</div>
        </div>
  <div className="divide-y divide-gray-200 dark:divide-gray-700">
          {loading ? (
            <div role="status" aria-label="Loading federations">
              <span className="sr-only">Loading federations…</span>
              {Array.from({ length: 6 }, (_, index) => <div key={index} aria-hidden="true" className="grid h-32 grid-cols-2 items-center gap-4 border-t border-gray-200 px-3 py-4 dark:border-gray-700 sm:px-6 md:h-[88px] md:grid-cols-3 xl:h-24">
                <div className="h-4 w-32 max-w-full rounded-sm bg-gray-200 dark:bg-gray-700" />
                <div className="h-4 w-24 max-w-full rounded-sm bg-gray-200 dark:bg-gray-700" />
                <div className="h-10 rounded-sm bg-gray-100 dark:bg-gray-700 md:h-14 xl:h-16" />
              </div>)}
            </div>
          ) : failed ? (
            loadError
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
                expiryTimestamp={fed.expiry_timestamp}
                activityData={fed.last_7d_activity}
                maxTransaction={globalMaxTransaction}
                maxVolume={globalMaxVolume}
              />
            ))
          )}
        </div>
      </div>

      <CollapsibleFederations
        id="offline-federations"
        title="Offline Federations"
        description="Federations with too few guardians answering in the last 5 minutes to reach consensus"
        emptyText={searchQuery ? 'No offline federations match your search' : 'No offline federations'}
        federations={filteredOfflineFederations}
        loading={loading}
        loadError={failed ? loadError : null}
        maxTransaction={globalMaxTransaction}
        maxVolume={globalMaxVolume}
      />

      <CollapsibleFederations
        id="shut-down-federations"
        title="Shut Down Federations"
        description="Federations whose guardians announced an expiry date that has passed"
        emptyText={searchQuery ? 'No shut down federations match your search' : 'No shut down federations'}
        federations={filteredShutDownFederations}
        loading={loading}
        loadError={failed ? loadError : null}
        maxTransaction={globalMaxTransaction}
        maxVolume={globalMaxVolume}
      />
    </div>
  );
}

interface CollapsibleFederationsProps {
  id: string;
  title: string;
  description: string;
  emptyText: string;
  federations: FederationSummary[];
  loading: boolean;
  loadError: ReactNode;
  maxTransaction: number;
  maxVolume: number;
}

/** A secondary federation list, collapsed until opened */
function CollapsibleFederations({
  id,
  title,
  description,
  emptyText,
  federations,
  loading,
  loadError,
  maxTransaction,
  maxVolume,
}: CollapsibleFederationsProps) {
  const [collapsed, setCollapsed] = useState(true);
  const message = (text: string) => (
    <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
      {text}
    </div>
  );

  return (
    <div className="relative overflow-x-auto bg-white shadow-md rounded-lg border border-gray-200 dark:bg-gray-800 dark:border-gray-700 mt-6">
      <button
        type="button"
        aria-expanded={!collapsed}
        aria-controls={id}
        className="w-full p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800"
        onClick={() => setCollapsed(!collapsed)}
      >
        <svg
          aria-hidden="true"
          className={`w-3 h-3 shrink-0 inline mr-2 transition-transform ${collapsed ? 'rotate-180' : ''}`}
          xmlns="http://www.w3.org/2000/svg"
          fill="none"
          viewBox="0 0 10 6"
        >
          <path stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M9 5 5 1 1 5" />
        </svg>
        <span>{title}</span>
        {!loading && !loadError && (
          <span className="ml-2 text-sm font-normal text-gray-500 dark:text-gray-400">({federations.length})</span>
        )}
        <span className="mt-1 block text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
          {description}
        </span>
      </button>
      <div id={id} hidden={collapsed}>
        {!collapsed && (
          <>
            <div className="hidden md:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-300 uppercase font-semibold grid-cols-3 gap-4 border-y border-gray-200 dark:border-gray-600">
              <div>Name</div>
              <div>Total Assets</div>
              <div>Activity Charts (7d)</div>
            </div>
            <div className="divide-y divide-gray-200 dark:divide-gray-700">
              {loading
                ? message('Loading...')
                : loadError
                  ? loadError
                  : federations.length === 0
                    ? message(emptyText)
                    : federations.map((fed) => (
                      <FederationRow
                        key={fed.id}
                        id={fed.id}
                        name={fed.name || 'Unnamed'}
                        totalAssets={fed.deposits}
                        health={fed.health}
                        expiryTimestamp={fed.expiry_timestamp}
                        activityData={fed.last_7d_activity}
                        maxTransaction={maxTransaction}
                        maxVolume={maxVolume}
                      />
                    ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
