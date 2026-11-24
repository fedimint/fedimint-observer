import { useEffect, useState } from 'react';
import { api } from '../../../services/api';
import { ApiError } from '../../../services/http';
import type { FederationSummary } from '../../../types/api';

const RETRY_MS = 60_000;


export function useGatewayFederation(id: string) {
  const [federation, setFederation] = useState<FederationSummary | null | undefined>(undefined);
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api.getFederationSummary(id)
      .then((summary) => {
        if (cancelled) return;
        setFederation(summary);
        setFailed(false);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        if (error instanceof ApiError && error.status === 404) {
          setFederation(null);
          setFailed(false);
        } else {
          setFailed(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [id, attempt]);

  useEffect(() => {
    if (!failed) return;
    const timer = setTimeout(() => setAttempt((count) => count + 1), RETRY_MS);
    return () => clearTimeout(timer);
  }, [failed, attempt]);

  return { federation, failed, retry: () => setAttempt((count) => count + 1) };
}
