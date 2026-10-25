import type {
  ClientConfig,
  FedimintTotals,
  FederationHistogram,
  FederationSummary,
  FederationUtxosResponse,
  GatewayInfo,
  GatewayOverview,
  GatewayWindow,
  GuardianHealth,
} from '../types/api';
import type { SignedNostrEvent } from '../types/nostr';

const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';

// Invite codes are user input, so they are always encoded: a / ? or # would change the path
export const api = {
  async getTotals(signal?: AbortSignal): Promise<FedimintTotals> {
    const response = await fetch(`${BASE_URL}/federations/totals`, { signal });
    if (!response.ok) {
      throw new Error('Failed to fetch totals');
    }
    return response.json();
  },

  async getFederations(signal?: AbortSignal): Promise<FederationSummary[]> {
    const response = await fetch(`${BASE_URL}/federations`, { signal });
    if (!response.ok) {
      throw new Error('Failed to fetch federations');
    }
    return response.json();
  },

  async getNostrFederations(signal?: AbortSignal): Promise<Record<string, string>> {
    const response = await fetch(`${BASE_URL}/nostr/federations`, { signal });
    if (!response.ok) {
      throw new Error('Failed to fetch nostr federations');
    }
    return response.json();
  },

  // Only works for federations the observer follows; others need the invite code
  async getFederationConfig(id: string, signal?: AbortSignal): Promise<ClientConfig> {
    const response = await fetch(`${BASE_URL}/federations/${id}/config`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch config for federation ${id} (${response.status})`);
    }
    return response.json();
  },

  /** Keyed by guardian id */
  async getFederationHealth(id: string, signal?: AbortSignal): Promise<Record<string, GuardianHealth>> {
    const response = await fetch(`${BASE_URL}/federations/${id}/health`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch guardian health for federation ${id} (${response.status})`);
    }
    return response.json();
  },

  async getFederationHistogram(id: string, signal?: AbortSignal): Promise<FederationHistogram> {
    const response = await fetch(`${BASE_URL}/federations/${id}/transactions/histogram`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch transaction histogram for federation ${id} (${response.status})`);
    }
    return response.json();
  },

  async getFederationUtxos(id: string, signal?: AbortSignal): Promise<FederationUtxosResponse> {
    const response = await fetch(`${BASE_URL}/federations/${id}/utxos`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch UTXOs for federation ${id} (${response.status})`);
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

  async getFederationConfigByInvite(inviteCode: string, signal?: AbortSignal): Promise<ClientConfig> {
    const response = await fetch(`${BASE_URL}/config/${encodeURIComponent(inviteCode)}`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch federation config: ${response.status}`);
    }
    return response.json();
  },

  // Meta values are parsed as JSON where possible, so not every value is a string
  async getFederationMetaByInvite(inviteCode: string, signal?: AbortSignal): Promise<Record<string, unknown>> {
    const response = await fetch(`${BASE_URL}/config/${encodeURIComponent(inviteCode)}/meta`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch federation meta (${response.status})`);
    }
    return response.json();
  },

  async getFederationIdByInvite(inviteCode: string, signal?: AbortSignal): Promise<string> {
    const response = await fetch(`${BASE_URL}/config/${encodeURIComponent(inviteCode)}/id`, { signal });
    if (!response.ok) {
      throw new Error('Failed to fetch federation ID');
    }
    return response.json();
  },

  async getFederationGatewaysByInvite(inviteCode: string, signal?: AbortSignal): Promise<GatewayInfo[]> {
    const response = await fetch(`${BASE_URL}/config/${encodeURIComponent(inviteCode)}/gateways`, { signal });
    if (!response.ok) {
      throw new Error(`Failed to fetch gateways by invite (${response.status})`);
    }
    return response.json();
  },

  // Announces a federation (kind 38173)
  async publishNostrFederation(event: SignedNostrEvent, signal?: AbortSignal): Promise<void> {
    const response = await fetch(`${BASE_URL}/nostr/federations`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(event),
      signal,
    });
    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(`Failed to announce federation (${response.status}): ${errorText}`);
    }
  },

  // Recommends a federation (kind 38000)
  async publishNostrRating(event: SignedNostrEvent, signal?: AbortSignal): Promise<void> {
    const response = await fetch(`${BASE_URL}/federations/nostr/rating`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(event),
      signal,
    });
    if (!response.ok) {
      throw new Error(`Failed to publish rating: ${response.status}`);
    }
  },
};
