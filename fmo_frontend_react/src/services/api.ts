import type {
  ClientConfig,
  FedimintTotals,
  FederationHistogram,
  FederationOverview,
  WalletV2History,
  GatewayV2Registry,
  FederationPage,
  FederationPageQuery,
  FederationSummary,
  FederationUtxosResponse,
  GatewayListQuery,
  NostrFederationPage,
  NostrGroup,
  GatewayInfo,
  GatewayOverview,
  GatewayWindow,
  GuardianHealth,
  GuardianHealthHistory,
  GuardianHealthWindow,
  FederationLiveHealth,
  ManifoldSeatBindingsReport,
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

  /** One group of the home page's list, searched, ordered and paged by the backend */
  getFederationsPage(query: FederationPageQuery, signal?: AbortSignal) {
    const params = new URLSearchParams({ group: query.group, sort: query.sort, dir: query.dir, limit: String(query.limit) });
    if (query.search.trim()) params.set('search', query.search.trim());
    if (query.after) params.set('after', query.after);
    return request<FederationPage>(`${path`/federations/page`}?${params}`, { signal });
  },

  /** One federation's summary, instead of the whole list */
  getFederationSummary: (id: string, signal?: AbortSignal) =>
    request<FederationSummary>(path`/federations/${id}/summary`, { signal }),

  /** One group of the federations announced on Nostr, one page at a time */
  getNostrFederationsPage(group: NostrGroup, limit: number, after?: string, signal?: AbortSignal) {
    const params = new URLSearchParams({ group, limit: String(limit) });
    if (after) params.set('after', after);
    return request<NostrFederationPage>(`${path`/nostr/federations/page`}?${params}`, { signal });
  },

  // Only works for federations the observer follows; others need the invite code
  getFederationConfig: (id: string, signal?: AbortSignal) =>
    request<ClientConfig>(path`/federations/${id}/config`, { signal }),

  /** Keyed by guardian id */
  getFederationHealth: (id: string, signal?: AbortSignal) =>
    request<Record<string, GuardianHealth>>(path`/federations/${id}/health`, { signal }),

  /** Availability buckets per guardian; historical, from the hourly rollup */
  getGuardianHealthHistory(id: string, window: GuardianHealthWindow, signal?: AbortSignal) {
    const query = new URLSearchParams({ window });
    return request<GuardianHealthHistory>(`${path`/federations/${id}/health/history`}?${query}`, { signal });
  },

  /** Each guardian's latest polls and peer view; live, from the last hour */
  getGuardianLiveHealth: (id: string, signal?: AbortSignal) =>
    request<FederationLiveHealth>(path`/federations/${id}/health/live`, { signal }),

  /** The consensus seat-binding directory, verified by the observer */
  getManifoldSeatBindings: (id: string, signal?: AbortSignal) =>
    request<ManifoldSeatBindingsReport>(path`/federations/${id}/manifold/seat-bindings`, { signal }),

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

  getWalletV2OnChain(id: string, moduleInstanceId: number, limit: number, before?: number, signal?: AbortSignal) {
    const query = new URLSearchParams({ module_instance_id: String(moduleInstanceId), limit: String(limit) });
    if (before !== undefined) query.set('before', String(before));
    return request<WalletV2History>(`${path`/federations/${id}/wallet-v2/history`}?${query}`, { signal });
  },

  getGatewayV2Registry(id: string, moduleInstanceId: number, window: GatewayWindow, limit: number, after?: string, signal?: AbortSignal) {
    const query = new URLSearchParams({ module_instance_id: String(moduleInstanceId), window, limit: String(limit) });
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

  /**
   * The gateways overview: the summary, the trend and one page of gateways,
   * searched, filtered and ordered by the backend
   */
  getFederationGatewayOverview(id: string, window: GatewayWindow, list: GatewayListQuery, signal?: AbortSignal) {
    const query = new URLSearchParams({ window, status: list.status, sort: list.sort, dir: list.dir, limit: String(list.limit) });
    if (list.include) query.set('include', list.include);
    if (list.search.trim()) query.set('search', list.search.trim());
    if (list.after) query.set('after', list.after);
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
};
