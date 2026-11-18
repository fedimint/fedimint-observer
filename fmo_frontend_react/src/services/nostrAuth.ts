import type { NostrEvent, SignedNostrEvent } from '../types/nostr';
import type { HttpMethod } from './http';

/**
 * Admin requests are authorized by a Nostr event the admin's signer signs for
 * that one request (NIP-98): the event names the request's URL, method and the
 * hash of its body, and the server accepts it for a minute, once. The key never
 * leaves the signer, and the page holds nothing that could be stolen and reused.
 */

/** NIP-98 HTTP auth */
const HTTP_AUTH_KIND = 27235;

/** What signs: the browser's Nostr extension (NIP-07), or a stand-in in tests */
export interface NostrSigner {
  getPublicKey(): Promise<string>;
  signEvent(event: NostrEvent): Promise<SignedNostrEvent>;
}

export class NostrSignerError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'NostrSignerError';
  }
}

/** The browser's Nostr extension; fails with a message fit to show without one */
export function browserSigner(): NostrSigner {
  const nostr = window.nostr;
  if (!nostr) {
    throw new NostrSignerError('No Nostr extension found. Install one such as nos2x or Alby, then reload this page.');
  }
  return nostr;
}

const hex = (bytes: Uint8Array) => Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');

async function sha256Hex(text: string): Promise<string> {
  // Only in secure contexts (https, localhost), which is also where a signer should be used
  if (!globalThis.crypto?.subtle) {
    throw new NostrSignerError('This page is not served securely, so requests cannot be signed.');
  }
  return hex(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text))));
}

function base64(text: string): string {
  let binary = '';
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte);
  return btoa(binary);
}

interface SignedRequest {
  /** The absolute URL the request must be sent to, as signed */
  url: string;
  method: HttpMethod;
  /** The exact body that was hashed and must be sent */
  body?: string;
}

/**
 * The `Authorization` header for one request, signed by `pubkey`.
 *
 * The signer is asked for exactly this request, and its answer is checked: a
 * signer that switched accounts, or returned an event other than the one asked
 * for, fails here instead of sending something the admin did not mean.
 */
export async function authorizationHeader(signer: NostrSigner, pubkey: string, { url, method, body }: SignedRequest): Promise<string> {
  const tags = [
    ['u', url],
    ['method', method],
  ];
  if (body !== undefined) tags.push(['payload', await sha256Hex(body)]);
  // Two requests alike within one second would otherwise be the same event, and
  // the server refuses an event it has seen
  tags.push(['nonce', hex(crypto.getRandomValues(new Uint8Array(16)))]);

  const template: NostrEvent = { kind: HTTP_AUTH_KIND, created_at: Math.floor(Date.now() / 1000), tags, content: '', pubkey };
  let signed: SignedNostrEvent;
  try {
    signed = await signer.signEvent(template);
  } catch {
    // Extensions reject with their own wording when the user declines
    throw new NostrSignerError('The request was not signed. Approve it in your Nostr extension to continue.');
  }
  if (signed.pubkey !== pubkey) {
    throw new NostrSignerError('Your Nostr extension signed with a different key. Sign in again.');
  }
  if (signed.kind !== template.kind || signed.content !== template.content || JSON.stringify(signed.tags) !== JSON.stringify(tags)) {
    throw new NostrSignerError('Your Nostr extension changed the request before signing it.');
  }
  return `Nostr ${base64(JSON.stringify(signed))}`;
}

/** `url` as the browser will request it: absolute, so it is what the server compares */
export function absoluteUrl(url: string): string {
  return new URL(url, typeof window === 'undefined' ? undefined : window.location.href).toString();
}
