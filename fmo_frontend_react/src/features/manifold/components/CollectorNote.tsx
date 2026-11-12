import type { ReactNode } from 'react';
import { Icon } from '../../../components/ui/Icon';

/**
 * Where a page's discovery data comes from and what its verification covers.
 * The sync time is the page header's data stamp, so it is not repeated here.
 */
export function CollectorNote({ relays, children }: { relays: readonly string[]; children?: ReactNode }) {
  return (
    <div className="flex gap-3 rounded-xl border border-manifold/25 bg-manifold-soft px-4 py-3 text-xs leading-relaxed text-manifold-ink">
      <Icon name="info" className="mt-px size-4" />
      <div className="min-w-0 space-y-1">
        <p>
          Collected from{' '}
          {relays.map((relay, i) => (
            <span key={relay}>
              <code className="font-mono">{relay.replace(/^wss:\/\//, '').replace(/\/$/, '')}</code>
              {i < relays.length - 2 ? ', ' : i === relays.length - 2 ? ' and ' : ''}
            </span>
          ))}
          . Every advertisement shown passed its signature checks, but its contents are the publisher's own claims.
        </p>
        {children && <p className="opacity-90">{children}</p>}
      </div>
    </div>
  );
}
