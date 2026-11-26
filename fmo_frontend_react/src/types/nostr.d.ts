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

declare global {
  interface Window {
    nostr?: {
      getPublicKey(): Promise<string>;
      signEvent(event: NostrEvent): Promise<SignedNostrEvent>;
    };
  }
}
