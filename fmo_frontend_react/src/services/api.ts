import type {
  FedimintTotals,
  FederationSummary,
  FederationUtxosResponse,
  GatewayInfo,
  GatewayUptimeTrendPoint,
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

  async getFederationUtxos(id: string): Promise<FederationUtxosResponse> {
    const response = await fetch(`${BASE_URL}/federations/${id}/utxos`);
    if (!response.ok) {
      throw new Error(`Failed to fetch UTXOs for federation ${id} (${response.status})`);
    }
    return response.json();
  },

  async getFederationGateways(id: string, window?: GatewayWindow): Promise<GatewayInfo[]> {
    const query = window ? `?window=${encodeURIComponent(window)}` : '';
    const response = await fetch(`${BASE_URL}/federations/${id}/gateways${query}`);
    if (!response.ok) {
      throw new Error(`Failed to fetch gateways for federation ${id} (${response.status})`);
    }
    return response.json();
  },

  async getFederationGatewayUptimeTrend(
    id: string,
    window: GatewayWindow,
  ): Promise<GatewayUptimeTrendPoint[]> {
    const response = await fetch(
      `${BASE_URL}/federations/${id}/gateways/uptime-trend?window=${encodeURIComponent(window)}`,
    );
    if (!response.ok) {
      throw new Error(`Failed to fetch gateway uptime trend (${response.status})`);
    }
    return response.json();
  },

  async getFederationGatewaysByInvite(inviteCode: string): Promise<GatewayInfo[]> {
    const encodedInvite = encodeURIComponent(inviteCode);
    const response = await fetch(`${BASE_URL}/config/${encodedInvite}/gateways`);
    if (!response.ok) {
      throw new Error(`Failed to fetch gateways by invite (${response.status})`);
    }
    return response.json();
  },
};
