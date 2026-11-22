import { useState } from 'react';
import { Button } from './Button';
import { CONTROL_RADIUS } from './controls';

interface CopyableProps {
  text: string;
}

/** A read-only value in a field, with a copy button of the same height */
export function Copyable({ text }: CopyableProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy text:', err);
    }
  };

  return (
    <div className="flex items-center gap-2">
      <input
        className={`h-8 min-w-0 flex-1 truncate border border-line bg-surface-2 px-3 font-mono text-xs text-ink-2 ${CONTROL_RADIUS}`}
        value={text}
        readOnly
        title={text}
        aria-label="Invite code"
      />
      <Button
        icon={copied ? 'check' : 'copy'}
        onClick={handleCopy}
        aria-label={copied ? 'Copied' : 'Copy to clipboard'}
        title={copied ? 'Copied' : 'Copy to clipboard'}
        className={copied ? 'text-ok-ink' : ''}
      />
    </div>
  );
}
