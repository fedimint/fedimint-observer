import type { FederationSummary } from '../../../types/api';
import { sortFederations, type SortDirection, type SortKey } from '../../../utils/sortFederations';

export interface FederationGroups {
  active: FederationSummary[];
  offline: FederationSummary[];
  shutDown: FederationSummary[];
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
): FederationGroups {
  const query = search.trim().toLowerCase();
  const matching = query
    ? federations.filter((fed) => (fed.name?.toLowerCase() ?? '').includes(query) || (fed.invite?.toLowerCase() ?? '').includes(query))
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
