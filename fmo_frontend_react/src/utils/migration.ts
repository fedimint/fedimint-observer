import type { FederationSummary } from '../types/api';

/** Fedi's explainer for the federations its guardian bots are moving off */
export const MIGRATION_HELP_URL = 'https://www.fedi.xyz/blog/important-update-migrate-your-g-bot-federations';

// The explainer is linked for a month after the announcement, Dec 9, 2026 UTC
const MIGRATION_HELP_UNTIL = Date.UTC(2026, 11, 9);

export function showMigrationHelp(now = Date.now()): boolean {
  return now < MIGRATION_HELP_UNTIL;
}

/**
 * A federation is migrating once its guardians publish both an expiry date and
 * a successor invite code that parses
 */
export function isMigrating(federation: Pick<FederationSummary, 'expiry_timestamp' | 'successor_id'>): boolean {
  return federation.expiry_timestamp != null && federation.successor_id != null;
}

/** Display name of every federation in a list, by ID, to name a successor the observer tracks */
export function federationNames(federations: FederationSummary[]): Map<string, string> {
  return new Map(federations.map((federation) => [federation.id, federation.name || 'Unnamed']));
}
