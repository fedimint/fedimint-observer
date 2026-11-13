import { api } from '../../../services/api';
import type { FederationHealth } from '../../../types/api';
import { LookupCache } from '../../../utils/lookupCache';

export interface NostrFederation {
  id: string;
  name: string | null;
  invite: string;
  health?: FederationHealth;
}

// Each name lookup makes the server fetch the federation's config, so only a few run at once
const NAME_LOOKUP_CONCURRENCY = 4;
const NAME_LOOKUP_TIMEOUT_MS = 5000;
// Names that arrive this close together update the list once
const NAME_BATCH_MS = 200;

// Meta values are parsed as JSON where possible, so a name like "21" arrives as a number
export function federationName(meta: Record<string, unknown>): string | null {
  const name = meta.federation_name;
  return name ? String(name) : null;
}

// Each lookup makes the server ask the federation's guardians, and names
// rarely change, so they are remembered for the session instead of fetched on
// every visit.
const federationNames = new LookupCache<string | null>();

// Gives up after the timeout. It is not tied to one visit of the page: a lookup
// still running when the page closes completes into the cache for the next.
function fetchFederationName(invite: string): Promise<string | null> {
  return federationNames.get(invite, async () => {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), NAME_LOOKUP_TIMEOUT_MS);
    try {
      return federationName(await api.getFederationMetaByInvite(invite, controller.signal));
    } finally {
      clearTimeout(timeout);
    }
  });
}

/** Looks up the names a few at a time and hands them to `onNames` in batches */
export async function lookUpNames(
  federations: NostrFederation[],
  signal: AbortSignal,
  onNames: (names: Map<string, string>) => void,
) {
  const pending = new Map<string, string>();
  let flushTimer: ReturnType<typeof setTimeout> | undefined;
  const flush = () => {
    flushTimer = undefined;
    if (signal.aborted || pending.size === 0) return;
    onNames(new Map(pending));
    pending.clear();
  };

  let next = 0;
  const worker = async () => {
    while (next < federations.length && !signal.aborted) {
      const fed = federations[next++];
      try {
        const name = await fetchFederationName(fed.invite);
        if (name) {
          pending.set(fed.id, name);
          flushTimer ??= setTimeout(flush, NAME_BATCH_MS);
        }
      } catch (err) {
        if (!signal.aborted) console.error(`Failed to fetch name for ${fed.id}:`, err);
      }
    }
  };
  await Promise.all(Array.from({ length: NAME_LOOKUP_CONCURRENCY }, worker));
  clearTimeout(flushTimer);
  flush();
}
