import { useEffect, useMemo, useRef } from 'react';
import { api } from '../../../services/api';
import { useApiResource } from '../../../hooks/useApiResource';
import { startAutoRefresh } from '../../../utils/autoRefresh';
import type { GuardianHealthHistory, GuardianHealthWindow } from '../../../types/api';
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

/** The collector polls guardians every minute, so the live view refreshes as often */
const LIVE_REFRESH_MS = 60_000;
/** History buckets are hourly; reloading a window within this reuses the answer */
const HISTORY_CACHE_MS = 60_000;

const historyCache = new Map<string, { at: number; data: Promise<GuardianHealthHistory> }>();

function cachedHistory(id: string, window: GuardianHealthWindow, signal: AbortSignal): Promise<GuardianHealthHistory> {
  const key = `${id}:${window}`;
  const cached = historyCache.get(key);
  if (cached && Date.now() - cached.at < HISTORY_CACHE_MS) return cached.data;
  // Not tied to one component's signal, since another may reuse it; abort only drops the answer
  const data = api.getGuardianHealthHistory(id, window);
  historyCache.set(key, { at: Date.now(), data });
  data.catch(() => historyCache.delete(key));
  return new Promise((resolve, reject) => {
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    data.then(resolve, reject);
  });
}

/** Each guardian's latest polls and peer views, refreshed every minute while the page is visible */
export function useLiveHealth(id: string) {
  const resource = useApiResource((signal) => api.getGuardianLiveHealth(id, signal), [id], { label: 'live guardian health' });
  // The first load is already running; count it as fresh so the timer does not repeat it
  const lastSuccess = useRef<number | null>(Date.now());
  useEffect(() => {
    if (resource.data) lastSuccess.current = Date.now();
  }, [resource.data]);
  const { reload } = resource;
  useEffect(() => startAutoRefresh(reload, LIVE_REFRESH_MS, { lastSuccessAt: () => lastSuccess.current }), [reload]);
  return resource;
}

/** Availability buckets for one window; switching windows shows a fresh load, not the old window's grid */
export function useGuardianHistory(id: string, window: GuardianHealthWindow, enabled = true) {
  return useApiResource((signal) => cachedHistory(id, window, signal), [id, window], {
    enabled,
    resetOnReload: true,
    label: 'guardian health history',
  });
}

export function useSeatBindings(id: string) {
  return useApiResource((signal) => api.getManifoldSeatBindings(id, signal), [id], { label: 'Manifold seat bindings' });
}

/**
 * Everything the federation's guardian views show, from the observer's stored
 * data only. `config` comes from the page's federation details.
 */
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
