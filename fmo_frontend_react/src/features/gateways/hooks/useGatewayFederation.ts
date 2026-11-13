import { useEffect, useState } from 'react';
import { api } from '../../../services/api';
import type { FederationSummary } from '../../../types/api';

const RETRY_MS = 60_000;

/**
 * The federation's entry in the observer's list: undefined while loading, null
 * when the observer does not track it. Only its name and the live registry
 * lookup need it, so a failure is retried every minute without blocking the page.
 */
export function useGatewayFederation(id: string) {
  const [federation, setFederation] = useState<FederationSummary | null | undefined>(undefined);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api.getFederations()
      .then((federations) => {
        if (cancelled) return;
        setFederation(federations.find((item) => item.id === id) ?? null);
        setFailed(false);
      })
      .catch(() => {
        // Only the name and the live lookup need it; the recorded data still shows
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [id, attempt]);

  // Keep retrying the federation list while it is missing
  useEffect(() => {
    if (!failed) return;
    const timer = setTimeout(() => setAttempt((count) => count + 1), RETRY_MS);
    return () => clearTimeout(timer);
  }, [failed, attempt]);

  return { federation, failed, retry: () => setAttempt((count) => count + 1) };
}
