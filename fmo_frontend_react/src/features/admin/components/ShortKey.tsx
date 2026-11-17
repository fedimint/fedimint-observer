import { useState } from 'react';
import type { AdminKeyId } from '../../../types/api';
import { shortKey } from '../lib/format';

/** A Nostr key, shortened, that copies its npub when clicked */
export function ShortKey({ id }: { id: AdminKeyId }) {
  const [copied, setCopied] = useState(false);
  // An npub the server could not encode comes back empty
  const text = id.npub || id.pubkey;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // Without clipboard access the full key is still in the title
    }
  };

  return (
    <button
      type="button"
      onClick={copy}
      title={copied ? 'Copied' : `${text} (click to copy)`}
      className="font-mono text-xs text-gray-700 hover:text-blue-700 focus:outline-hidden focus-visible:underline dark:text-gray-300 dark:hover:text-blue-400"
    >
      {copied ? 'Copied' : shortKey(text)}
    </button>
  );
}
