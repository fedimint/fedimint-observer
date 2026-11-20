import type { FederationSummary } from '../../../types/api';
import { sortFederations, type SortDirection, type SortKey } from '../../../utils/sortFederations.ts';

interface FederationGroups {
  active: FederationSummary[];
  offline: FederationSummary[];
  shutDown: FederationSummary[];
}

/** Cleans input by trimming and stripping URL schemes (fedimint://, web+fedimint:) */
export function normalizeSearchInput(input: string): string {
  return input.trim().replace(/^(fedimint|web\+fedimint):\/\//i, '').replace(/^(fedimint|web\+fedimint):/i, '');
}

/** Checks if the sanitized input conforms to a Fedimint invite prefix (Bech32 'fed1' or legacy Base32 'fedimint') */
export function isInviteCodeQuery(input: string): boolean {
  const normalized = normalizeSearchInput(input).toLowerCase();
  return normalized.startsWith('fed1') || normalized.startsWith('fedimint');
}

/** The key a lookup is stored under: equal keys mean the same query */
export function searchKey(input: string): string {
  return normalizeSearchInput(input).toLowerCase();
}

/** State of an invite lookup, tagged with the query key that started it */
export type InviteLookup =
  | { query: string; status: 'loading' }
  | { query: string; status: 'done'; federationId: string }
  | { query: string; status: 'error'; message: string };

/** The lookup if it belongs to the current input, otherwise null */
export function lookupForInput(lookup: InviteLookup | null, input: string): InviteLookup | null {
  return lookup && lookup.query === searchKey(input) ? lookup : null;
}

/**
 * Search, then order, then split: a federation is shut down once the expiry its
 * guardians published has passed (as of `asOf`, when the list last loaded),
 * whatever its health; otherwise offline while too few guardians answer.
 */
export function groupFederations(
  federations: FederationSummary[],
  search: string,
  sortKey: SortKey,
  sortDirection: SortDirection,
  asOf: number | null,
  resolvedFederationId?: string | null,
): FederationGroups {
  const query = search.trim().toLowerCase();
  const normalized = normalizeSearchInput(query).toLowerCase();
  const matching = query
    ? federations.filter((fed) => {
        if (resolvedFederationId && fed.id.toLowerCase() === resolvedFederationId.toLowerCase()) return true;
        if (
          normalized &&
          (fed.id.toLowerCase().startsWith(normalized) ||
            (normalized.length >= 6 && fed.id.toLowerCase().includes(normalized)))
        ) {
          return true;
        }
        if ((fed.name?.toLowerCase() ?? '').includes(query)) return true;
        if (normalized && (fed.invite?.toLowerCase() ?? '').includes(normalized)) return true;
        return false;
      })
    : federations;
  const groups: FederationGroups = { active: [], offline: [], shutDown: [] };
  for (const fed of sortFederations(matching, sortKey, sortDirection)) {
    if (fed.expiry_timestamp != null && asOf !== null && fed.expiry_timestamp * 1000 <= asOf) groups.shutDown.push(fed);
    else if (fed.health === 'offline') groups.offline.push(fed);
    else groups.active.push(fed);
  }
  return groups;
}

/** The largest daily figures across all federations, so every row's chart has the same scale */
export interface ActivityScale {
  maxTransaction: number;
  /** In msats */
  maxVolume: number;
}

export function activityScale(federations: FederationSummary[]): ActivityScale {
  let maxTransaction = 0;
  let maxVolume = 0;
  for (const fed of federations) {
    for (const day of fed.last_7d_activity) {
      maxTransaction = Math.max(maxTransaction, day.num_transactions);
      maxVolume = Math.max(maxVolume, day.amount_transferred);
    }
  }
  return { maxTransaction, maxVolume };
}
