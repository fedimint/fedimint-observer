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
import { request, requestNoContent, requestShared, share } from './http';
import { GATEWAY_LIST, gatewayViewFrom, storedGatewayWindow } from '../features/gateways/lib/gatewayView';
import { storedPageSize } from '../utils/pageSize';

const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';

const path = (strings: TemplateStringsArray, ...segments: (string | number)[]) =>
  BASE_URL + strings.reduce((url, part, i) => url + part + (i < segments.length ? encodeURIComponent(segments[i]) : ''), '');

const HISTORY_SHARE_MS = 60_000;
const PREFETCH_SHARE_MS = 10_000;

const urls = {
  summary: (id: string) => path`/federations/${id}/summary`,
  config: (id: string) => path`/federations/${id}/config`,
  health: (id: string) => path`/federations/${id}/health`,
  liveHealth: (id: string) => path`/federations/${id}/health/live`,
  healthHistory: (id: string, window: GuardianHealthWindow) => `${path`/federations/${id}/health/history`}?${new URLSearchParams({ window })}`,
  seatBindings: (id: string) => path`/federations/${id}/manifold/seat-bindings`,
  gatewayOverview: (id: string, window: GatewayWindow, list: GatewayListQuery) => {
    const query = new URLSearchParams({ window, status: list.status, sort: list.sort, dir: list.dir, limit: String(list.limit) });
    if (list.include) query.set('include', list.include);
    if (list.search.trim()) query.set('search', list.search.trim());
    if (list.after) query.set('after', list.after);
    return `${path`/federations/${id}/gateways/overview`}?${query}`;
  },
};

const GUARDIAN_TABS = new Set(['', 'guardians', 'reliability']);

/** Starts the requests a federation page makes first, before its code has loaded */
export function prefetchFederationRoute(pathname: string, search = '') {
  const match = /^\/federations\/([^/]+)\/?(.*)$/.exec(pathname);
  if (!match) return;
  const id = decodeURIComponent(match[1]);
  const tab = match[2].replace(/\/$/, '').toLowerCase();
  const prefetched = [urls.summary(id), urls.config(id)];
  if (GUARDIAN_TABS.has(tab)) {
    prefetched.push(urls.health(id), urls.liveHealth(id), urls.seatBindings(id));
    api.getGuardianHealthHistory(id, '7d').catch(() => undefined);
  }
  if (tab === 'gateways') {
    const view = gatewayViewFrom(new URLSearchParams(search), storedGatewayWindow());
    prefetched.push(urls.gatewayOverview(id, view.timeWindow, {
      search: '', status: view.filter, sort: view.sort, dir: view.direction, limit: storedPageSize(GATEWAY_LIST),
    }));
  }
  for (const url of prefetched) share(url, PREFETCH_SHARE_MS).catch(() => undefined);
}

export const api = {
  getTotals: (signal?: AbortSignal) =>
    request<FedimintTotals>(path`/federations/totals`, { signal }),

  getFederations: (signal?: AbortSignal) =>
    request<FederationSummary[]>(path`/federations`, { signal }),

  getNostrFederations: (signal?: AbortSignal) =>
    request<Record<string, string>>(path`/nostr/federations`, { signal }),


  getFederationsPage(query: FederationPageQuery, signal?: AbortSignal) {
    const params = new URLSearchParams({ group: query.group, sort: query.sort, dir: query.dir, limit: String(query.limit) });
    if (query.search.trim()) params.set('search', query.search.trim());
    if (query.after) params.set('after', query.after);
    return request<FederationPage>(`${path`/federations/page`}?${params}`, { signal });
  },


  getFederationSummary: (id: string, signal?: AbortSignal) =>
    request<FederationSummary>(urls.summary(id), { signal }),


  getNostrFederationsPage(group: NostrGroup, limit: number, after?: string, signal?: AbortSignal) {
    const params = new URLSearchParams({ group, limit: String(limit) });
    if (after) params.set('after', after);
    return request<NostrFederationPage>(`${path`/nostr/federations/page`}?${params}`, { signal });
  },

  getFederationConfig: (id: string, signal?: AbortSignal) =>
    request<ClientConfig>(urls.config(id), { signal }),


  getFederationHealth: (id: string, signal?: AbortSignal) =>
    request<Record<string, GuardianHealth>>(urls.health(id), { signal }),


  getGuardianHealthHistory: (id: string, window: GuardianHealthWindow, signal?: AbortSignal) =>
    requestShared<GuardianHealthHistory>(urls.healthHistory(id, window), HISTORY_SHARE_MS, signal),


  getGuardianLiveHealth: (id: string, signal?: AbortSignal) =>
    request<FederationLiveHealth>(urls.liveHealth(id), { signal }),


  getManifoldSeatBindings: (id: string, signal?: AbortSignal) =>
    request<ManifoldSeatBindingsReport>(urls.seatBindings(id), { signal }),

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


  getFederationGatewayOverview: (id: string, window: GatewayWindow, list: GatewayListQuery, signal?: AbortSignal) =>
    request<GatewayOverview>(urls.gatewayOverview(id, window, list), { signal }),

  getFederationConfigByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<ClientConfig>(path`/config/${inviteCode}`, { signal }),

  getFederationMetaByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<Record<string, unknown>>(path`/config/${inviteCode}/meta`, { signal }),

  getFederationIdByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<string>(path`/config/${inviteCode}/id`, { signal }),

  getFederationGatewaysByInvite: (inviteCode: string, signal?: AbortSignal) =>
    request<GatewayInfo[]>(path`/config/${inviteCode}/gateways`, { signal }),

  publishNostrFederation: (event: SignedNostrEvent, signal?: AbortSignal) =>
    requestNoContent(path`/nostr/federations`, { method: 'PUT', json: event, signal }),
};
