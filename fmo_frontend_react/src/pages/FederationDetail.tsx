import { useEffect, useRef, useState, useMemo, lazy, Suspense } from 'react';
import { useParams, Link } from 'react-router-dom';
import { QRCodeSVG } from 'qrcode.react';
import { api, ApiError } from '../services/api';
import { errorMessage } from '../utils/errors';
import { publishNostrEvent } from '../services/nostr';
import type {
  FederationOverview,
  FederationSummary,
  FederationUtxosResponse,
  GuardianClaimedUtxoState,
  GuardianHealth,
  GuardianUtxoReport,
  HistogramAverage,
  UtxoView,
} from '../types/api';
import type { HistogramEntry } from '../utils/histogram';
import { Badge } from '../components/Badge';
import { Alert } from '../components/Alert';
import { Copyable } from '../components/Copyable';
import { WalletV2Panel } from '../components/WalletV2Panel';
import type { GatewayPageHandoff } from '../utils/gateways';
import { guardianThreshold, parseConfig, type FederationConfig } from '../utils/config';
import { asBitcoin, MSATS_PER_BTC, shortMiddle } from '../utils/format';
import { explorerAddressUrl, explorerTxUrl, nostrNetworkTag } from '../utils/network';
import { toHistogramEntries } from '../utils/histogram';
import { formatUtcDateTime } from '../utils/time';

const UTXOS_PER_PAGE = 25;
const UTXO_SEARCH_DEBOUNCE_MS = 300;
const NO_GUARDIANS: GuardianUtxoReport[] = [];

interface UtxoInventoryRow {
  outPoint: string;
  amount: number;
  address: string | null;
  /** State each responding guardian lists this output in */
  guardianStates: Map<number, GuardianClaimedUtxoState>;
  kind: Exclude<UtxoView, 'all'>;
  detail?: string;
}

const UTXO_KIND_STYLES: Record<UtxoInventoryRow['kind'], { label: string; pill: string; detail?: string }> = {
  verified: {
    label: 'Verified',
    pill: 'bg-emerald-100 text-emerald-800 dark:bg-emerald-900/50 dark:text-emerald-300',
    detail: 'text-gray-500 dark:text-gray-400',
  },
  pending: {
    label: 'Pending',
    pill: 'bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300',
    detail: 'text-amber-700 dark:text-amber-300/90',
  },
  mismatch: {
    label: 'Mismatch',
    pill: 'bg-rose-100 text-rose-800 dark:bg-rose-900/50 dark:text-rose-300',
    detail: 'text-rose-700 dark:text-rose-300/90',
  },
};

// Page numbers to show around the current page, with gaps for skipped ranges
function utxoPageItems(current: number, total: number): (number | 'gap')[] {
  const pages = [...new Set([1, current - 1, current, current + 1, total])]
    .filter((page) => page >= 1 && page <= total)
    .sort((left, right) => left - right);
  return pages.flatMap((page, index) =>
    index > 0 && page - pages[index - 1] > 1 ? (['gap', page] as const) : [page]
  );
}

// Lazy load the chart component for code splitting
const TransactionChart = lazy(() => import('../components/TransactionChart').then(module => ({ default: module.TransactionChart })));

