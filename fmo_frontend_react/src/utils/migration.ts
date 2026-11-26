import type { FederationSummary } from '../types/api';


export const MIGRATION_HELP_URL = 'https://www.fedi.xyz/blog/important-update-migrate-your-g-bot-federations';

const MIGRATION_HELP_UNTIL = Date.UTC(2026, 11, 9);

export function showMigrationHelp(now = Date.now()): boolean {
  return now < MIGRATION_HELP_UNTIL;
}


export function isMigrating(federation: Pick<FederationSummary, 'expiry_timestamp' | 'successor_id'>): boolean {
  return federation.expiry_timestamp != null && federation.successor_id != null;
}
