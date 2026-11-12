import { useEffect, useRef, useState } from 'react';
import { shortMiddle } from '../../utils/format';
import { Icon } from './Icon';

interface KeyChipProps {
  value: string;
  /** Accessible description of what is copied, e.g. "Manifold Guardian public key" */
  label: string;
  head?: number;
  tail?: number;
  /** Show the full value, wrapping, instead of a shortened one */
  full?: boolean;
}

/** A public key or id in monospace, shortened in the middle, with a copy button */
export function KeyChip({ value, label, head = 8, tail = 6, full = false }: KeyChipProps) {
  const [copied, setCopied] = useState<'yes' | 'failed' | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopied('yes');
    } catch {
      setCopied('failed');
    }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(null), 1800);
  };

  return (
    <span className="inline-flex max-w-full items-center gap-1 rounded-md border border-line bg-surface-2 py-0.5 pr-0.5 pl-2 align-middle">
      <code className={`font-mono text-[0.72rem] text-ink-2 ${full ? 'break-all' : 'truncate'}`} title={value}>
        {full ? value : shortMiddle(value, head, tail)}
      </code>
      <button
        type="button"
        onClick={copy}
        aria-label={`Copy ${label}`}
        className="inline-flex size-6 shrink-0 items-center justify-center rounded text-ink-3 hover:bg-surface-3 hover:text-ink"
      >
        <Icon name={copied === 'yes' ? 'check' : 'copy'} className={`size-3.5 ${copied === 'yes' ? 'text-ok' : ''}`} />
      </button>
      <span role="status" className="sr-only">
        {copied === 'yes' ? `${label} copied` : copied === 'failed' ? `Could not copy ${label}` : ''}
      </span>
    </span>
  );
}
