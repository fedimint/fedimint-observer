import { api } from '../../../services/api';
import type { GuardianHealth } from '../../../types/api';
import { useApiResource } from '../../../hooks/useApiResource';

export type GuardianHealthStatus = 'loading' | 'loaded' | 'failed';

const NO_HEALTH: Record<string, GuardianHealth> = {};

/** The latest status of each guardian, by guardian id */
export function useGuardianHealth(id: string) {
  const resource = useApiResource((signal) => api.getFederationHealth(id, signal), [id], {
    resetOnReload: true,
    label: 'guardian health',
  });
  const status: GuardianHealthStatus = resource.loading ? 'loading' : resource.failed ? 'failed' : 'loaded';
  const health = resource.data ?? NO_HEALTH;
  const hasOnlineGuardian = Object.values(health).some((guardian) => guardian?.latest != null);
  return { health, status, hasOnlineGuardian, retry: resource.reload };
}
