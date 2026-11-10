import type { ReactNode } from 'react';
import { shortMiddle } from '../../utils/format';
import { explorerTxUrl } from '../../utils/network';

interface ExplorerLinkProps {
  /** The block explorer page, or null on a network it has no pages for */
  href: string | null;
  className: string;
  title?: string;
  children: ReactNode;
}

/** A link to the block explorer, or plain text on a network it has no pages for */
export function ExplorerLink({ href, className, title, children }: ExplorerLinkProps) {
  return href ? (
    <a href={href} target="_blank" rel="noopener noreferrer" className={className} title={title}>{children}</a>
  ) : (
    <span className={className.replace(/\S*hover:\S+/g, '')} title={title}>{children}</span>
  );
}

/** Text with each transaction id in it shortened and linked to the explorer */
export function TxidLinkedText({ text, network }: { text: string; network: string | undefined }) {
  return text.split(/\b([0-9a-f]{64})\b/).map((part, index) =>
    index % 2 === 1 ? (
      <ExplorerLink
        key={index}
        href={explorerTxUrl(part, network)}
        className="font-mono underline hover:no-underline"
        title={part}
      >
        {shortMiddle(part, 8, 8)}
      </ExplorerLink>
    ) : (
      part
    )
  );
}
