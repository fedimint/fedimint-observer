/** Trims and strips a fedimint: URL scheme */
export function normalizeSearchInput(input: string): string {
  return input.trim().replace(/^(fedimint|web\+fedimint):\/\//i, '').replace(/^(fedimint|web\+fedimint):/i, '');
}

/** Bech32 fed1 or Base32 fedimint invite */
export function isInviteCodeQuery(input: string): boolean {
  const normalized = normalizeSearchInput(input).toLowerCase();
  return normalized.startsWith('fed1') || normalized.startsWith('fedimint');
}

export function searchKey(input: string): string {
  return normalizeSearchInput(input).toLowerCase();
}

export type InviteLookup =
  | { query: string; status: 'loading' }
  | { query: string; status: 'done'; federationId: string }
  | { query: string; status: 'error'; message: string };

export function lookupForInput(lookup: InviteLookup | null, input: string): InviteLookup | null {
  return lookup && lookup.query === searchKey(input) ? lookup : null;
}

/** The largest daily figures across all federations, so every row's chart has the same scale */
export interface ActivityScale {
  maxTransaction: number;
  /** In msats */
  maxVolume: number;
}
