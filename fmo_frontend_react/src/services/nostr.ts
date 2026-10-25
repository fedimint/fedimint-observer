import type { NostrEvent, SignedNostrEvent } from '../types/nostr';

export type NostrEventTemplate = Omit<NostrEvent, 'created_at' | 'pubkey'>;

// Signs an event with the browser's Nostr extension and hands it to `publish`. The
// extension is checked before `buildEvent` runs, so a missing one fails before any request.
export async function publishNostrEvent(
  buildEvent: () => NostrEventTemplate | Promise<NostrEventTemplate>,
  publish: (event: SignedNostrEvent) => Promise<void>,
): Promise<void> {
  const nostr = window.nostr;
  if (!nostr) {
    throw new Error('Nostr extension not found. Please install a Nostr browser extension like nos2x or Alby.');
  }

  const template = await buildEvent();
  const pubkey = await nostr.getPublicKey();
  const signedEvent = await nostr.signEvent({
    ...template,
    created_at: Math.floor(Date.now() / 1000),
    pubkey,
  });
  await publish(signedEvent);
}
