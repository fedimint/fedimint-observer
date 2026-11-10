import { useEffect, useState } from 'react';
import { api } from '../../../services/api';
import type { FederationSummary } from '../../../types/api';
import { startAutoRefresh } from '../../../utils/autoRefresh';

const REFRESH_MS = 60_000;

/**
 * Every observed federation, refreshed each minute while the page is visible.
 * A failed refresh keeps the list from the last load and sets `refreshFailed`.
 */
export function useFederationList() {
  const [federations, setFederations] = useState<FederationSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);
  // When the shown list last loaded; set while a later refresh is failing
  const [updatedAt, setUpdatedAt] = useState<number | null>(null);
  const [refreshFailed, setRefreshFailed] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;
    let lastSuccess: number | null = null;
    let inFlight = false;
    const loadFederations = async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        const data = await api.getFederations(signal);
        if (signal.aborted) return;
        setFederations(data);
        lastSuccess = Date.now();
        setUpdatedAt(lastSuccess);
        setRefreshFailed(false);
        setFailed(false);
        setLoading(false);
      } catch (err) {
        if (signal.aborted) return;
        console.error('Failed to fetch federations:', err);
        if (lastSuccess === null) {
          setFailed(true);
          setLoading(false);
        } else {
          setRefreshFailed(true);
        }
      } finally {
        inFlight = false;
      }
    };
    // No polling in hidden tabs; showing one refreshes a list over a minute old
    const stop = startAutoRefresh(() => { void loadFederations(); }, REFRESH_MS, { lastSuccessAt: () => lastSuccess });
    return () => {
      stop();
      controller.abort();
    };
  }, []);

  return { federations, loading, failed, updatedAt, refreshFailed };
}
