import type { AdminAuditEntry } from '../../../types/api';

/** "npub1abcdefg…uvwxyz": enough to recognise a key or id */
export function shortKey(key: string): string {
  return key.length > 20 ? `${key.slice(0, 12)}…${key.slice(-6)}` : key;
}

// Invite codes are bech32 with this prefix; longer than this is not one
const INVITE_PREFIX = 'fed1';
const MAX_INVITE_LENGTH = 4096;

/** Why the text can't be an invite code, or null if it may be one. The server decides. */
export function inviteProblem(invite: string): string | null {
  if (!invite) return 'Paste an invite code first.';
  if (!invite.toLowerCase().startsWith(INVITE_PREFIX) || invite.length > MAX_INVITE_LENGTH || /\s/.test(invite)) {
    return 'That is not an invite code. Invite codes start with "fed1".';
  }
  return null;
}

/** Why the text can't be a Nostr public key, or null if it may be one. The server decides. */
export function keyProblem(key: string): string | null {
  if (!key) return "Paste the person's public key first.";
  if (/^nsec1/i.test(key)) return 'That is a secret key (nsec). Never share it; paste the public key (npub) instead.';
  if (!/^npub1[0-9a-z]{58}$/i.test(key) && !/^[0-9a-f]{64}$/i.test(key)) {
    return 'That is not a public key. Paste an npub1… key or 64 hex characters.';
  }
  return null;
}

/** How long a grant lasts; null lasts until revoked */
export const GRANT_DURATIONS = [
  { label: 'Until revoked', days: null },
  { label: '1 day', days: 1 },
  { label: '7 days', days: 7 },
  { label: '30 days', days: 30 },
  { label: '90 days', days: 90 },
] as const;

const ACTION_LABELS: Record<string, string> = {
  'federation.add': 'Added a federation',
  'federation.backfill': 'Started a backfill',
  'module.reset': 'Reset a module',
  'key.grant': 'Granted access',
  'key.revoke': 'Revoked access',
};

/** An audit action in words; an action this build doesn't know is shown as recorded */
export function actionLabel(action: string): string {
  return ACTION_LABELS[action] ?? action;
}

/** Whether the entry's subject is a federation id, which has a page to link to */
export function isFederationAction(entry: AdminAuditEntry): boolean {
  return entry.action.startsWith('federation.') || entry.action.startsWith('module.');
}

/** A text field of an entry's detail, which is free-form JSON */
export function detailText(entry: AdminAuditEntry, field: string): string | null {
  const value = entry.detail[field];
  return typeof value === 'string' && value ? value : null;
}
