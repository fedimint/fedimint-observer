import { useEffect, useMemo, useState } from 'react';
import { api } from '../../../services/api';
import { errorMessage } from '../../../utils/errors';
import type { LiveLookup } from '../lib/gatewaySelection';

const LIVE_LOOKUP_TIMEOUT_MS = 10_000;

interface LiveGatewayLookupOptions {

  invite: string | undefined;

  offline: boolean;

  federationPending: boolean;
}


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
          error: timedOut ? 'The live registry lookup timed out' : errorMessage(err),
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

    retryIfFailed: () => {
      if (live.status === 'done' && live.error) setAttempt((count) => count + 1);
    },
  };
}
