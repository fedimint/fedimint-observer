import type {
  FedimintTotals,
  FederationSummary,
  GatewayInfo,
  GatewayOverview,
  GatewayWindow,
} from '../types/api';

const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';

export const api = {
  async getTotals(): Promise<FedimintTotals> {
    const response = await fetch(`${BASE_URL}/federations/totals`);
    if (!response.ok) {
      throw new Error('Failed to fetch totals');
    }
    return response.json();
  },

  async getFederations(): Promise<FederationSummary[]> {
    const response = await fetch(`${BASE_URL}/federations`);
    if (!response.ok) {
      throw new Error('Failed to fetch federations');
    }
    return response.json();
  },

  async getNostrFederations(): Promise<Record<string, string>> {
    const response = await fetch(`${BASE_URL}/nostr/federations`);
    if (!response.ok) {
      throw new Error('Failed to fetch nostr federations');
    }
    return response.json();
  },

  async getFederationGatewayOverview(id: string, window: GatewayWindow, signal?: AbortSignal): Promise<GatewayOverview> {
    const response = await fetch(`${BASE_URL}/federations/${id}/gateways/overview?window=${encodeURIComponent(window)}`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch gateways (${response.status})`);
    }
    return response.json();
  },

  async getFederationGatewaysByInvite(inviteCode: string, signal?: AbortSignal): Promise<GatewayInfo[]> {
    const encodedInvite = encodeURIComponent(inviteCode);
    const response = await fetch(`${BASE_URL}/config/${encodedInvite}/gateways`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch gateways by invite (${response.status})`);
    }
    return response.json();
  },
};
