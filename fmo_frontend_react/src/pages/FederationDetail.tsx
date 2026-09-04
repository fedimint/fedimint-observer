import { useEffect, useState, useMemo, lazy, Suspense } from 'react';
import { useParams, Link } from 'react-router-dom';
import { QRCodeSVG } from 'qrcode.react';
import { api } from '../services/api';
import type { FederationSummary } from '../types/api';
import { Badge } from '../components/Badge';
import { Alert } from '../components/Alert';
import { Copyable } from '../components/Copyable';

// Lazy load the chart component for code splitting
const TransactionChart = lazy(() => import('../components/TransactionChart').then(module => ({ default: module.TransactionChart })));

interface Guardian {
  id: number;
  name: string;
  url: string;
  online: boolean;
  session: number;
  block: number;
  sessionOutdated: boolean;
  blockOutdated: boolean;
}

interface GuardianHealth {
  avg_uptime: number;
  avg_latency: number;
  latest: {
    block_height: number;
    block_outdated: boolean;
    session_count: number;
    session_outdated: boolean;
  } | null;
}

interface FederationConfig {
  guardians: Guardian[];
  modules: string[];
  network: string;
  confirmations_required: number;
  rawConfig: Record<string, unknown>; // Store raw config for display
}

interface UTXO {
  out_point: string;
  amount: number; // millisats
  address: string;
}

interface HistogramEntry {
  date: string;
  volume: number;
  count: number;
  avgVolume?: number;
  avgCount?: number;
  isoDate?: string;
  timestamp?: number;
}

