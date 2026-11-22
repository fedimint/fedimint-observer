import { request } from '../../services/http';
import type { FlipProvider, ManifoldGuardianProfile, ManifoldGuardianSummary, Sourced } from './types';
import type { DirectoryQuery } from './lib/directory';

const BASE_URL = import.meta.env.VITE_FMO_API_BASE_URL || 'https://observer.fedimint.org/api';

/** The backend's discovery answer: what its collector stored, and when it last synced */
type DiscoveryResponse<T> = Sourced<T>;

const sourced = <T,>({ observed_at, relays, data, page, facets }: DiscoveryResponse<T>): Sourced<T> => ({
  observed_at,
  relays,
  data,
  page,
  facets,
});

/** The directory's filters and order as the backend's query parameters */
function directoryParams(query: DirectoryQuery, limit: number, after?: string): URLSearchParams {
  const params = new URLSearchParams({ sort: query.sort, limit: String(limit) });
  if (query.search.trim()) params.set('search', query.search.trim());
  if (query.verification !== 'all') params.set('verification', query.verification);
  if (query.version !== 'all') params.set('version', query.version);
  if (query.size !== 'all') params.set('size', String(query.size));
  if (query.freshOnly) params.set('fresh_only', 'true');
  if (after) params.set('after', after);
  return params;
}

/**
 * Manifold discovery data as the observer's collector verified and stored it
 * from Nostr relays. The backend never asks relays per request, so these are
 * cheap reads.
 */
export const manifoldApi = {
  /** One page of the directory, searched, filtered and ordered by the backend */
  async listGuardians(query: DirectoryQuery, limit: number, after?: string, signal?: AbortSignal): Promise<Sourced<ManifoldGuardianSummary[]>> {
    const params = directoryParams(query, limit, after);
    return sourced(await request<DiscoveryResponse<ManifoldGuardianSummary[]>>(`${BASE_URL}/manifold/guardians?${params}`, { signal }));
  },

  /** null when the observer has never seen the key (404) */
  async getGuardian(fmanPubkey: string, signal?: AbortSignal): Promise<Sourced<ManifoldGuardianProfile> | null> {
    try {
      return sourced(await request<DiscoveryResponse<ManifoldGuardianProfile>>(
        `${BASE_URL}/manifold/guardians/${encodeURIComponent(fmanPubkey)}`,
        { signal },
      ));
    } catch (error) {
      if (error && typeof error === 'object' && 'status' in error && (error.status === 404 || error.status === 400)) return null;
      throw error;
    }
  },

  async listFlipProviders(limit: number, after?: string, signal?: AbortSignal): Promise<Sourced<FlipProvider[]>> {
    const params = new URLSearchParams({ limit: String(limit) });
    if (after) params.set('after', after);
    return sourced(await request<DiscoveryResponse<FlipProvider[]>>(`${BASE_URL}/manifold/flip-providers?${params}`, { signal }));
  },
};
