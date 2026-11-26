import type { ReactNode } from 'react';
import { api } from '../../../services/api';
import { toBitcoin, formatNumber } from '../../../utils/format';
import { Alert } from '../../../components/ui/Alert';
import { Icon } from '../../../components/ui/Icon';
import { useApiResource } from '../../../hooks/useApiResource';
import { activeAssetsStat } from '../lib/activeAssets';

export function Totals() {
  const { data: totals, loading, failed } = useApiResource((signal) => api.getTotals(signal), [], { label: 'totals' });

  if (failed) {
    return <Alert level="error" message="Could not load the totals. Please try again later." />;
  }

  const assets = activeAssetsStat(totals);

  return (
    <div className="grid grid-cols-3 divide-x divide-line-strong text-ink">
      <Stat label={<><span className="sm:hidden">Federations</span><span className="hidden sm:inline">Observed federations</span></>}>
        {loading ? '…' : formatNumber(totals?.federations || 0)}
      </Stat>
      <Stat label="Total transactions">
        {loading ? '…' : formatNumber(totals?.tx_count || 0)}
      </Stat>
      <Stat
        label="Total assets"
        title={assets.title}
        note={!loading && assets.kind === 'held' ? assets.coverage : undefined}
      >
        {loading ? '…' : assets.kind === 'unavailable' ? (
          <span className="text-base font-medium text-ink-2 sm:text-2xl">Unavailable</span>
        ) : (
          <>
            <Icon name="bitcoin" className="size-5 sm:size-8" />
            <span className="whitespace-nowrap">{toBitcoin(assets.msat, 3)}</span>
          </>
        )}
      </Stat>
    </div>
  );
}

function Stat({ label, title, note, children }: { label: ReactNode; title?: string; note?: ReactNode; children: ReactNode }) {
  return (
    <div className="px-2 text-center" title={title}>
      <div className="flex h-8 items-center justify-center gap-1.5 text-2xl font-semibold tabular sm:h-10 sm:text-4xl">{children}</div>
      <div className="mt-1 text-xs text-ink-2 sm:mt-2 sm:text-sm">{label}</div>
      {note && <div className="mt-0.5 text-xs text-ink-3">{note}</div>}
    </div>
  );
}
