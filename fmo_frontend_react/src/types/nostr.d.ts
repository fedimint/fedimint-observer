export interface NostrEvent {
  kind: number;
  created_at: number;
  tags: string[][];
  content: string;
  pubkey: string;
}

export interface SignedNostrEvent extends NostrEvent {
  id: string;
  sig: string;
}

// Injected by Nostr browser extensions such as nos2x or Alby (NIP-07)
declare global {
  interface Window {
    nostr?: {
      getPublicKey(): Promise<string>;
      signEvent(event: NostrEvent): Promise<SignedNostrEvent>;
    };
  }
}
