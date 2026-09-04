import type {
  FedimintTotals,
  FederationSummary,
  GatewayInfo,
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

  async getFederation(id: string): Promise<FederationSummary> {
    const response = await fetch(`${BASE_URL}/federations/${id}`);
    if (!response.ok) {
      throw new Error(`Failed to fetch federation ${id}`);
    }
    // The backend returns overview data, but we need full summary
    // So we still fetch from the list and find it
    // TODO: Backend should provide a full federation detail endpoint
    const allFederations = await this.getFederations();
    const federation = allFederations.find(f => f.id === id);
    if (!federation) {
      throw new Error(`Federation ${id} not found`);
    }
    return federation;
  },

  async getFederationConfig(id: string): Promise<Record<string, unknown>> {
    const response = await fetch(`${BASE_URL}/federations/${id}/config`);
    if (!response.ok) {
      throw new Error(`Failed to fetch config for federation ${id}`);
    }
    return response.json();
  },

  async getFederationUtxos(id: string): Promise<unknown[]> {
    const response = await fetch(`${BASE_URL}/federations/${id}/utxos`);
    if (!response.ok) {
      throw new Error(`Failed to fetch UTXOs for federation ${id}`);
    }
    return response.json();
  },

  async getFederationHistogram(id: string): Promise<Record<string, { num_transactions: number; amount_transferred: number }>> {
    const response = await fetch(`${BASE_URL}/federations/${id}/transactions/histogram`);
    if (!response.ok) {
      throw new Error(`Failed to fetch histogram for federation ${id}`);
    }
    return response.json();
  },

  async getFederationHealth(id: string): Promise<Record<string, unknown>> {
    const response = await fetch(`${BASE_URL}/federations/${id}/health`);
    if (!response.ok) {
      throw new Error(`Failed to fetch health for federation ${id}`);
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

  async getFederationGatewaysByInvite(inviteCode: string): Promise<GatewayInfo[]> {
    const encodedInvite = encodeURIComponent(inviteCode);
    const response = await fetch(`${BASE_URL}/config/${encodedInvite}/gateways`);
    if (!response.ok) {
      throw new Error(`Failed to fetch gateways by invite (${response.status})`);
    }
    return response.json();
  },
};
