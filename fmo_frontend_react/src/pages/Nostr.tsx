import { useEffect, useMemo, useState } from 'react';
import { api } from '../services/api';
import { publishNostrEvent } from '../services/nostr';
import { Copyable } from '../components/Copyable';
import type { FederationHealth } from '../types/api';
import { parseConfig, type FederationConfig } from '../utils/config';

interface NostrFederation {
  id: string;
  name: string | null;
  invite: string;
  health?: FederationHealth;
}

interface FederationInfo extends FederationConfig {
  name: string;
}

// Each name lookup makes the server fetch the federation's config, so only a few run at once
const NAME_LOOKUP_CONCURRENCY = 4;
const NAME_LOOKUP_TIMEOUT_MS = 5000;
// Names that arrive this close together update the list once
const NAME_BATCH_MS = 200;

// Meta values are parsed as JSON where possible, so a name like "21" arrives as a number
function federationName(meta: Record<string, unknown>): string | null {
  const name = meta.federation_name;
  return name ? String(name) : null;
}

// Gives up after the timeout, or as soon as `signal` aborts
async function fetchFederationName(invite: string, signal: AbortSignal): Promise<string | null> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  const timeout = setTimeout(abort, NAME_LOOKUP_TIMEOUT_MS);
  signal.addEventListener('abort', abort);
  try {
    return federationName(await api.getFederationMetaByInvite(invite, controller.signal));
  } finally {
    clearTimeout(timeout);
    signal.removeEventListener('abort', abort);
  }
}

// Looks up the names a few at a time and hands them to `onNames` in batches
async function lookUpNames(
  federations: NostrFederation[],
  signal: AbortSignal,
  onNames: (names: Map<string, string>) => void,
) {
  const pending = new Map<string, string>();
  let flushTimer: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    flushTimer = undefined;
    if (signal.aborted || pending.size === 0) return;
    onNames(new Map(pending));
    pending.clear();
  };

  let next = 0;
  const worker = async () => {
    while (next < federations.length && !signal.aborted) {
      const fed = federations[next++];
      try {
        const name = await fetchFederationName(fed.invite, signal);
        if (name) {
          pending.set(fed.id, name);
          flushTimer ??= setTimeout(flush, NAME_BATCH_MS);
        }
      } catch (err) {
        if (!signal.aborted) console.error(`Failed to fetch name for ${fed.id}:`, err);
      }
    }
  };
  await Promise.all(Array.from({ length: NAME_LOOKUP_CONCURRENCY }, worker));
  clearTimeout(flushTimer);
  flush();
}

