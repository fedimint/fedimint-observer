import type {
  ClientConfig,
  FedimintTotals,
  FederationHistogram,
  FederationOverview,
  WalletV2History,
  GatewayV2Registry,
  FederationSummary,
  FederationUtxosResponse,
  GatewayInfo,
  GatewayOverview,
  GatewayWindow,
  GuardianHealth,
  HistogramAverage,
  UtxoView,
} from '../types/api';
import type { SignedNostrEvent } from '../types/nostr';
import { request, requestNoContent } from './http';

const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';

// Path segments come from the URL or user input (invite codes), so they are always
// encoded: a / ? or # would change which endpoint is asked
const path = (strings: TemplateStringsArray, ...segments: (string | number)[]) =>
  BASE_URL + strings.reduce((url, part, i) => url + part + (i < segments.length ? encodeURIComponent(segments[i]) : ''), '');

// Failures reject with an ApiError (see ./http) whose message is fit to show
export const api = {
  getTotals: (signal?: AbortSignal) =>
    request<FedimintTotals>(path`/federations/totals`, { signal }),

  getFederations: (signal?: AbortSignal) =>
    request<FederationSummary[]>(path`/federations`, { signal }),

  getNostrFederations: (signal?: AbortSignal) =>
    request<Record<string, string>>(path`/nostr/federations`, { signal }),

  // Only works for federations the observer follows; others need the invite code
  getFederationConfig: (id: string, signal?: AbortSignal) =>
    request<ClientConfig>(path`/federations/${id}/config`, { signal }),

  /** Keyed by guardian id */
  getFederationHealth: (id: string, signal?: AbortSignal) =>
    request<Record<string, GuardianHealth>>(path`/federations/${id}/health`, { signal }),

  getFederationHistogram(
    id: string,
    options: { average: HistogramAverage; excludeOutliers: boolean },
    signal?: AbortSignal,
  ) {
    const query = new URLSearchParams({
      average: String(options.average),
      outliers: options.excludeOutliers ? 'exclude' : 'include',
    });
    return request<FederationHistogram>(`${path`/federations/${id}/transactions/histogram`}?${query}`, { signal });
  },

  getFederationOverview: (id: string, signal?: AbortSignal) =>
    request<FederationOverview>(path`/federations/${id}`, { signal }),

  getWalletV2OnChain(id: string, moduleInstanceId: number, before?: number, signal?: AbortSignal) {
    const query = new URLSearchParams({ module_instance_id: String(moduleInstanceId), limit: '20' });
    if (before !== undefined) query.set('before', String(before));
    return request<WalletV2History>(`${path`/federations/${id}/wallet-v2/history`}?${query}`, { signal });
  },

  getGatewayV2Registry(id: string, moduleInstanceId: number, window: GatewayWindow, after?: string, signal?: AbortSignal) {
    const query = new URLSearchParams({ module_instance_id: String(moduleInstanceId), window });
    if (after !== undefined) query.set('after', after);
    return request<GatewayV2Registry>(`${path`/federations/${id}/gateways/v2`}?${query}`, { signal });
  },

  // 400 names the bad parameter; 404 means the federation has no Wallet v1 module
  getFederationUtxos(
    id: string,
    options: { view: UtxoView; search: string; page: number; limit: number },
    signal?: AbortSignal,
  ) {
    const query = new URLSearchParams({
      view: options.view,
      search: options.search,
      page: String(options.page),
      limit: String(options.limit),
    });
    return request<FederationUtxosResponse>(`${path`/federations/${id}/utxos`}?${query}`, { signal });
  },

  getFederationGatewayOverview(id: string, window: GatewayWindow, signal?: AbortSignal) {
    const query = new URLSearchParams({ window });
    return request<GatewayOverview>(`${path`/federations/${id}/gateways/overview`}?${query}`, { signal });
  },

  getFederationConfigByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<ClientConfig>(path`/config/${inviteCode}`, { signal }),

  // Meta values are parsed as JSON where possible, so not every value is a string
  getFederationMetaByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<Record<string, unknown>>(path`/config/${inviteCode}/meta`, { signal }),

  getFederationIdByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<string>(path`/config/${inviteCode}/id`, { signal }),

  // 404 without a Lightning v1 module, 502 or 504 when no guardian answers
  getFederationGatewaysByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<GatewayInfo[]>(path`/config/${inviteCode}/gateways`, { signal }),

  // Announces a federation (kind 38173)
  publishNostrFederation: (event: SignedNostrEvent, signal?: AbortSignal) =>
    requestNoContent(path`/nostr/federations`, { method: 'PUT', json: event, signal }),

  // Recommends a federation (kind 38000)
  publishNostrRating: (event: SignedNostrEvent, signal?: AbortSignal) =>
    requestNoContent(path`/federations/nostr/rating`, { method: 'PUT', json: event, signal }),
};