export function FederationDetail() {
  const { id } = useParams<{ id: string }>();
  const [federation, setFederation] = useState<FederationSummary | null>(null);
  const [config, setConfig] = useState<FederationConfig | null>(null);
  const [utxos, setUtxos] = useState<UTXO[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<'activity' | 'utxos' | 'config'>('activity');
  const [utxosLoading, setUtxosLoading] = useState(false);
  const [rating, setRating] = useState(5);
  const [comment, setComment] = useState('');
  const [ratingError, setRatingError] = useState<string | null>(null);
  const [ratingSuccess, setRatingSuccess] = useState(false);
  const [histogram, setHistogram] = useState<HistogramEntry[]>([]);
  const [histogramLoading, setHistogramLoading] = useState(false);
  const [chartMetric, setChartMetric] = useState<'volume' | 'count'>('volume');
  const [filterOutliers, setFilterOutliers] = useState(true);
  const [movingAverageWindow, setMovingAverageWindow] = useState<number>(0); // 0 = off, 7 = 7-day, 30 = 30-day
  const [useLogScale, setUseLogScale] = useState(false);
  const [guardianHealth, setGuardianHealth] = useState<Record<string, GuardianHealth>>({});
  
  // Check if at least one guardian is online
  const hasOnlineGuardian = useMemo(() => {
    return Object.values(guardianHealth).some(
      (health) => health?.latest !== null && health?.latest !== undefined
    );
  }, [guardianHealth]);
  
  // Calculate initial zoom to show last 3 months of data by default
  const initialZoom = useMemo(() => {
    if (histogram.length === 0) return { start: 0, end: 100 };
    
    // Use timestamps to compute last 90 days (3 months ~ 90 days)
    const THREE_MONTHS_MS = 90 * 24 * 60 * 60 * 1000;
    const lastTs = histogram[histogram.length - 1].timestamp || 0;
    const cutoffTs = lastTs - THREE_MONTHS_MS;
    
    // Find first index with timestamp >= cutoff
    let startIndex = histogram.findIndex((h) => (h.timestamp ?? 0) >= cutoffTs);
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

    // Fetch all federations and find the one matching the ID
    api.getFederations()
      .then((federations) => {
        const fed = federations.find((f) => f.id === id);
        if (!fed) {
          throw new Error('Federation not found');
        }
        setFederation(fed);
        // Fetch config using federation ID first, fallback to invite if needed
        return fetchFederationConfig(id, fed.invite);
      })
      .then((configData) => {
        setConfig(configData);
        setLoading(false);
        // Fetch UTXOs, histogram, and guardian health in background
        fetchUTXOs(id);
        fetchHistogram(id);
        fetchGuardianHealth(id);
      })
      .catch((err) => {
        console.error('Failed to fetch federation details:', err);
        setError(err.message);
        setLoading(false);
      });
  }, [id]);

  // Update zoom state when histogram data loads
  useEffect(() => {
    if (histogram.length > 0) {
      setZoomState(initialZoom);
    }
  }, [histogram.length, initialZoom]);

  // Remove outliers (values > 10 * 95th percentile)
  const removeOutliers = (data: HistogramEntry[]): HistogramEntry[] => {
    if (data.length === 0) return data;
    const values = data.map(d => d.volume).sort((a, b) => a - b);
    const percentile95Index = Math.floor(values.length * 0.95);
    const percentile95 = values[percentile95Index];
    const threshold = percentile95 * 10;
    return data.filter(d => d.volume < threshold);
  };

  // Calculate moving average with configurable window size using O(n) sliding window
  const calculateMovingAverage = (data: HistogramEntry[], windowSize = 7) => {
    if (data.length === 0) return data;
    
    let volumeSum = 0;
    let countSum = 0;
    const result: HistogramEntry[] = [];
    
    for (let i = 0; i < data.length; i++) {
      // Add current value to the window
      volumeSum += data[i].volume;
      countSum += data[i].count;
      
      // Remove the value that falls out of the window (if window is full)
      if (i >= windowSize) {
        volumeSum -= data[i - windowSize].volume;
        countSum -= data[i - windowSize].count;
      }
      
      // Calculate average based on actual window size
      const actualWindowSize = Math.min(i + 1, windowSize);
      const avgVolume = volumeSum / actualWindowSize;
      const avgCount = countSum / actualWindowSize;
      
      result.push({
        ...data[i],
        avgVolume,
        avgCount,
      });
    }
    
    return result;
  };


  // Memoize the processed chart data to avoid recalculating on every render
  const processedChartData = useMemo(() => {
    let data = chartMetric === 'volume' && filterOutliers ? removeOutliers(histogram) : histogram;
    if (movingAverageWindow > 0) {
      data = calculateMovingAverage(data, movingAverageWindow);
    }
    // Don't apply log scale manually - let ECharts handle it with yAxis type: 'log'
    return data;
  }, [histogram, chartMetric, filterOutliers, movingAverageWindow]);

  const fetchGuardianHealth = async (federationId: string) => {
    try {
  const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';
      const response = await fetch(`${BASE_URL}/federations/${federationId}/health`);
      if (response.ok) {
        const data = await response.json();
        setGuardianHealth(data);
      }
    } catch (err) {
      console.error('Failed to fetch guardian health:', err);
    }
  };

  const fetchHistogram = async (federationId: string) => {
    setHistogramLoading(true);
    try {
  const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';
      const response = await fetch(`${BASE_URL}/federations/${federationId}/transactions/histogram`);
      if (response.ok) {
        const data = await response.json() as Record<string, { num_transactions: number; amount_transferred: number }>;
        // Data comes as: { "2024-05-31": { "num_transactions": 1, "amount_transferred": 2000000000 }, ... }
        const chartData = Object.entries(data).map(([dateStr, stats]) => {
          const date = new Date(dateStr);
          return {
            date: date.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' }),
            isoDate: dateStr,
            timestamp: date.getTime(),
            volume: stats.amount_transferred / 100000000000, // millisats to BTC
            count: stats.num_transactions,
          };
        });
        setHistogram(chartData);
      }
    } catch (error) {
      console.error('Failed to fetch histogram:', error);
    } finally {
      setHistogramLoading(false);
    }
  };

  const fetchUTXOs = async (federationId: string) => {
    setUtxosLoading(true);
    try {
  const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';
      const response = await fetch(`${BASE_URL}/federations/${federationId}/utxos`);
      if (response.ok) {
        const data = await response.json();
        setUtxos(data);
      }
    } catch (err) {
      console.error('Failed to fetch UTXOs:', err);
    } finally {
      setUtxosLoading(false);
    }
  };

  const handleRateSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setRatingError(null);
    setRatingSuccess(false);

    try {
      // Check for Nostr extension
      if (!window.nostr) {
        throw new Error('Nostr extension not found. Please install a Nostr browser extension like nos2x or Alby.');
      }

      const nostr = window.nostr;
      const pubkey = await nostr.getPublicKey();

      if (!id) throw new Error('Federation ID missing');

      // Create unsigned event (Kind 38000 for rating)
      const unsignedEvent = {
        kind: 38000,
        created_at: Math.floor(Date.now() / 1000),
        tags: [
          ['d', id],
          ['n', 'mainnet'],
          ['k', '38173'],
        ],
        content: `[${rating}/5] ${comment}`,
        pubkey,
      };

      // Sign event
      const signedEvent = await nostr.signEvent(unsignedEvent);

      // Publish to backend
  const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';
      const response = await fetch(`${BASE_URL}/federations/nostr/rating`, {
        method: 'PUT',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(signedEvent),
      });

      if (!response.ok) {
        throw new Error(`Failed to publish rating: ${response.status}`);
      }

      setRatingSuccess(true);
      setComment('');
    } catch (err) {
      setRatingError(err instanceof Error ? err.message : 'Failed to publish rating');
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

      <h1 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white mb-6 sm:mb-8 break-words">
        {federation.name || 'Unnamed Federation'}
      </h1>

      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 sm:gap-6">
        {/* Guardians Panel */}
        <div className="lg:col-span-2 bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-4 sm:p-6">
          <h2 className="text-base sm:text-lg font-semibold text-gray-900 dark:text-white mb-3 sm:mb-4">
            Guardians
            {config && (
              <span className="ml-2 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
                {config.guardians.length} of {config.guardians.length} Federation
              </span>
            )}
          </h2>
          {config ? (
            <div className="space-y-3 sm:space-y-4">
              {config.guardians.map((guardian) => {
                const health = guardianHealth[guardian.id.toString()];
                const isLoading = !health;
                const isOnline = health?.latest !== null && health?.latest !== undefined;
                const session = health?.latest?.session_count || 0;
                const block = health?.latest ? health.latest.block_height - 1 : 0;
                const sessionOutdated = health?.latest?.session_outdated || false;
                const blockOutdated = health?.latest?.block_outdated || false;

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
                      ) : (
                        <>
                          <Badge level={isOnline ? 'success' : 'error'}>
                            {isOnline ? 'Online' : 'Offline'}
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
                                Block {block}
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
                  {config?.confirmations_required || 'N/A'}
                </div>
              </div>
              {id && (
                <div>
                  <Link
                    to={`/federations/${id}/gateways`}
                    className="inline-flex items-center justify-center w-full px-4 py-2.5 text-sm font-medium text-white bg-blue-700 hover:bg-blue-800 rounded-lg focus:ring-4 focus:outline-none focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800"
                  >
                    Gateway Details
                  </Link>
                </div>
              )}
              {federation?.invite && hasOnlineGuardian && (
                <div>
                  <div className="text-xs sm:text-sm text-gray-500 dark:text-gray-400 mb-2">Invite Link</div>
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
            <button
              onClick={() => setActiveTab('utxos')}
              className={`border-b-2 py-3 sm:py-4 px-1 text-xs sm:text-sm font-medium whitespace-nowrap ${
                activeTab === 'utxos'
                  ? 'border-blue-500 text-blue-600 dark:text-blue-400'
                  : 'border-transparent text-gray-500 hover:text-gray-700 hover:border-gray-300 dark:text-gray-400 dark:hover:text-gray-300'
              }`}
            >
              UTXOs
            </button>
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
                    <h3 className="text-2xl sm:text-3xl font-bold text-gray-900 dark:text-white break-words">
                      {chartMetric === 'volume'
                        ? histogram.reduce((sum, entry) => sum + entry.volume, 0).toFixed(6) + ' BTC'
                        : histogram.reduce((sum, entry) => sum + entry.count, 0).toString()}
                    </h3>
                    <p className="text-xs sm:text-sm text-gray-500 dark:text-gray-400">
                      {chartMetric === 'volume' ? 'Total Volume' : 'Total Transactions'}
                    </p>
                  </div>
                  <div className="flex flex-col sm:flex-row items-start sm:items-center gap-2 sm:gap-4 shrink-0 relative z-10">
                    <div className="relative">
                      <select
                        className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white text-xs sm:text-sm min-w-[140px] appearance-none cursor-pointer"
                        value={movingAverageWindow}
                        onChange={(e) => setMovingAverageWindow(Number(e.target.value))}
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
                          className="mr-2 w-4 h-4 text-blue-600 bg-gray-100 border-gray-300 rounded focus:ring-blue-500 shrink-0"
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
                          className="mr-2 w-4 h-4 text-blue-600 bg-gray-100 border-gray-300 rounded focus:ring-blue-500 shrink-0"
                          checked={filterOutliers}
                          onChange={(e) => setFilterOutliers(e.target.checked)}
                        />
                        <span className="hidden sm:inline">Filter Extreme Outliers</span>
                        <span className="sm:hidden">Filter Outliers</span>
                      </label>
                    )}
                    <div className="relative">
                      <select
                        className="px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white text-xs sm:text-sm min-w-[140px] appearance-none cursor-pointer debug-select"
                        value={chartMetric}
                        onChange={(e) => setChartMetric(e.target.value as 'volume' | 'count')}
                        aria-label="Chart metric (debug)"
                      >
                        <option value="volume">Volume</option>
                        <option value="count">Transactions</option>
                      </select>
                     
                    </div>
                  </div>
                </div>

                {histogramLoading ? (
                  <div className="text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 py-12">
                    Loading chart data...
                  </div>
                ) : histogram.length > 0 ? (
                  <div className="mt-4 -mx-4 sm:mx-0">
                    <h4 className="text-center text-sm sm:text-base font-normal text-gray-500 dark:text-gray-400 mb-4 px-4 sm:px-0">
                      Daily {chartMetric === 'volume' ? 'Volume' : 'Transactions'}{useLogScale && chartMetric === 'count' ? ' (Log Scale)' : ''}
                    </h4>
                    <div className="w-full px-4 sm:px-0">
                      <Suspense fallback={
                        <div className="flex items-center justify-center h-[400px] text-gray-500 dark:text-gray-400">
                          Loading chart...
                        </div>
                      }>
                        <TransactionChart
                          data={processedChartData}
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

          {activeTab === 'utxos' && (
            <>
              <Alert
                level="info"
                message="The UTXO view is reconstructed from a combination of the public federation log and on-chain transactions, hence unconfirmed change UTXOs may be missing."
              />

              <div className="mt-4 relative shadow-md sm:rounded-lg">
                <div className="bg-gray-100 dark:bg-gray-700 px-3 sm:px-6 py-3 text-xs text-gray-700 dark:text-gray-400 uppercase font-semibold">
                  UTXOs ({utxos.length} total)
                </div>
                <div className="divide-y divide-gray-200 dark:divide-gray-700">
                  {utxosLoading ? (
                    <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                      Loading UTXOs...
                    </div>
                  ) : utxos.length === 0 ? (
                    <div className="px-3 sm:px-6 py-4 text-center text-xs sm:text-sm text-gray-500 dark:text-gray-400 bg-white dark:bg-gray-800">
                      No UTXOs found
                    </div>
                  ) : (
                    utxos.map((utxo, index) => (
                      <div key={index} className="bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700 px-3 sm:px-6 py-3 sm:py-4">
                        <div className="flex flex-col sm:flex-row sm:justify-between sm:items-center gap-2">
                          <div className="flex-1 min-w-0">
                            <span className="text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1">UTXO</span>
                            <a
                              href={`https://mempool.space/address/${utxo.address}`}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="text-blue-600 underline dark:text-blue-500 hover:no-underline font-mono text-[10px] sm:text-xs block truncate"
                              title={utxo.out_point}
                            >
                              {utxo.out_point}
                            </a>
                          </div>
                          <div className="sm:text-right shrink-0">
                            <span className="text-[10px] sm:hidden uppercase text-gray-500 dark:text-gray-400 block mb-1">Amount</span>
                            <span className="text-xs sm:text-sm text-gray-900 dark:text-white font-mono whitespace-nowrap">
                              {(utxo.amount / 100000000000).toFixed(8)} BTC
                            </span>
                          </div>
                        </div>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </>
          )}

          {activeTab === 'config' && (
            <div className="bg-white dark:bg-gray-800 rounded-lg shadow-md border border-gray-200 dark:border-gray-700 p-3 sm:p-6 overflow-hidden">
              <pre className="text-[10px] sm:text-xs lg:text-sm text-gray-900 dark:text-white overflow-x-auto whitespace-pre break-words">
                {config?.rawConfig ? JSON.stringify(config.rawConfig, null, 2) : 'Loading config...'}
              </pre>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

async function fetchFederationConfig(federationId: string, inviteCode: string): Promise<FederationConfig> {
  const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';

  // Try fetching config using federation ID first (works for actively observed federations)
  try {
    const configResponse = await fetch(`${BASE_URL}/federations/${federationId}/config`);
    if (configResponse.ok) {
      const config = await configResponse.json();
      return parseConfig(config);
    }
  } catch {
    console.log('Failed to fetch from /federations/{id}/config, trying invite code fallback');
  }

  // Fallback: fetch config using invite code (works for any federation with valid invite)
  const configResponse = await fetch(`${BASE_URL}/config/${inviteCode}`);
  if (!configResponse.ok) {
    throw new Error(`Failed to fetch federation config: ${configResponse.status}`);
  }
  const config = await configResponse.json() as Record<string, unknown>;
  return parseConfig(config);
}

function parseConfig(config: Record<string, unknown>): FederationConfig {
  // Parse guardians
  const guardians: Guardian[] = (config.global as { api_endpoints?: Record<string, { url?: string }> })?.api_endpoints
    ? Object.entries((config.global as { api_endpoints: Record<string, { url?: string }> }).api_endpoints).map(([id, endpoint]) => ({
        id: parseInt(id),
        name: `Guardian ${id}`,
        url: endpoint.url || 'Unknown',
        online: false, // Will be determined by health endpoint
        session: 0, // Will be updated by health endpoint
        block: 0, // Will be updated by health endpoint
        sessionOutdated: false,
        blockOutdated: false,
      }))
    : [];

  // Parse modules
  const modules: string[] = config.modules
    ? Object.values(config.modules as Record<string, { kind?: string }>).map((mod) => mod.kind || 'unknown')
    : [];

  // Get network from wallet module
  const walletModule = config.modules
    ? Object.values(config.modules as Record<string, { kind?: string; network?: string; finality_delay?: number }>).find((mod) => mod.kind === 'wallet')
    : null;
  const network = walletModule ? (walletModule.network || 'unknown') : 'unknown';

  // Get confirmations required from wallet module (finality_delay + 1)
  const confirmations_required = walletModule
    ? ((walletModule.finality_delay || 0) + 1)
    : 0;

  return {
    guardians,
    modules,
    network,
    confirmations_required,
    rawConfig: config, // Store raw config for display in Config tab
  };
}
