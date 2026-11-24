import { formatUtcDateRange, formatUtcDateTime } from '../../../utils/time.ts';

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

/** What a shut-down row shows for its observed sessions */
export interface ObservedSessionSpan {
  text: string;
  /** Exact bounds and what they measure, for the hover title */
  title: string;
}

const SPAN_NOTE = 'Estimated times of the first and last sessions this observer recorded, not announced start or shutdown dates';

/** Missing or omitted bounds (older servers send neither) read as unavailable */
export function observedSessionSpan(first: number | null | undefined, last: number | null | undefined): ObservedSessionSpan {
  const start = first == null ? null : first * 1000;
  const end = last == null ? null : last * 1000;
  if (start === null || end === null) return { text: formatUtcDateRange(null, null), title: SPAN_NOTE };
  return {
    text: formatUtcDateRange(start, end),
    title: `${formatUtcDateTime(start)} – ${formatUtcDateTime(end)}. ${SPAN_NOTE}`,
  };
}
