import type { AdFreshness, CredentialState, MgPlan } from '../types';

/** The FA's maximum advertisement age (`MG_ADVERTISEMENT_MAX_AGE`) */
export const MAX_AD_AGE_SECONDS = 2 * 60 * 60;

/** Freshness of an advertisement at `nowSeconds`, by the FA's rule */
export function adFreshness(issuedAt: number, expiresAt: number, nowSeconds: number): AdFreshness {
  if (issuedAt > nowSeconds) return 'not_yet_valid';
  if (expiresAt <= nowSeconds) return 'expired';
  if (nowSeconds - issuedAt > MAX_AD_AGE_SECONDS) return 'stale';
  return 'fresh';
}

/** The setup price a plan advertises up front, in msat */
export function planUpfrontMsats(plan: MgPlan): number {
  return plan.kind === 'infinite_best_effort' ? plan.price_msats : plan.initial_price_msats;
}

/** Lowest up-front price among the plans; null without plans */
export function cheapestPlanMsats(plans: readonly MgPlan[]): number | null {
  return plans.length === 0 ? null : Math.min(...plans.map(planUpfrontMsats));
}

/** "0.8.1+fedi" -> "0.8"; the FA groups compatibility by major and minor */
export function minorVersion(version: string): string {
  const match = /^(\d+)\.(\d+)/.exec(version);
  return match ? `${match[1]}.${match[2]}` : version;
}

export type VerificationFilter = 'all' | CredentialState;
export type DirectorySort = 'freshness' | 'price_asc' | 'price_desc' | 'name';

export interface DirectoryQuery {
  search: string;
  verification: VerificationFilter;
  /** A minor version such as "0.8", or 'all' */
  version: string;
  /** A federation size, or 'all' */
  size: number | 'all';
  /** Hide advertisements that are no longer fresh */
  freshOnly: boolean;
  sort: DirectorySort;
}

export const DEFAULT_QUERY: DirectoryQuery = {
  search: '',
  verification: 'all',
  version: 'all',
  size: 'all',
  freshOnly: false,
  sort: 'freshness',
};

/** "2,100 sats" for prices, which the ads state in msat */
export function formatPriceMsats(msats: number): string {
  const sats = msats / 1000;
  return `${sats.toLocaleString('en-US', { maximumFractionDigits: sats < 10 ? 3 : 0 })} sats`;
}

/** Federation sizes as ranges: [7, 8, 9, 12] -> "7–9, 12" */
export function formatSizes(sizes: readonly number[]): string {
  const sorted = [...new Set(sizes)].sort((a, b) => a - b);
  const parts: string[] = [];
  for (let i = 0; i < sorted.length; i++) {
    let j = i;
    while (j + 1 < sorted.length && sorted[j + 1] === sorted[j] + 1) j++;
    parts.push(j > i + 1 ? `${sorted[i]}–${sorted[j]}` : j === i + 1 ? `${sorted[i]}, ${sorted[j]}` : `${sorted[i]}`);
    i = j;
  }
  return parts.join(', ');
}