export function FederationDetail() {
  const { id } = useParams<{ id: string }>();
  const [federation, setFederation] = useState<FederationSummary | null>(null);
  const [config, setConfig] = useState<FederationConfig | null>(null);
  // One page of the UTXO comparison; the server filters, counts and pages it
  const [utxoData, setUtxoData] = useState<FederationUtxosResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'activity' | 'utxos' | 'assets' | 'config'>('activity');
  // Once opened, the wallet panel stays mounted so switching tabs keeps its
  // history instead of fetching it again.
  const [assetsOpened, setAssetsOpened] = useState(false);
  // UTXOs load the first time their tab opens, and are only ever shown there
  const [utxosRequested, setUtxosRequested] = useState(false);
  const [utxosLoading, setUtxosLoading] = useState(true);
  // A 404 means the federation has no Wallet v1 module, which retrying won't change
  const [utxosError, setUtxosError] = useState<{ message: string; retryable: boolean } | null>(null);
  const [utxosReload, setUtxosReload] = useState(0);
  const [overview, setOverview] = useState<FederationOverview | null>(null);
  const [overviewFailed, setOverviewFailed] = useState(false);
  const [utxoView, setUtxoView] = useState<UtxoView>('all');
  const [utxoPage, setUtxoPage] = useState(1);
  const [utxoSearch, setUtxoSearch] = useState('');
  // The search sent to the server, once typing pauses
  const [utxoQuery, setUtxoQuery] = useState('');
  const [rating, setRating] = useState(5);
  const [comment, setComment] = useState('');
  const [ratingError, setRatingError] = useState<string | null>(null);
  const [ratingSuccess, setRatingSuccess] = useState(false);
  const [histogram, setHistogram] = useState<HistogramEntry[]>([]);
  const [histogramLoading, setHistogramLoading] = useState(true);
  const [histogramError, setHistogramError] = useState<string | null>(null);
  const [histogramReload, setHistogramReload] = useState(0);
  const [chartMetric, setChartMetric] = useState<'volume' | 'count'>('volume');
  const [filterOutliers, setFilterOutliers] = useState(true);
  const [movingAverageWindow, setMovingAverageWindow] = useState<HistogramAverage>(0);
  const [histogramTotals, setHistogramTotals] = useState({ volume: 0, count: 0 });
  // Outliers only distort the volume chart, so the transaction chart keeps them
  const excludeOutliers = chartMetric === 'volume' && filterOutliers;
  const [useLogScale, setUseLogScale] = useState(false);
  const [guardianHealth, setGuardianHealth] = useState<Record<string, GuardianHealth>>({});
  const [healthStatus, setHealthStatus] = useState<'loading' | 'loaded' | 'failed'>('loading');
  const [healthReload, setHealthReload] = useState(0);
  const network = config?.network;
  const hasWalletV1 = config?.modules.includes('wallet') ?? false;
  const hasWalletV2 = config?.modules.includes('walletv2') ?? false;
  const hasLightningV1 = config?.modules.includes('ln') ?? false;
  const hasLightningV2 = config?.modules.includes('lnv2') ?? false;
  const expiryDate = federation?.expiry_timestamp
    ? new Date(federation.expiry_timestamp * 1000)
    : null;
  const shuttingDown = expiryDate !== null;
  
  // Check if at least one guardian is online
  const hasOnlineGuardian = useMemo(() => {
    return Object.values(guardianHealth).some(
      (health) => health?.latest !== null && health?.latest !== undefined
    );
  }, [guardianHealth]);
  // The invite is hidden while every guardian is known to be offline, since joining
  // would fail. When the guardians could not be checked, the federation list's own
  // health decides: an unknown status is no reason to withhold a working invite.
  const showInvite = healthStatus === 'loaded'
    ? hasOnlineGuardian
    : healthStatus === 'failed' && federation?.health !== 'offline';

  const utxoGuardians = utxoData?.guardians ?? NO_GUARDIANS;
  const utxoThreshold = utxoData?.threshold ?? 0;
  const comparedGuardianIds = useMemo(
    () => utxoGuardians
      .filter((guardian) => guardian.status === 'ok')
      .map((guardian) => guardian.guardian_id)
      .sort((left, right) => left - right),
    [utxoGuardians]
  );
  const successfulGuardianCount = comparedGuardianIds.length;

  // The rows of the page shown, reshaped for display
  const visibleUtxos = useMemo(
    () => (utxoData?.utxos ?? []).map((row): UtxoInventoryRow => ({
      outPoint: row.out_point,
      amount: row.amount,
      address: row.address,
      guardianStates: new Map(
        Object.entries(row.guardian_states).map(([guardianId, state]) => [Number(guardianId), state])
      ),
      kind: row.status,
      detail: row.detail ?? undefined,
    })),
    [utxoData]
  );
  const utxoCounts = utxoData?.counts;
  const utxoTotalRows = utxoData?.total_rows ?? 0;
  const utxoPageCount = Math.max(1, utxoData?.page_count ?? 1);
  // The server clamps a page past the end, so its answer names the page shown
  const currentUtxoPage = utxoData?.page ?? utxoPage;
  const utxoPageStart = (currentUtxoPage - 1) * (utxoData?.limit ?? UTXOS_PER_PAGE);
  const guardianSummaryUnavailable = utxoGuardians.length > 0 && successfulGuardianCount === 0;

  const tooFewGuardians = successfulGuardianCount > 0 && successfulGuardianCount < utxoThreshold;
  // Deviations from what a threshold of guardians agrees on, one sentence each
  const guardianFindings = utxoGuardians.flatMap((guardian) => [
    guardian.missing_outputs > 0
      && `Guardian ${guardian.guardian_id} is missing ${pluralOutputs(guardian.missing_outputs)} the other guardians agree the federation holds.`,
    guardian.extra_outputs > 0
      && `Guardian ${guardian.guardian_id} lists ${pluralOutputs(guardian.extra_outputs)} the other guardians agree the federation does not hold.`,
    guardian.wrong_amounts > 0
      && `Guardian ${guardian.guardian_id} reports a different amount for ${pluralOutputs(guardian.wrong_amounts)}.`,
  ].filter((finding): finding is string => Boolean(finding)));
  const laggingGuardians = utxoGuardians.filter((guardian) => guardian.status === 'lagging');

  // A new search starts on its first page, once typing pauses
  useEffect(() => {
    const query = utxoSearch.trim();
    if (query === utxoQuery) return;
    const timer = setTimeout(() => {
      setUtxoQuery(query);
      setUtxoPage(1);
    }, UTXO_SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [utxoSearch, utxoQuery]);
  
  // Calculate initial zoom to show last 3 months of data by default
  const initialZoom = useMemo(() => {
    if (histogram.length === 0) return { start: 0, end: 100 };
    
    // Use timestamps to compute last 90 days (3 months ~ 90 days)
    const THREE_MONTHS_MS = 90 * 24 * 60 * 60 * 1000;
    const lastTs = histogram[histogram.length - 1].timestamp;
    const cutoffTs = lastTs - THREE_MONTHS_MS;
    
    // Find first index with timestamp >= cutoff
    let startIndex = histogram.findIndex((h) => h.timestamp >= cutoffTs);
    if (startIndex === -1) {
      // If no entry is within the last 90 days, show all
      startIndex = 0;
    }
    
    // Calculate percentage
    const startPercent = (startIndex / histogram.length) * 100;
    return { start: startPercent, end: 100 };
  }, [histogram]);
  
  const [zoomState, setZoomState] = useState<{ start: number; end: number }>(initialZoom);

  useEffect(() => {
    if (!id) return;
    const controller = new AbortController();
    const { signal } = controller;

    setLoading(true);
    setError(null);
    setConfig(null);
    setOverview(null);
    setOverviewFailed(false);
    setHistogram([]);
    setHistogramTotals({ volume: 0, count: 0 });
    setUtxosRequested(false);
    setUtxoData(null);
    setUtxoView('all');
    setUtxoSearch('');
    setUtxoQuery('');
    setUtxoPage(1);
    setUtxosLoading(true);
    setUtxosError(null);
    setActiveTab('activity');
    setAssetsOpened(false);

    // Every request below needs only the id, so none waits for another; only the
    // invite fallback waits for the federation list.
    // Fetch all federations and find the one matching the ID
    const federationRequest = api.getFederations(signal).then((federations) => {
      const fed = federations.find((f) => f.id === id);
      if (!fed) {
        throw new Error('Federation not found');
      }
      return fed;
    });
    // Fetch config using federation ID first (works for actively observed federations),
    // falling back to the invite code, which works for any federation with a valid invite
    const configRequest = api.getFederationConfig(id, signal)
      .catch(async () => api.getFederationConfigByInvite((await federationRequest).invite, signal))
      .then(parseConfig);

    Promise.all([federationRequest, configRequest])
      .then(([fed, configData]) => {
        if (signal.aborted) return;
        setFederation(fed);
        setConfig(configData);
        setLoading(false);
      })
      .catch((err) => {
        if (signal.aborted) return;
        console.error('Failed to fetch federation details:', err);
        setError(err.message);
        setLoading(false);
      });

    api.getFederationOverview(id, signal)
      .then((data) => {
        if (!signal.aborted) setOverview(data);
      })
      .catch((err) => {
        if (signal.aborted) return;
        console.error('Failed to fetch federation overview:', err);
        setOverviewFailed(true);
      });

    return () => controller.abort();
  }, [id]);

  useEffect(() => {
    if (!id) return;
    const controller = new AbortController();
    const { signal } = controller;
    setHistogramLoading(true);
    setHistogramError(null);

    api.getFederationHistogram(id, { average: movingAverageWindow, excludeOutliers }, signal)
      .then((data) => {
        if (signal.aborted) return;
        setHistogram(toHistogramEntries(data, excludeOutliers));
        setHistogramTotals({ volume: data.total_volume_msat / MSATS_PER_BTC, count: data.total_count });
      })
      .catch((err) => {
        if (signal.aborted) return;
        console.error('Failed to fetch histogram:', err);
        setHistogramError(errorMessage(err, 'Failed to fetch activity'));
      })
      .finally(() => {
        if (!signal.aborted) setHistogramLoading(false);
      });

    return () => controller.abort();
  }, [id, histogramReload, movingAverageWindow, excludeOutliers]);

  useEffect(() => {
    if (!id) return;
    const controller = new AbortController();
    const { signal } = controller;
    setGuardianHealth({});
    setHealthStatus('loading');

    api.getFederationHealth(id, signal)
      .then((data) => {
        if (signal.aborted) return;
        setGuardianHealth(data);
        setHealthStatus('loaded');
      })
      .catch((err) => {
        if (signal.aborted) return;
        console.error('Failed to fetch guardian health:', err);
        setHealthStatus('failed');
      });

    return () => controller.abort();
  }, [id, healthReload]);

  // The server asks every guardian and mempool.space for these, so only when the tab is opened
  useEffect(() => {
    if (!id || !utxosRequested || !hasWalletV1) return;
    // A newer view, search or page aborts the request it replaces
    const controller = new AbortController();
    const { signal } = controller;
    setUtxosLoading(true);
    setUtxosError(null);

    api.getFederationUtxos(id, { view: utxoView, search: utxoQuery, page: utxoPage, limit: UTXOS_PER_PAGE }, signal)
      .then((data) => {
        if (!signal.aborted) setUtxoData(data);
      })
      .catch((err) => {
        if (signal.aborted) return;
        console.error('Failed to fetch UTXOs:', err);
        // Rows from an earlier request would read as the answer to this one
        setUtxoData(null);
        setUtxosError({
          message: errorMessage(err, 'Failed to fetch UTXOs'),
          retryable: !(err instanceof ApiError && err.status === 404),
        });
      })
      .finally(() => {
        if (!signal.aborted) setUtxosLoading(false);
      });

    return () => controller.abort();
  }, [id, utxosRequested, hasWalletV1, utxoView, utxoQuery, utxoPage, utxosReload]);

  const walletSessions = overview?.module_sessions.wallet;
  const walletBehind = overview != null && walletSessions != null && walletSessions < overview.session_count;

  // Zoom to the last three months when a federation's activity first loads; later
  // reloads for another average or outlier setting keep the viewer's zoom
  const zoomedFederation = useRef<string | null>(null);
  useEffect(() => {
    if (histogram.length > 0 && zoomedFederation.current !== id) {
      zoomedFederation.current = id ?? null;
      setZoomState(initialZoom);
    }
  }, [histogram.length, initialZoom, id]);

  const handleRateSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setRatingError(null);
    setRatingSuccess(false);

    try {
      await publishNostrEvent(() => {
        if (!id) throw new Error('Federation ID missing');
        // Without a known network the tag is left out rather than guessed
        const network = nostrNetworkTag(config?.network);

        // Kind 38000 for rating
        return {
          kind: 38000,
          tags: [
            ['d', id],
            ...(network ? [['n', network]] : []),
            ['k', '38173'],
          ],
          content: `[${rating}/5] ${comment}`,
        };
      }, (event) => api.publishNostrRating(event));

      setRatingSuccess(true);
      setComment('');
    } catch (err) {
      setRatingError(errorMessage(err, 'Failed to publish rating'));
    }
  };

  if (loading) {
    return (
      <div className="flex justify-center items-center min-h-[400px]">
        <div className="text-gray-500 dark:text-gray-400">Loading...</div>
      </div>
    );
  }

  if (error || !federation) {
    return (
      <div className="flex justify-center items-center min-h-[400px]">
        <div className="text-red-500">Error: {error || 'Federation not found'}</div>
      </div>
    );
  }

  return (
    <div className="py-4 sm:py-8 px-4 sm:px-0">
      <div className="mb-4 sm:mb-6">
        <Link
          to="/"
          className="text-sm sm:text-base text-blue-600 dark:text-blue-400 hover:underline"
        >
          ← Back to Federations
        </Link>
      </div>

      <h1 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white mb-6 sm:mb-8 wrap-break-word">
        {federation.name || 'Unnamed Federation'}
      </h1>
      {expiryDate && (
        <div className={`mb-4 rounded-lg border p-3 text-sm ${
          expiryDate.getTime() <= Date.now()
            ? 'border-red-500/60 bg-red-900/20 text-red-300'
            : 'border-yellow-500/60 bg-yellow-900/20 text-yellow-300'
        }`}>
          <strong>{expiryDate.getTime() <= Date.now() ? 'Shut down' : 'Shutting down'}</strong>
          {' '}on {formatUtcDateTime(expiryDate)}.
          {federation.successor && (
            <>
              {' '}Successor: {/^https?:\/\//.test(federation.successor) ? (
                <a className="underline" href={federation.successor}>{federation.successor}</a>
              ) : (
                <span className="font-mono break-all">{federation.successor}</span>
              )}
            </>
          )}
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 sm:gap-6">
        {/* Guardians Panel */}
        <div className="lg:col-span-2 bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:p-6">
          <h2 className="text-base sm:text-lg font-semibold text-gray-900 dark:text-white mb-3 sm:mb-4">
            Guardians
            {config && (
              <span className="ml-2 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
                Consensus needs {guardianThreshold(config.guardians.length)} of {config.guardians.length} guardians
              </span>
            )}
          </h2>
          {healthStatus === 'failed' && (
            <div className="mb-3 flex flex-wrap items-center gap-x-2 gap-y-1 rounded-lg bg-red-50 p-3 text-xs text-red-800 dark:bg-gray-900/60 dark:text-red-400 sm:text-sm" role="alert">
              <span><span className="font-bold">Error: </span>Could not check the guardians' status.</span>
              <button type="button" onClick={() => setHealthReload((count) => count + 1)} className="font-medium underline hover:no-underline">
                Try again
              </button>
            </div>
          )}
          {config ? (
            <div className="space-y-3 sm:space-y-4">
              {config.guardians.map((guardian) => {
                const health = guardianHealth[guardian.id.toString()];
                const isLoading = healthStatus === 'loading';
                const isOnline = health?.latest !== null && health?.latest !== undefined;
                const session = health?.latest?.session_count || 0;
                const block = health?.latest?.block_height ?? null;
                const sessionOutdated = health?.latest?.session_outdated || false;
                const blockOutdated = health?.latest?.block_outdated || false;
                const softwareVersion = health?.software_version || 'Version unknown';

                return (
                  <div key={guardian.id} className="border-b border-gray-200 dark:border-gray-700 pb-3 sm:pb-4 last:border-0">
                    <div className="font-medium text-sm sm:text-base text-gray-900 dark:text-white mb-1">
                      Guardian {guardian.id}
                    </div>
                    <div className="text-xs sm:text-sm text-gray-600 dark:text-gray-400 mb-2 break-all">
                      {guardian.url}
                    </div>
                    <div className="flex gap-2 flex-wrap">
                      {isLoading ? (
                        <span className="text-xs sm:text-sm font-medium text-gray-500 dark:text-gray-400">
                          Loading...
                        </span>
                      ) : !health ? (
                        <Badge
                          level="warning"
                          tooltip={healthStatus === 'failed'
                            ? 'The guardian status request failed'
                            : 'The observer has no status recorded for this guardian'}
                        >
                          Status unknown
                        </Badge>
                      ) : (
                        <>
                          <Badge level={isOnline ? 'success' : 'error'}>
                            {isOnline ? 'Online' : 'Offline'}
                          </Badge>
                          <Badge level="info">
                            {softwareVersion}
                          </Badge>
                          {isOnline && (
                            <>
                              <Badge
                                level={sessionOutdated ? 'warning' : 'info'}
                                tooltip={sessionOutdated ? 'Guardian is lacking behind others' : undefined}
                              >
                                Session {session}
                              </Badge>
                              <Badge
                                level={blockOutdated ? 'warning' : 'info'}
                                tooltip={blockOutdated ? "Guardian's bitcoind is out of sync" : undefined}
                              >
                                {block === null ? 'Block height unavailable' : `Block ${block}`}
                              </Badge>
                            </>
                          )}
                        </>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="text-sm text-gray-500 dark:text-gray-400">Loading guardians...</div>
          )}
        </div>

        {/* Right Column */}
        <div className="space-y-4 sm:space-y-6">
          {/* Federation Info Panel */}
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:p-6">
            <h2 className="text-base sm:text-lg font-semibold text-gray-900 dark:text-white mb-3 sm:mb-4">
              Federation
            </h2>
            <div className="space-y-3 sm:space-y-4">
              <div>
                <div className="text-xs sm:text-sm text-gray-500 dark:text-gray-400 mb-1">Network</div>
                <div className="text-sm sm:text-base text-gray-900 dark:text-white">
                  {config?.network || 'Unknown'}
                </div>
              </div>
              <div>
                <div className="text-xs sm:text-sm text-gray-500 dark:text-gray-400 mb-1">Modules</div>
                <div className="flex gap-2 flex-wrap">
                  {config?.modules.map((module) => (
                    <Badge key={module} level="info">
                      {module}
                    </Badge>
                  ))}
                </div>
              </div>
              <div>
                <div className="text-xs sm:text-sm text-gray-500 dark:text-gray-400 mb-1">
                  Confirmations Required
                </div>
                <div className="text-sm sm:text-base text-gray-900 dark:text-white">
                  {config?.confirmationsRequired || 'N/A'}
                </div>
              </div>
              {id && hasLightningV1 && (
                <div>
                  <Link
                    to={`/federations/${id}/gateways`}
                    className="inline-flex items-center justify-center w-full px-4 py-2.5 text-sm font-medium text-white bg-blue-700 hover:bg-blue-800 rounded-lg focus:ring-4 focus:outline-hidden focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800"
                  >
                    Gateway Details
                  </Link>
                </div>
              )}
              {id && hasLightningV2 && (
                <Link to={`/federations/${id}/gateways/v2`} state={{ id, name: federation?.name ?? null, config: config?.raw } satisfies GatewayPageHandoff} className="inline-flex w-full items-center justify-center rounded-lg bg-blue-700 px-4 py-2.5 text-sm font-medium text-white hover:bg-blue-800 dark:bg-blue-600 dark:hover:bg-blue-700">
                  Lightning v2 Gateway Details
                </Link>
              )}
              {federation?.invite && showInvite && !shuttingDown && (
                <div>
                  <div className="text-xs sm:text-sm text-gray-500 dark:text-gray-400 mb-2">Invite Link</div>
                  {healthStatus === 'failed' && (
                    <p className="mb-2 text-xs text-yellow-700 dark:text-yellow-300">
                      Guardian status could not be checked, so whether this federation accepts new users right now is unknown.
                    </p>
                  )}
                  <div className="flex flex-col items-center bg-white dark:bg-gray-900 p-3 rounded-lg border border-gray-200 dark:border-gray-600">
                    <QRCodeSVG
                      value={federation.invite}
                      size={160}
                      level="M"
                      marginSize={4}
                      className="w-full h-auto max-w-[160px]"
                    />
                    <div className="mt-3 w-full">
                      <Copyable text={federation.invite} />
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>

          {/* Recommend Section */}
          <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:p-6">
            <h2 className="text-base sm:text-lg font-semibold text-gray-900 dark:text-white mb-3 sm:mb-4">
              Recommend
            </h2>
            <form onSubmit={handleRateSubmit}>
              {ratingError && (
                <Alert level="error" message={ratingError} />
              )}
              {ratingSuccess && (
                <Alert level="success" message="Your recommendation was published!" />
              )}

              <div className="mb-4">
                <div className="flex items-center gap-1 sm:gap-2">
                  {/* Star Selector */}
                  {[1, 2, 3, 4, 5].map((star) => (
                    <svg
                      key={star}
                      onClick={() => setRating(star)}
                      className={`w-5 h-5 sm:w-6 sm:h-6 cursor-pointer ${
                        star <= rating ? 'text-yellow-300' : 'text-gray-300 dark:text-gray-500'
                      }`}
                      fill="currentColor"
                      viewBox="0 0 22 20"
                      xmlns="http://www.w3.org/2000/svg"
                    >
                      <path d="M20.924 7.625a1.523 1.523 0 0 0-1.238-1.044l-5.051-.734-2.259-4.577a1.534 1.534 0 0 0-2.752 0L7.365 5.847l-5.051.734A1.535 1.535 0 0 0 1.463 9.2l3.656 3.563-.863 5.031a1.532 1.532 0 0 0 2.226 1.616L11 17.033l4.518 2.375a1.534 1.534 0 0 0 2.226-1.617l-.863-5.03L20.537 9.2a1.523 1.523 0 0 0 .387-1.575Z"/>
                    </svg>
                  ))}
                  <span className="ml-2 sm:ml-4 text-lg sm:text-xl text-gray-900 dark:text-white">
                    {rating}/5
                  </span>
                </div>
              </div>

              <div className="mb-4">
                <input
                  type="text"
                  value={comment}
                  onChange={(e) => setComment(e.target.value)}
                  placeholder="Comment"
                  className="block w-full p-3 sm:p-4 text-sm sm:text-base text-gray-900 border border-gray-300 rounded-lg bg-white dark:bg-gray-700 dark:border-gray-600 dark:placeholder-gray-400 dark:text-white focus:ring-blue-500 focus:border-blue-500"
                />
              </div>

              <button
                type="submit"
                className="w-full px-5 py-2.5 text-sm sm:text-base text-white bg-blue-700 hover:bg-blue-800 focus:ring-4 focus:ring-blue-300 font-medium rounded-lg dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800"
              >
                Rate
              </button>
            </form>
          </div>
        </div>
      </div>

      {/* Tabs Section (Activity, UTXOs, Config) */}
      <div className="mt-4 sm:mt-6">
        <div className="border-b border-gray-200 dark:border-gray-700">
          <nav className="-mb-px flex space-x-4 sm:space-x-8 overflow-x-auto">
            <button
              onClick={() => setActiveTab('activity')}
              className={`border-b-2 py-3 sm:py-4 px-1 text-xs sm:text-sm font-medium whitespace-nowrap ${
                activeTab === 'activity'
                  ? 'border-blue-500 text-blue-600 dark:text-blue-400'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 dark:text-gray-400 dark:hover:text-gray-300'
              }`}
            >
              Activity
            </button>
            {hasWalletV1 && (
              <button
                onClick={() => {
                  setActiveTab('utxos');
                  setUtxosRequested(true);
                }}
                className={`border-b-2 py-3 sm:py-4 px-1 text-xs sm:text-sm font-medium whitespace-nowrap ${
                  activeTab === 'utxos'
                    ? 'border-blue-500 text-blue-600 dark:text-blue-400'
                    : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 dark:text-gray-400 dark:hover:text-gray-300'
                }`}
              >
                UTXOs
              </button>
            )}
            {hasWalletV2 && (
              <button
                onClick={() => { setActiveTab('assets'); setAssetsOpened(true); }}
                className={`border-b-2 py-3 sm:py-4 px-1 text-xs sm:text-sm font-medium whitespace-nowrap ${
                  activeTab === 'assets'
                    ? 'border-blue-500 text-blue-600 dark:text-blue-400'
                    : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 dark:text-gray-400 dark:hover:text-gray-300'
                }`}
              >
                Wallet Assets
              </button>
            )}
            <button
              onClick={() => setActiveTab('config')}
              className={`border-b-2 py-3 sm:py-4 px-1 text-xs sm:text-sm font-medium whitespace-nowrap ${
                activeTab === 'config'
                  ? 'border-blue-500 text-blue-600 dark:text-blue-400'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 dark:text-gray-400 dark:hover:text-gray-300'
              }`}
            >
              Config
            </button>
          </nav>
        </div>

        <div className="mt-4 sm:mt-6">
          {activeTab === 'activity' && (
            <>
              <div className="bg-blue-900/20 border border-blue-500 rounded-lg p-3 sm:p-4 mb-4 sm:mb-6">
                <span className="text-xs sm:text-sm text-blue-400 font-semibold">Info:</span>
                <span className="text-xs sm:text-sm text-blue-400 ml-2">
                  Some transaction types, like Lightning transactions, cause more than one Fedimint transaction.
                </span>
              </div>

              <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:p-6">
                <div className="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-4 mb-4 sm:mb-6">
                  <div className="flex-1 min-w-0">
                    <h3 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white wrap-break-word">
                      {histogramError || (histogramLoading && histogram.length === 0) ? '—' : chartMetric === 'volume'
                        ? histogramTotals.volume.toFixed(6) + ' BTC'
                        : histogramTotals.count.toString()}
                    </h3>
                    <p className="text-xs sm:text-sm text-gray-500 dark:text-gray-400">
                      {histogramError ? 'Unavailable: activity could not be loaded' : chartMetric === 'volume' ? 'Total Volume' : 'Total Transactions'}
                    </p>
                  </div>
                  <div className="flex flex-col sm:flex-row items-start sm:items-center gap-2 sm:gap-4 shrink-0 relative z-10">
                    <div className="relative">
                      <select
                        className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white text-xs sm:text-sm min-w-[140px] appearance-none cursor-pointer"
                        value={movingAverageWindow}
                        onChange={(e) => setMovingAverageWindow(Number(e.target.value) as HistogramAverage)}
                        aria-label="Moving average"
                      >
                        <option value="0">No Average</option>
                        <option value="7">7-Day Avg</option>
                        <option value="30">30-Day Avg</option>
                      </select>
                    </div>
                    {chartMetric === 'count' && (
                      <label
                        className="flex items-center text-xs sm:text-sm text-gray-600 dark:text-gray-400 cursor-pointer whitespace-nowrap"
                        title="Use logarithmic scale (base 10) for Y-axis. Zeros are replaced with a small value."
                      >
                        <input
                          type="checkbox"
                          className="mr-2 w-4 h-4 text-blue-600 bg-gray-100 border-gray-300 rounded-sm focus:ring-blue-500 shrink-0"
                          checked={useLogScale}
                          onChange={(e) => setUseLogScale(e.target.checked)}
                        />
                        <span className="hidden sm:inline">Log Scale</span>
                        <span className="sm:hidden">Log</span>
                      </label>
                    )}
                    {chartMetric === 'volume' && (
                      <label
                        className="flex items-center text-xs sm:text-sm text-gray-600 dark:text-gray-400 cursor-pointer whitespace-nowrap"
                        title="Filter out values that are more than 10 times the 95th percentile"
                      >
                        <input
                          type="checkbox"
                          className="mr-2 w-4 h-4 text-blue-600 bg-gray-100 border-gray-300 rounded-sm focus:ring-blue-500 shrink-0"
                          checked={filterOutliers}
                          onChange={(e) => setFilterOutliers(e.target.checked)}
                        />
                        <span className="hidden sm:inline">Filter Extreme Outliers</span>
                        <span className="sm:hidden">Filter Outliers</span>
                      </label>
                    )}
                    <div className="relative">
                      <select
                        className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white text-xs sm:text-sm min-w-[140px] appearance-none cursor-pointer"
                        value={chartMetric}
                        onChange={(e) => setChartMetric(e.target.value as 'volume' | 'count')}
                        aria-label="Chart metric"
                      >
                        <option value="volume">Volume</option>
                        <option value="count">Transactions</option>
                      </select>
                     
                    </div>
                  </div>
                </div>

                {histogramLoading && histogram.length === 0 ? (
                  <div className="text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 py-12">
                    Loading chart data...
                  </div>
                ) : histogramError ? (
                  <div className="py-12 text-center text-xs text-red-700 dark:text-red-400 sm:text-sm" role="alert">
                    <p>Could not load the activity data: {histogramError}</p>
                    <button
                      type="button"
                      onClick={() => setHistogramReload((count) => count + 1)}
                      className="mt-3 rounded-lg border border-gray-300 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-700"
                    >
                      Try again
                    </button>
                  </div>
                ) : histogram.length > 0 ? (
                  <div className={`mt-4 -mx-4 sm:mx-0 transition-opacity ${histogramLoading ? 'opacity-60' : ''}`} aria-busy={histogramLoading}>
                    <h4 className="text-center text-sm sm:text-base font-normal text-gray-500 dark:text-gray-400 mb-4 px-4 sm:px-0">
                      Daily {chartMetric === 'volume' ? 'Volume' : 'Transactions'}{useLogScale && chartMetric === 'count' ? ' (Log Scale)' : ''}
                      <span className="block text-xs text-gray-400 dark:text-gray-500">Days in UTC</span>
                    </h4>
                    <div className="w-full px-4 sm:px-0">
                      <Suspense fallback={
                        <div className="flex items-center justify-center h-[400px] text-gray-500 dark:text-gray-400">
                          Loading chart...
                        </div>
                      }>
                        <TransactionChart
                          data={histogram}
                          chartMetric={chartMetric}
                          movingAverageWindow={movingAverageWindow}
                          useLogScale={useLogScale}
                          zoomStart={zoomState.start}
                          zoomEnd={zoomState.end}
                          onZoomChange={(start, end) => setZoomState({ start, end })}
                        />
                      </Suspense>
                    </div>
                  </div>
                ) : (
                  <div className="text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 py-12">
                    No activity data available
                  </div>
                )}
              </div>
            </>
          )}
          {assetsOpened && hasWalletV2 && (
            <div hidden={activeTab !== 'assets'}>
              <WalletV2Panel id={id!} modules={config?.raw.modules ?? {}} active={activeTab === 'assets'} />
            </div>
          )}
          {activeTab === 'utxos' && (
            <>
              <Alert
                level="info"
                message="This view compares reconstructed observer history with guardian wallet data. Results may change while history is catching up."
              />

              <section className="mt-5 overflow-hidden border border-gray-200 bg-white dark:border-gray-700 dark:bg-gray-800">
                <div className="flex flex-wrap items-end justify-between gap-3 border-b border-gray-200 px-4 py-4 dark:border-gray-700 sm:px-6">
                  <div>
                    <h2 className="text-base font-semibold text-gray-950 dark:text-white">Wallet UTXO Inventory</h2>
                    <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
                      One row per outpoint. Guardian agreement is shown alongside the reconstructed observer record.
                    </p>
                  </div>
                  {utxoData && (
                    <div className="text-xs text-gray-500 dark:text-gray-400">
                      {successfulGuardianCount} of {utxoGuardians.length} guardians compared
                    </div>
                  )}
                </div>

                {utxosError && (
                  <div className="flex flex-wrap items-center gap-x-2 gap-y-1 border-b border-red-200 bg-red-50 px-4 py-3 text-xs text-red-800 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-300 sm:px-6" role="alert">
                    <span><span className="font-bold">Error: </span>Could not load the UTXOs: {utxosError.message}</span>
                    {utxosError.retryable && (
                      <button type="button" onClick={() => setUtxosReload((count) => count + 1)} className="font-medium underline hover:no-underline">
                        Try again
                      </button>
                    )}
                  </div>
                )}

                {overviewFailed && !utxosError && (
                  <div className="border-b border-gray-200 px-4 py-3 text-xs text-gray-600 dark:border-gray-700 dark:text-gray-300 sm:px-6">
                    Could not check how far the observer's wallet history is indexed, so outputs from recent sessions may be missing.
                  </div>
                )}

                {guardianSummaryUnavailable && (
                  <div className="border-b border-amber-200 bg-amber-50 px-4 py-3 text-xs text-amber-900 dark:border-amber-900/60 dark:bg-amber-950/40 dark:text-amber-200 sm:px-6">
                    Guardian wallet data is unavailable right now, so outputs are only checked for having been spent on-chain.
                  </div>
                )}

                {tooFewGuardians && (
                  <div className="border-b border-amber-200 bg-amber-50 px-4 py-3 text-xs text-amber-900 dark:border-amber-900/60 dark:bg-amber-950/40 dark:text-amber-200 sm:px-6">
                    Only {successfulGuardianCount} of {utxoGuardians.length} guardians responded, but {utxoThreshold} must agree, so outputs cannot be verified right now.
                  </div>
                )}

                {guardianFindings.length > 0 && (
                  <div className="border-b border-rose-200 bg-rose-50 px-4 py-3 text-xs text-rose-900 dark:border-rose-900/60 dark:bg-rose-950/40 dark:text-rose-200 sm:px-6">
                    {guardianFindings.join(' ')}
                  </div>
                )}

                {walletBehind && (
                  <div className="border-b border-gray-200 px-4 py-3 text-xs text-gray-600 dark:border-gray-700 dark:text-gray-300 sm:px-6">
                    The observer's wallet history is indexed to session {walletSessions} of {overview.session_count}, so outputs from later sessions are not listed yet.
                  </div>
                )}

                {laggingGuardians.length > 0 && (
                  <div className="border-b border-gray-200 px-4 py-3 text-xs text-gray-600 dark:border-gray-700 dark:text-gray-300 sm:px-6">
                    Left out of the comparison because they are behind the federation:{' '}
                    {laggingGuardians.map((guardian) => `guardian ${guardian.guardian_id} (session ${guardian.session_count})`).join(', ')}.
                  </div>
                )}
                <div className="flex flex-col gap-3 border-b border-gray-200 px-4 py-3 dark:border-gray-700 sm:px-6 lg:flex-row lg:items-center lg:justify-between">
                  <div className="flex flex-wrap gap-2" role="group" aria-label="Filter UTXOs by status">
                    {(['all', 'verified', 'pending', 'mismatch'] as const).map((view) => {
                      const active = utxoView === view;
                      const alert = view === 'mismatch' && (utxoCounts?.mismatch ?? 0) > 0;
                      return (
                        <button
                          key={view}
                          type="button"
                          onClick={() => {
                            setUtxoView(view);
                            setUtxoPage(1);
                          }}
                          aria-pressed={active}
                          className={`inline-flex items-center gap-1.5 whitespace-nowrap rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
                            active
                              ? 'border-blue-600 bg-blue-600 text-white'
                              : alert
                                ? 'border-rose-300 text-rose-700 hover:bg-rose-50 dark:border-rose-800 dark:text-rose-300 dark:hover:bg-rose-950/40'
                                : 'border-gray-300 text-gray-600 hover:bg-gray-50 dark:border-gray-600 dark:text-gray-300 dark:hover:bg-gray-700/50'
                          }`}
                        >
                          <span>{view === 'all' ? 'All' : UTXO_KIND_STYLES[view].label}</span>
                          {utxoCounts && <span className={`tabular-nums ${active ? 'text-white/80' : 'text-gray-400 dark:text-gray-500'}`}>{utxoCounts[view]}</span>}
                        </button>
                      );
                    })}
                  </div>
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
                    <input
                      type="search"
                      value={utxoSearch}
                      onChange={(event) => setUtxoSearch(event.target.value)}
                      aria-label="Search UTXOs"
                      placeholder="Search outpoint, txid, address or BTC amount"
                      className="w-full rounded-lg border border-gray-300 bg-white px-3 py-1.5 text-sm text-gray-900 outline-hidden focus:ring-2 focus:ring-blue-500 dark:border-gray-600 dark:bg-gray-900 dark:text-white sm:w-80"
                    />
                    <div className="whitespace-nowrap text-xs text-gray-500 dark:text-gray-400">
                      {!utxoData
                        ? ''
                        : utxoTotalRows === 0
                          ? '0 UTXOs'
                          : `${utxoPageStart + 1}–${utxoPageStart + visibleUtxos.length} of ${utxoTotalRows}`}
                    </div>
                  </div>
                </div>

                <table className="w-full table-fixed text-left text-sm">
                  <colgroup>
                    <col />
                    <col className="hidden w-32 sm:table-column" />
                    <col className="hidden w-28 sm:table-column" />
                    <col className="w-36 sm:w-40" />
                  </colgroup>
                  <thead className="border-b border-gray-200 bg-gray-50 text-[11px] font-medium uppercase tracking-wide text-gray-500 dark:border-gray-700 dark:bg-gray-900/40 dark:text-gray-400">
                    <tr>
                      <th scope="col" className="px-4 py-2 sm:px-6">Outpoint</th>
                      <th scope="col" className="hidden px-2 py-2 sm:table-cell">Guardians</th>
                      <th scope="col" className="hidden px-2 py-2 sm:table-cell">Status</th>
                      <th scope="col" className="px-4 py-2 text-right sm:px-6">Amount</th>
                    </tr>
                  </thead>
                  <tbody aria-busy={utxosLoading} className={`divide-y divide-gray-100 transition-opacity dark:divide-gray-700/70 ${utxosLoading && utxoData ? 'opacity-60' : ''}`}>
                    {utxosLoading && !utxoData ? (
                      <tr>
                        <td colSpan={4} className="px-4 py-6 text-center text-sm text-gray-500 dark:text-gray-400">Loading UTXOs...</td>
                      </tr>
                    ) : utxosError ? (
                      <tr>
                        <td colSpan={4} className="px-4 py-6 text-center text-sm text-gray-500 dark:text-gray-400">UTXOs unavailable</td>
                      </tr>
                    ) : visibleUtxos.length === 0 ? (
                      <tr>
                        <td colSpan={4} className="px-4 py-6 text-center text-sm text-gray-500 dark:text-gray-400">
                          {utxoQuery ? `No UTXOs match "${utxoQuery}"` : 'No UTXOs found'}
                        </td>
                      </tr>
                    ) : (
                      visibleUtxos.map((utxo) => {
                        const style = UTXO_KIND_STYLES[utxo.kind];
                        const heldBy = comparedGuardianIds.filter((guardianId) => utxo.guardianStates.has(guardianId)).length;
                        return (
                          <tr
                            key={utxo.outPoint}
                            className={`align-top transition-colors hover:bg-gray-50 dark:hover:bg-gray-700/40 ${utxo.kind === 'mismatch' ? 'bg-rose-50/50 dark:bg-rose-950/10' : ''}`}
                          >
                            <td className="px-4 py-2.5 sm:px-6">
                              <ExplorerLink
                                href={explorerTxUrl(utxo.outPoint, network)}
                                className="block truncate font-mono text-xs text-blue-700 hover:underline dark:text-blue-400"
                                title={utxo.outPoint}
                              >
                                {shortOutpoint(utxo.outPoint)}
                              </ExplorerLink>
                              {utxo.address && (
                                <ExplorerLink
                                  href={explorerAddressUrl(utxo.address, network)}
                                  className="mt-0.5 block truncate font-mono text-[11px] text-gray-500 hover:underline dark:text-gray-400"
                                  title={utxo.address}
                                >
                                  {shortMiddle(utxo.address, 10, 8)}
                                </ExplorerLink>
                              )}
                              {utxo.detail && <p className={`mt-1 text-xs ${style.detail}`}>{linkTxids(utxo.detail, network)}</p>}
                            </td>
                            <td className="hidden px-2 py-2.5 sm:table-cell">
                              {comparedGuardianIds.length > 0 && (
                                <div className="flex items-center gap-2" aria-label={`Listed by ${heldBy} of ${comparedGuardianIds.length} guardians`}>
                                  <div className="flex gap-1">
                                    {comparedGuardianIds.map((guardianId) => {
                                      const state = utxo.guardianStates.get(guardianId);
                                      const dot = !state
                                        ? 'border border-gray-400 dark:border-gray-500'
                                        : state === 'spendable' || state === 'unconfirmed_change'
                                          ? 'bg-emerald-500'
                                          : 'bg-amber-400';
                                      return (
                                        <span
                                          key={guardianId}
                                          title={`Guardian ${guardianId}: ${state ? state.replaceAll('_', ' ') : 'not listed'}`}
                                          className={`h-2.5 w-2.5 rounded-full ${dot}`}
                                        />
                                      );
                                    })}
                                  </div>
                                  <span className="text-xs tabular-nums text-gray-500 dark:text-gray-400">{heldBy}/{comparedGuardianIds.length}</span>
                                </div>
                              )}
                            </td>
                            <td className="hidden px-2 py-2.5 sm:table-cell">
                              <span className={`inline-block rounded-full px-2 py-0.5 text-xs font-medium ${style.pill}`}>{style.label}</span>
                            </td>
                            <td className="px-4 py-2.5 text-right sm:px-6">
                              <div className="whitespace-nowrap font-mono text-xs tabular-nums text-gray-900 dark:text-white">
                                {asBitcoin(utxo.amount, 8)}
                              </div>
                              <span className={`mt-1 inline-block rounded-full px-2 py-0.5 text-[11px] font-medium sm:hidden ${style.pill}`}>{style.label}</span>
                            </td>
                          </tr>
                        );
                      })
                    )}
                  </tbody>
                </table>

                {utxoPageCount > 1 && (
                  <nav
                    aria-label="UTXO pages"
                    className="flex flex-wrap items-center justify-between gap-2 border-t border-gray-200 px-4 py-3 dark:border-gray-700 sm:px-6"
                  >
                    <button
                      type="button"
                      onClick={() => setUtxoPage(currentUtxoPage - 1)}
                      disabled={currentUtxoPage === 1}
                      className="rounded-md border border-gray-300 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-40 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-700"
                    >
                      ← Previous
                    </button>
                    <div className="flex flex-wrap items-center gap-1">
                      {utxoPageItems(currentUtxoPage, utxoPageCount).map((item, index) =>
                        item === 'gap' ? (
                          <span key={`gap-${index}`} className="px-1 text-xs text-gray-400">…</span>
                        ) : (
                          <button
                            key={item}
                            type="button"
                            onClick={() => setUtxoPage(item)}
                            aria-current={item === currentUtxoPage ? 'page' : undefined}
                            className={`min-w-8 rounded-md px-2 py-1.5 text-xs font-medium ${item === currentUtxoPage ? 'bg-blue-600 text-white' : 'text-gray-700 hover:bg-gray-100 dark:text-gray-200 dark:hover:bg-gray-700'}`}
                          >
                            {item}
                          </button>
                        )
                      )}
                    </div>
                    <button
                      type="button"
                      onClick={() => setUtxoPage(currentUtxoPage + 1)}
                      disabled={currentUtxoPage === utxoPageCount}
                      className="rounded-md border border-gray-300 px-3 py-1.5 text-xs font-medium text-gray-700 hover:bg-gray-50 disabled:cursor-not-allowed disabled:opacity-40 dark:border-gray-600 dark:text-gray-200 dark:hover:bg-gray-700"
                    >
                      Next →
                    </button>
                  </nav>
                )}
              </section>
            </>
          )}

          {activeTab === 'config' && (
            <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-3 sm:p-6 overflow-hidden">
              <pre className="text-[10px] sm:text-xs lg:text-sm text-gray-900 dark:text-white overflow-x-auto whitespace-pre wrap-break-word">
                {config?.raw ? JSON.stringify(config.raw, null, 2) : 'Loading config...'}
              </pre>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function pluralOutputs(count: number): string {
  return `${count} ${count === 1 ? 'output' : 'outputs'}`;
}

// `txid:vout` with the txid shortened in the middle; the full value is in the title
function shortOutpoint(outPoint: string): string {
  const [txid, vout] = outPoint.split(':');
  return vout === undefined ? shortMiddle(outPoint, 12, 12) : `${shortMiddle(txid, 12, 12)}:${vout}`;
}

// A link to the block explorer, or plain text on a network it has no pages for
function ExplorerLink({ href, className, title, children }: { href: string | null; className: string; title?: string; children: React.ReactNode }) {
  return href ? (
    <a href={href} target="_blank" rel="noopener noreferrer" className={className} title={title}>{children}</a>
  ) : (
    <span className={className.replace(/\S*hover:\S+/g, '')} title={title}>{children}</span>
  );
}

// Shortens transaction ids in backend descriptions and links them to the explorer
function linkTxids(text: string, network: string | undefined) {
  return text.split(/\b([0-9a-f]{64})\b/).map((part, index) =>
    index % 2 === 1 ? (
      <ExplorerLink
        key={index}
        href={explorerTxUrl(part, network)}
        className="font-mono underline hover:no-underline"
        title={part}
      >
        {shortMiddle(part, 8, 8)}
      </ExplorerLink>
    ) : (
      part
    )
  );
}
