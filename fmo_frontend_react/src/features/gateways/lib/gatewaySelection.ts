// Which gateways the page shows, and which banner, given its two sources: the
// observer's recorded history and the federation's own live registry. Pure, so
// every combination of loading, failure and empty data can be tested alone.
import type { GatewayInfo, GatewayWindow } from '../../../types/api';

export type GatewayWarningLevel = 'info' | 'warning' | 'error';

export interface GatewayWarningState {
  level: GatewayWarningLevel;
  title: string;
  message: string;
  detail?: string;
}

interface GatewaySelection {
  gateways: GatewayInfo[];
  warning: GatewayWarningState | null;
  retry: boolean;
}

export type LiveLookup =
  | { status: 'pending' }
  | { status: 'skipped' }
  | { status: 'done'; gateways: GatewayInfo[]; error: string | null };

export interface LoadError {
  window: GatewayWindow;
  message: string;
  network: boolean; // fetch itself failed, as opposed to an error answer from the API
}

interface GatewaySources {
  observed: GatewayInfo[] | null; // recorded history, null until it has loaded once
  observedError: LoadError | null; // the latest request for the selected window failed
  live: LiveLookup; // the federation's own registry, which only adds details
  federationFailed: boolean;
  federationOffline: boolean;
  dataWindow: GatewayWindow;
  /** A search, a status filter or a later page: an empty answer means no match, not no gateways */
  listNarrowed?: boolean;
}

/**
 * The live registry's fresher details over the recorded page of gateways,
 * which keeps its own timings and activity. The live registry lists every
 * gateway, so it only refreshes rows of the page; it stands in for the list
 * only when nothing was recorded at all.
 */
export function mergeGatewayData(observedGateways: GatewayInfo[], liveGateways: GatewayInfo[], fallbackToLive = true): GatewayInfo[] {
  if (observedGateways.length === 0) return fallbackToLive ? liveGateways : observedGateways;
  if (liveGateways.length === 0) return observedGateways;

  const liveById = new Map(liveGateways.map((gateway) => [gateway.gateway_id, gateway] as const));
  return observedGateways.map((observedGateway) => {
    const liveGateway = liveById.get(observedGateway.gateway_id);
    if (!liveGateway) return observedGateway;
    // Everything the observer recorded and computed (timings, activity, uptime,
    // coverage and the online/offline/unknown minutes) comes from the recorded row;
    // the live registry only refreshes the gateway's own details. Copying a subset
    // of the recorded fields left a green strip beside "No samples, Unknown 1d".
    return {
      ...liveGateway,
      ...observedGateway,
      lightning_alias: liveGateway.lightning_alias || observedGateway.lightning_alias,
      api_endpoint: liveGateway.api_endpoint || observedGateway.api_endpoint,
      node_pub_key: liveGateway.node_pub_key || observedGateway.node_pub_key,
      vetted: liveGateway.vetted || observedGateway.vetted,
      raw: liveGateway.raw ?? observedGateway.raw,
      // A recorded row without these keeps the live registry's
      first_seen: observedGateway.first_seen ?? liveGateway.first_seen,
      last_seen: observedGateway.last_seen ?? liveGateway.last_seen,
      activity_7d: observedGateway.activity_7d ?? liveGateway.activity_7d,
      activity_window: observedGateway.activity_window ?? liveGateway.activity_window,
      uptime_window: observedGateway.uptime_window ?? liveGateway.uptime_window,
      metrics_window: observedGateway.metrics_window ?? liveGateway.metrics_window,
    };
  });
}

// Banners are only for problems or empty states; normal pages show none
export function selectGatewayData({
  observed,
  observedError,
  live,
  federationFailed,
  federationOffline,
  dataWindow,
  listNarrowed = false,
}: GatewaySources): GatewaySelection {
  const liveGateways = live.status === 'done' ? live.gateways : [];
  const liveError = live.status === 'done' ? live.error : null;

  if (observed !== null) {
    const gateways = liveGateways.length > 0 ? mergeGatewayData(observed, liveGateways, !listNarrowed) : observed;
    if (observedError) {
      return {
        gateways,
        retry: true,
        warning: {
          level: 'warning',
          title: `Could not load ${observedError.window.toUpperCase()} gateway data`,
          message: dataWindow === observedError.window
            ? 'Showing the last data that loaded.'
            : `Showing ${dataWindow.toUpperCase()} data instead.`,
          detail: observedError.message,
        },
      };
    }
    if (federationFailed) {
      return {
        gateways,
        retry: true,
        warning: {
          level: 'warning',
          title: "Could not load the federation's details",
          message: 'Its name and the live gateway check are missing until they load.',
        },
      };
    }
    if (observed.length > 0 || listNarrowed) return { gateways, warning: null, retry: false };
    if (liveGateways.length > 0) {
      return {
        gateways,
        retry: false,
        warning: {
          level: 'info',
          title: 'No recorded history yet',
          message: "These gateways come from the federation's live registry. Their status history has not been recorded yet.",
        },
      };
    }
    if (live.status === 'pending') {
      return {
        gateways,
        retry: false,
        warning: { level: 'info', title: 'Checking for gateways', message: "Looking up the federation's gateway registry…" },
      };
    }
    if (liveError) {
      return {
        gateways,
        retry: true,
        warning: {
          level: 'warning',
          title: "Could not check the federation's gateway registry",
          message: 'No gateways have been recorded for this federation, and its own registry could not be reached.',
          detail: liveError,
        },
      };
    }
    return {
      gateways,
      retry: false,
      warning: federationOffline
        ? { level: 'info', title: 'Federation offline', message: 'This federation is offline and no gateways have been recorded for it.' }
        : { level: 'info', title: 'No gateways', message: 'No gateways have been recorded for this federation.' },
    };
  }

  // Registry-only rows stand in for the history only once loading it has failed
  if (observedError && liveGateways.length > 0) {
    return {
      gateways: liveGateways,
      retry: true,
      warning: {
        level: 'warning',
        title: 'Gateway history unavailable',
        message: "Showing gateways from the federation's live registry only.",
        detail: observedError?.message,
      },
    };
  }

  return {
    gateways: [],
    retry: true,
    warning: {
      level: 'error',
      title: 'Could not load gateway data',
      message: observedError?.network
        ? 'Check your connection and try again.'
        : 'The observer API returned an error. Try again in a moment.',
      detail: liveError ? `${observedError?.message} · Live registry: ${liveError}` : observedError?.message,
    },
  };
}