export function Nostr() {
  const [federations, setFederations] = useState<NostrFederation[]>([]);
  const [loading, setLoading] = useState(true);
  const [inviteCode, setInviteCode] = useState('');
  const [checking, setChecking] = useState(false);
  const [federationInfo, setFederationInfo] = useState<FederationInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcing, setAnnouncing] = useState(false);
  const [announceSuccess, setAnnounceSuccess] = useState(false);
  const [showOffline, setShowOffline] = useState(false);
  const [observedFederationIds, setObservedFederationIds] = useState<Set<string>>(new Set());

  // Federation checking logic (extracted for reuse)
  const checkFederation = async (inviteCodeToCheck: string, signal?: AbortSignal) => {
    // Clear previous data and errors
    setFederationInfo(null);
    setError(null);
    setAnnounceSuccess(false);

    // Validate invite code is not empty
    if (!inviteCodeToCheck.trim()) {
      setError('Invite code not present. Please enter a valid invite code.');
      return;
    }

    setChecking(true);

    try {
      // Fetch federation config
      const config = await api.getFederationConfigByInvite(inviteCodeToCheck, signal);

      // Fetch federation metadata; it runs after the config request so the server answers it
      // from the config it just cached. Without it the name is unknown.
      const meta = await api.getFederationMetaByInvite(inviteCodeToCheck, signal).catch(() => null);
      if (signal?.aborted) return;

      setFederationInfo({
        name: (meta && federationName(meta)) || 'Unknown',
        ...parseConfig(config),
      });
    } catch (err) {
      if (signal?.aborted) return;
      setError(err instanceof Error ? err.message : 'Failed to fetch federation info');
    } finally {
      if (!signal?.aborted) setChecking(false);
    }
  };

  // Handle deep-linking: auto-fill input and trigger check if 'check' parameter is present
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search);
    const checkParam = urlParams.get('check');
    if (!checkParam) return;

    const controller = new AbortController();
    setInviteCode(checkParam);
    checkFederation(checkParam, controller.signal);
    return () => controller.abort();
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;

    const fetchFederationsWithNames = async () => {
      try {
        const [nostrData, mainFederations] = await Promise.all([
          api.getNostrFederations(signal),
          api.getFederations(signal)
        ]);
        if (signal.aborted) return;

        // Create a map of federation IDs to names from the main list
        const nameMap = new Map(
          mainFederations.map(fed => [fed.id, fed.name])
        );

        // Create a map of federation IDs to health status
        const healthMap = new Map(
          mainFederations.map(fed => [fed.id, fed.health])
        );

        // Store IDs of federations that are being observed
        const observedIds = new Set(mainFederations.map(fed => fed.id));
        setObservedFederationIds(observedIds);

        // Convert the nostr object to an array
        const nostrFeds = Object.entries(nostrData).map(([id, invite]) => ({
          id,
          name: nameMap.get(id) || null,
          invite,
          health: healthMap.get(id),
        }));

        setFederations(nostrFeds);
        setLoading(false); // Stop loading immediately - show table with IDs

        // For federations without names, fetch from /config/{invite}/meta in background
        await lookUpNames(nostrFeds.filter(fed => !fed.name), signal, (names) => {
          setFederations(prevFeds =>
            prevFeds.map(f => {
              const name = names.get(f.id);
              return name ? { ...f, name } : f;
            })
          );
        });
      } catch (err) {
        if (signal.aborted) return;
        console.error('Failed to fetch nostr federations:', err);
        setLoading(false);
      }
    };

    fetchFederationsWithNames();
    return () => controller.abort();
  }, []);

  const onlineFederations = useMemo(
    () => federations.filter(fed => fed.name && fed.health !== 'offline'),
    [federations]
  );
  const offlineFederations = useMemo(
    () => federations.filter(fed => !fed.name || fed.health === 'offline'),
    [federations]
  );

  const handleCheckFederation = async (e: React.FormEvent) => {
    e.preventDefault();
    checkFederation(inviteCode);
  };

  const handleAnnounceFederation = async () => {
    if (!federationInfo) return;

    setAnnouncing(true);
    setError(null);
    setAnnounceSuccess(false);

    try {
      await publishNostrEvent(async () => {
        // The server derives the federation ID from the invite code, since calculating
        // it client-side would require crypto libraries
        let federationId: string;
        try {
          federationId = await api.getFederationIdByInvite(inviteCode);
        } catch (err) {
          throw new Error(`Failed to calculate federation ID: ${err instanceof Error ? err.message : 'Unknown error'}`);
        }

        // Use the stored invite code (the one user entered)
        if (!inviteCode.trim()) {
          throw new Error('Invite code is required');
        }

        return {
          kind: 38173,
          tags: [
            ['d', federationId],
            ['u', inviteCode],
            ['n', federationInfo.network],
            ['modules', federationInfo.modules.join(',')],
          ],
          content: JSON.stringify(federationInfo.raw.global?.meta || {}),
        };
      }, (event) => api.publishNostrFederation(event));

      setAnnounceSuccess(true);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to announce federation');
      setAnnounceSuccess(false);
    } finally {
      setAnnouncing(false);
    }
  };

  return (
    <div className="pb-4">
  <div className="relative overflow-x-auto bg-white shadow-md rounded-lg border border-gray-200 dark:bg-gray-800 dark:border-gray-700 mt-8">
  <h1 className="p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800">
          Inspect Federation
          <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
            Fetch federation info by invite code
          </p>
        </h1>

        <div className="p-4 sm:p-5 pt-0 bg-white dark:text-white dark:bg-gray-800">
          <form className="flex flex-col sm:flex-row gap-2 sm:items-center" onSubmit={handleCheckFederation}>
            <div className="relative flex-1 w-full">
              <input
                type="text"
                value={inviteCode}
                onChange={(e) => setInviteCode(e.target.value)}
                disabled={checking || announcing}
                placeholder=" "
                className="block px-2.5 h-11 w-full text-sm text-gray-900 bg-gray-50 rounded-lg border-gray-300 appearance-none dark:text-white dark:bg-gray-700 dark:border-gray-600 dark:focus:border-blue-500 focus:outline-hidden focus:ring-0 focus:border-blue-600 peer border disabled:opacity-50 disabled:cursor-not-allowed"
              />
              <label className="absolute text-sm text-gray-600 dark:text-gray-400 duration-300 transform -translate-y-4 scale-75 top-2 z-10 origin-left bg-gray-50 dark:bg-gray-700 px-2 peer-focus:px-2 peer-focus:text-blue-600 dark:peer-focus:text-blue-500 peer-placeholder-shown:scale-100 peer-placeholder-shown:-translate-y-1/2 peer-placeholder-shown:top-1/2 peer-focus:top-2 peer-focus:scale-75 peer-focus:-translate-y-4 peer-focus:rtl:translate-x-1/4 peer-focus:rtl:left-auto inset-s-1">
                Invite Code
              </label>
            </div>
            <div className="flex flex-col sm:flex-row gap-2 w-full sm:w-auto">
              <button
                type="submit"
                disabled={checking}
                className="h-11 w-full sm:w-auto whitespace-nowrap font-medium rounded-lg text-sm px-5 py-2.5 text-white bg-blue-700 hover:bg-blue-800 focus:ring-4 focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 focus:outline-hidden dark:focus:ring-blue-800 disabled:bg-blue-400 disabled:cursor-not-allowed"
              >
                {checking ? 'Checking...' : 'Check Federation'}
              </button>
              <button
                type="button"
                onClick={handleAnnounceFederation}
                disabled={!federationInfo || announcing || announceSuccess || checking}
                className="h-11 w-full sm:w-auto whitespace-nowrap font-medium rounded-lg text-sm px-5 py-2.5 text-white bg-green-700 hover:bg-green-800 focus:ring-4 focus:ring-green-300 dark:bg-green-600 dark:hover:bg-green-700 focus:outline-hidden disabled:bg-green-400 disabled:cursor-not-allowed"
              >
                {announcing ? 'Announcing...' : announceSuccess ? 'Announced!' : 'Announce Federation'}
              </button>
            </div>
          </form>

          {(checking || federationInfo) && (
            <div className="flow-root mt-4">
              <div className="relative">
                <table className="w-full text-xs sm:text-sm text-left rtl:text-right text-gray-500 dark:text-gray-400">
                  <tbody>
                    <tr className="bg-white border-b dark:bg-gray-800 dark:border-gray-700">
                      <th scope="row" className="px-3 sm:px-6 py-3 sm:py-4 font-medium text-gray-900 dark:text-white w-24 sm:w-auto">
                        Name
                      </th>
                      <td className="px-3 sm:px-6 py-3 sm:py-4 wrap-break-word">
                        {checking ? (
                          <div className="h-2.5 bg-gray-200 rounded-full dark:bg-gray-700 w-32 sm:w-48"></div>
                        ) : (
                          federationInfo?.name || 'Unknown'
                        )}
                      </td>
                    </tr>
                    <tr className="bg-white border-b dark:bg-gray-800 dark:border-gray-700">
                      <th scope="row" className="px-3 sm:px-6 py-3 sm:py-4 font-medium text-gray-900 dark:text-white w-24 sm:w-auto">
                        Guardians
                      </th>
                      <td className="px-3 sm:px-6 py-3 sm:py-4 wrap-break-word">
                        {checking ? (
                          <div className="h-2.5 bg-gray-200 rounded-full dark:bg-gray-700 w-32 sm:w-48"></div>
                        ) : (
                          federationInfo?.guardians.length || 0
                        )}
                      </td>
                    </tr>
                    <tr className="bg-white border-b dark:bg-gray-800 dark:border-gray-700">
                      <th scope="row" className="px-3 sm:px-6 py-3 sm:py-4 font-medium text-gray-900 dark:text-white w-24 sm:w-auto align-top">
                        Modules
                      </th>
                      <td className="px-3 sm:px-6 py-3 sm:py-4 wrap-break-word">
                        {checking ? (
                          <div className="h-2.5 bg-gray-200 rounded-full dark:bg-gray-700 w-32 sm:w-48"></div>
                        ) : (
                          <div className="flex flex-wrap gap-1">
                            {federationInfo?.modules.map((mod) => (
                              <span
                                key={mod}
                                className="bg-blue-600 text-white text-xs font-medium px-2.5 py-0.5 rounded-sm  dark:bg-indigo-500"
                              >
                                {mod}
                              </span>
                            ))}
                          </div>
                        )}
                      </td>
                    </tr>
                    <tr className="bg-white border-b dark:bg-gray-800 dark:border-gray-700">
                      <th scope="row" className="px-3 sm:px-6 py-3 sm:py-4 font-medium text-gray-900 dark:text-white w-24 sm:w-auto">
                        Network
                      </th>
                      <td className="px-3 sm:px-6 py-3 sm:py-4 wrap-break-word">
                        {checking ? (
                          <div className="h-2.5 bg-gray-200 rounded-full dark:bg-gray-700 w-32 sm:w-48"></div>
                        ) : (
                          federationInfo?.network || 'Unknown'
                        )}
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {error && (
            <div className="p-3 sm:p-4 mb-4 mt-4 text-xs sm:text-sm text-red-800 rounded-lg bg-red-50 dark:bg-gray-800 dark:text-red-400 wrap-break-word">
              <span className="font-bold">Error: </span>
              {error}
            </div>
          )}

          {announceSuccess && (
            <div className="p-3 sm:p-4 mb-4 mt-4 text-xs sm:text-sm text-green-800 rounded-lg bg-green-50 dark:bg-gray-800 dark:text-green-400">
              <span className="font-bold">Success! </span>
              Federation announced successfully! Reload the page to see it listed.
            </div>
          )}
        </div>
      </div>

      {/* Online Nostr Federations */}
      <div className="relative overflow-x-auto bg-white shadow-md rounded-lg border border-gray-200 dark:bg-gray-800 dark:border-gray-700 mt-8">
        <div className="p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800">
          Online Nostr Federations
          <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
            Federations announced via Nostr that are currently online
          </p>
        </div>
        <div className="hidden sm:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-400 uppercase font-semibold grid-cols-2 gap-2 border-y border-gray-200 dark:border-gray-600">
          <div>Name</div>
          <div>Invite Code</div>
        </div>
        <div className="divide-y divide-gray-200 dark:divide-gray-700">
          {loading ? (
            <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
              Loading...
            </div>
          ) : onlineFederations.length === 0 ? (
            <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
              No online Nostr federations found
            </div>
          ) : (
            onlineFederations.map((fed) => (
              <div
                key={fed.id}
                className="bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 px-3 sm:px-6 py-3 sm:py-4 grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-2 text-xs sm:text-sm"
              >
                <div className="font-medium text-gray-900 dark:text-white break-all">
                  <span className="text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1">Name</span>
                  <div className="flex items-center gap-2 flex-wrap">
                    {fed.name}
                    {!observedFederationIds.has(fed.id) && (
                      <span className="bg-blue-100 text-blue-800 text-xs font-medium px-2.5 py-0.5 rounded-sm dark:bg-blue-900 dark:text-blue-300">
                        Unobserved
                      </span>
                    )}
                  </div>
                </div>
                <div className='bg-transparent'>
                  <span className="text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1">Invite Code</span>
                  <Copyable text={fed.invite} />
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Offline Nostr Federations */}
      <div className="relative overflow-x-auto bg-white shadow-md rounded-lg border border-gray-200 dark:bg-gray-800 dark:border-gray-700 mt-8">
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
            <div className="hidden sm:grid bg-gray-50 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-400 uppercase font-semibold grid-cols-2 gap-2 border-y border-gray-200 dark:border-gray-600">
              <div>Name / ID</div>
              <div>Invite Code</div>
            </div>
            <div className="divide-y divide-gray-200 dark:divide-gray-700">
              {loading ? (
                <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                  Loading...
                </div>
              ) : offlineFederations.length === 0 ? (
                <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                  No offline Nostr federations
                </div>
              ) : (
                offlineFederations.map((fed) => (
                  <div
                    key={fed.id}
                    className="bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 px-3 sm:px-6 py-3 sm:py-4 grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-2 text-xs sm:text-sm"
                  >
                    <div className="font-medium text-gray-900 dark:text-white break-all">
                      <span className="text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1">Name / ID</span>
                      {fed.name || fed.id}
                    </div>
                    <div className='bg-transparent'>
                      <span className="text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1">Invite Code</span>
                      <Copyable text={fed.invite} />
                    </div>
                  </div>
                ))
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
