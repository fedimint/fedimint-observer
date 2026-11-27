import { useEffect, useMemo, useRef } from 'react';
import { api } from '../../../services/api';
import { useApiResource } from '../../../hooks/useApiResource';
import { startAutoRefresh } from '../../../utils/autoRefresh';
import type { GuardianHealthWindow } from '../../../types/api';
import type { FederationConfig } from '../../../utils/config';
import { useGuardianHealth } from '../../federation/hooks/useGuardianHealth';
import {
  buildGuardianRows,
  connectivityIssues,
  connectivityMatrix,
  federationWarnings,
  resilience,
  versionSummary,
} from '../lib/guardianModel';


const LIVE_REFRESH_MS = 60_000;


/** Each guardian's latest polls and peer views, refreshed every minute while the page is visible */
export function useLiveHealth(id: string) {
  const resource = useApiResource((signal) => api.getGuardianLiveHealth(id, signal), [id], { label: 'live guardian health' });
  const lastSuccess = useRef<number | null>(Date.now());
  useEffect(() => {
    if (resource.data) lastSuccess.current = Date.now();
  }, [resource.data]);
  const { reload } = resource;
  useEffect(() => startAutoRefresh(reload, LIVE_REFRESH_MS, { lastSuccessAt: () => lastSuccess.current }), [reload]);
  return resource;
}

/** Starts loading a window before it is chosen, e.g. while its button is hovered, so choosing it is instant */
export function prefetchGuardianHistory(id: string, window: GuardianHealthWindow) {
  api.getGuardianHealthHistory(id, window).catch((error: unknown) =>
    console.warn('Could not prefetch guardian health history:', error),
  );
}

/** Availability buckets for one window; switching windows shows a fresh load, not the old window's grid */

export function useGuardianHistory(id: string, window: GuardianHealthWindow, enabled = true) {
  const resource = useApiResource((signal) => api.getGuardianHealthHistory(id, window, signal), [id, window], {
    enabled,
    resetOnError: true,
    label: 'guardian health history',
  });
  const switching = resource.loading && resource.data !== null && resource.data.window !== window;
  return { ...resource, switching };
}

export function useSeatBindings(id: string) {
  return useApiResource((signal) => api.getManifoldSeatBindings(id, signal), [id], { label: 'Manifold seat bindings' });
}


export function useGuardianObservability(id: string, config: FederationConfig | null) {
  const health = useGuardianHealth(id);
  const live = useLiveHealth(id);
  const history7d = useGuardianHistory(id, '7d');
  const seatBindings = useSeatBindings(id);

  const model = useMemo(() => {
    const rows = buildGuardianRows({
      guardians: config?.guardians ?? [],
      health: health.status === 'loaded' ? health.health : null,
      live: live.data,
      history7d: history7d.data,
      seatBindings: seatBindings.data,
    });
    const res = resilience(rows, live.data !== null);
    const matrix = connectivityMatrix(rows);
    const issues = connectivityIssues(rows, matrix);
    return {
      rows,
      resilience: res,
      matrix,
      connectivityIssues: issues,
      warnings: federationWarnings(rows, res, issues),
      versions: versionSummary(rows),
    };
  }, [config, health.status, health.health, live.data, history7d.data, seatBindings.data]);

  return { ...model, health, live, history7d, seatBindings };
}

export type GuardianObservability = ReturnType<typeof useGuardianObservability>;
