import { useEffect, useMemo, useState } from 'react';
import { api } from '../../../services/api';
import { errorMessage } from '../../../utils/errors';
import type { LiveLookup } from '../lib/gatewaySelection';

const LIVE_LOOKUP_TIMEOUT_MS = 10_000;

interface LiveGatewayLookupOptions {
  /** The federation's invite code, once known */
  invite: string | undefined;
  /** An offline federation's guardians can't answer; the lookup would hang for a minute */
  offline: boolean;
  /** Whether the federation, and so its invite, is still loading */
  federationPending: boolean;
}

/**
 * The gateways in the federation's own registry, asked of its guardians through
 * the observer. It only adds details, so it never holds up the page.
 */
export function useLiveGatewayLookup({ invite, offline, federationPending }: LiveGatewayLookupOptions) {
  const [live, setLive] = useState<LiveLookup>({ status: 'pending' });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!invite || offline) return;
    const controller = new AbortController();
    let cancelled = false;
    let timedOut = false;
    const timeout = setTimeout(() => {
      timedOut = true;
      controller.abort();
    }, LIVE_LOOKUP_TIMEOUT_MS);
    setLive({ status: 'pending' });
    api.getFederationGatewaysByInvite(invite, controller.signal)
      .then((gateways) => {
        if (!cancelled) setLive({ status: 'done', gateways, error: null });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setLive({
          status: 'done',
          gateways: [],
          error: timedOut ? 'The live registry lookup timed out' : errorMessage(err, 'The live registry lookup failed'),
        });
      })
      .finally(() => clearTimeout(timeout));
    return () => {
      cancelled = true;
      clearTimeout(timeout);
      controller.abort();
    };
  }, [invite, offline, attempt]);

  const lookup = useMemo<LiveLookup>(() => {
    if (invite && !offline) return live;
    return { status: federationPending ? 'pending' : 'skipped' };
  }, [federationPending, invite, live, offline]);

  return {
    lookup,
    /** Repeats a lookup that failed; a pending or successful one is left alone */
    retryIfFailed: () => {
      if (live.status === 'done' && live.error) setAttempt((count) => count + 1);
    },
  };
}
