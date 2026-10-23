import { useEffect, useState } from 'react';
import { api } from '../services/api';
import { Copyable } from '../components/Copyable';

interface NostrFederation {
  id: string;
  name: string | null;
  invite: string;
  health?: 'online' | 'degraded' | 'offline';
}

interface FederationInfo {
  name?: string;
  guardians?: number;
  modules?: string[];
  network?: string;
  config?: Record<string, unknown>; // Store the full config for announce
}

interface NostrEvent {
  kind: number;
  created_at: number;
  tags: string[][];
  content: string;
  pubkey: string;
}

interface SignedNostrEvent extends NostrEvent {
  id: string;
  sig: string;
}

// Extend window type for Nostr extension
declare global {
  interface Window {
    nostr?: {
      getPublicKey(): Promise<string>;
      signEvent(event: NostrEvent): Promise<SignedNostrEvent>;
    };
  }
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
  const checkFederation = async (inviteCodeToCheck: string) => {
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
      const configResponse = await fetch(
        `${import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api'}/config/${inviteCodeToCheck}`
      );

      if (!configResponse.ok) {
        throw new Error('Failed to fetch federation config');
      }

      const config = await configResponse.json();

      // Fetch federation metadata
      const metaResponse = await fetch(
        `${import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api'}/config/${inviteCodeToCheck}/meta`
      );

      let name = 'Unknown';
      if (metaResponse.ok) {
        const meta = await metaResponse.json();
        name = meta.federation_name || 'Unknown';
      }

      // Extract info from config
      const guardians = config.global?.api_endpoints
        ? Object.keys(config.global.api_endpoints).length
        : 0;

      const modules = config.modules
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ? Object.values(config.modules).map((mod: any) => mod.kind || 'unknown')
        : [];

      const walletModule = config.modules
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        ? Object.values(config.modules).find((mod: any) => mod.kind === 'wallet')
        : null;
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const network = walletModule ? ((walletModule as any).network || 'unknown') : 'unknown';
      
      setFederationInfo({
        name,
        guardians,
        modules,
        network,
        config, // Store config for announce
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch federation info');
    } finally {
      setChecking(false);
    }
  };

  // Handle deep-linking: auto-fill input and trigger check if 'check' parameter is present
  useEffect(() => {
    const urlParams = new URLSearchParams(window.location.search);
    const checkParam = urlParams.get('check');
    
    if (checkParam) {
      setInviteCode(checkParam);
      checkFederation(checkParam);
    }
  }, []);

  useEffect(() => {
    const fetchFederationsWithNames = async () => {
      try {
        const [nostrData, mainFederations] = await Promise.all([
          api.getNostrFederations(),
          api.getFederations()
        ]);

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
        const federationsWithoutNames = nostrFeds.filter(fed => !fed.name);

        if (federationsWithoutNames.length > 0) {
          const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';

          // Fetch names for federations without them (in background, non-blocking)
          federationsWithoutNames.forEach(async (fed) => {
            try {
              const metaResponse = await fetch(`${BASE_URL}/config/${fed.invite}/meta`, {
                signal: AbortSignal.timeout(5000), // 5 second timeout
              });
              if (metaResponse.ok) {
                const meta = await metaResponse.json();
                const fetchedName = meta.federation_name || null;

                // Update this specific federation's name
                if (fetchedName) {
                  setFederations(prevFeds =>
                    prevFeds.map(f =>
                      f.id === fed.id ? { ...f, name: fetchedName } : f
                    )
                  );
                }
              }
            } catch (err) {
              console.error(`Failed to fetch name for ${fed.id}:`, err);
            }
          });
        }
      } catch (err) {
        console.error('Failed to fetch nostr federations:', err);
        setLoading(false);
      }
    };

    fetchFederationsWithNames();
  }, []);

  const handleCheckFederation = async (e: React.FormEvent) => {
    e.preventDefault();
    checkFederation(inviteCode);
  };

  const handleAnnounceFederation = async () => {
    if (!federationInfo?.config) return;

    setAnnouncing(true);
    setError(null);
    setAnnounceSuccess(false);

    try {
      // Check if Nostr extension is available
      if (!window.nostr) {
        throw new Error('Nostr extension not found. Please install a Nostr browser extension like nos2x or Alby.');
      }

      const config = federationInfo.config;

      // Calculate federation ID from config
      const federationId = await calculateFederationId();

      // Use the stored invite code (the one user entered)
      if (!inviteCode.trim()) {
        throw new Error('Invite code is required');
      }

      // Get network and modules
      const network = federationInfo.network || 'unknown';
      const modules = federationInfo.modules?.join(',') || '';

      // Get public key from Nostr extension
      const pubkey = await window.nostr.getPublicKey();

      // Create unsigned event
      const unsignedEvent: NostrEvent = {
        kind: 38173,
        created_at: Math.floor(Date.now() / 1000),
        tags: [
          ['d', federationId],
          ['u', inviteCode],
          ['n', network],
          ['modules', modules],
        ],
        content: JSON.stringify((config as { global?: { meta?: Record<string, unknown> } }).global?.meta || {}),
        pubkey,
      };

      // Sign event with Nostr extension
      const signedEvent = await window.nostr.signEvent(unsignedEvent);

      // Publish to backend
      const response = await fetch(
        `${import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api'}/nostr/federations`,
        {
          method: 'PUT',
          headers: {
            'Content-Type': 'application/json',
          },
          body: JSON.stringify(signedEvent),
        }
      );

      if (!response.ok) {
        const errorText = await response.text();
        throw new Error(`Failed to announce federation (${response.status}): ${errorText}`);
      }

      setAnnounceSuccess(true);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to announce federation');
      setAnnounceSuccess(false);
    } finally {
      setAnnouncing(false);
    }
  };

  // Helper function to calculate federation ID from config
  // This matches the Rust implementation's calculate_federation_id() function
  const calculateFederationId = async (): Promise<string> => {
    try {
      // Use the API endpoint to calculate the federation ID from the invite code
      // since calculating it client-side would require crypto libraries
      const response = await fetch(
        `${import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api'}/config/${inviteCode}/id`
      );

      if (!response.ok) {
        throw new Error('Failed to fetch federation ID');
      }

      // The endpoint returns the FederationId directly as a JSON string
      const federationId = await response.json();
      return typeof federationId === 'string' ? federationId : String(federationId);
    } catch (err) {
      throw new Error(`Failed to calculate federation ID: ${err instanceof Error ? err.message : 'Unknown error'}`);
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
                          federationInfo?.guardians || 0
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
                            {federationInfo?.modules?.map((mod) => (
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
          ) : federations.filter(fed => fed.name && fed.health !== 'offline').length === 0 ? (
            <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
              No online Nostr federations found
            </div>
          ) : (
            federations
              .filter(fed => fed.name && fed.health !== 'offline')
              .map((fed) => (
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
              <div>Federation ID</div>
              <div>Invite Code</div>
            </div>
            <div className="divide-y divide-gray-200 dark:divide-gray-700">
              {loading ? (
                <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                  Loading...
                </div>
              ) : federations.filter(fed => !fed.name || fed.health === 'offline').length === 0 ? (
                <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                  No offline Nostr federations
                </div>
              ) : (
                federations
                  .filter(fed => !fed.name || fed.health === 'offline')
                  .map((fed) => (
                    <div
                      key={fed.id}
                      className="bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 px-3 sm:px-6 py-3 sm:py-4 grid grid-cols-1 sm:grid-cols-2 gap-3 sm:gap-2 text-xs sm:text-sm"
                    >
                      <div className="font-medium text-gray-900 dark:text-white break-all">
                        <span className="text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1">Federation ID</span>
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
