import { api } from '../../../services/api';
import { toBitcoin, formatNumber } from '../../../utils/format';
import { Alert } from '../../../components/ui/Alert';
import { Icon } from '../../../components/ui/Icon';
import { useApiResource } from '../../../hooks/useApiResource';
import { activeAssetsStat } from '../lib/activeAssets';

export function Totals() {
  const { data: totals, loading, failed } = useApiResource((signal) => api.getTotals(signal), [], { label: 'totals' });

  // Zeros would read as real figures
  if (failed) {
    return <Alert level="error" message="Could not load the totals. Please try again later." />;
  }

  const assets = activeAssetsStat(totals);

  return (
    <div className="grid grid-cols-3 gap-2 sm:gap-8 text-ink px-4 sm:px-0">
      <div className="text-center py-2 sm:py-0">
        <div className="text-xl sm:text-4xl font-bold mb-1 sm:mb-2">
          {loading ? '...' : formatNumber(totals?.federations || 0)}
        </div>
        <div className="text-[10px] sm:text-base text-ink-2">
          <span className="sm:hidden">Federations</span>
          <span className="hidden sm:inline">Observed Federations</span>
        </div>
      </div>
      <div className="text-center border-l border-line-strong py-2 sm:py-0">
        <div className="text-xl sm:text-4xl font-bold mb-1 sm:mb-2">
          {loading ? '...' : formatNumber(totals?.tx_count || 0)}
        </div>
        <div className="text-[10px] sm:text-base text-ink-2">Total Transactions</div>
      </div>
      <div className="text-center border-l border-line-strong py-2 sm:py-0" title={assets.title}>
        <div className="text-xl sm:text-4xl font-bold mb-1 sm:mb-2 flex items-center justify-center gap-1">
          {loading ? (
            '...'
          ) : assets.kind === 'unavailable' ? (
            <span className="text-sm leading-7 sm:text-2xl sm:leading-10 font-medium text-ink-2">Unavailable</span>
          ) : (
            <>
              <Icon name="bitcoin" className="size-4 sm:size-10" />
              <span className="whitespace-nowrap">{toBitcoin(assets.msat, 3)}</span>
            </>
          )}
        </div>
        <div className="text-[10px] sm:text-base text-ink-2">Total Assets</div>
        {!loading && assets.kind === 'held' && assets.coverage && (
          <div className="text-[10px] sm:text-sm text-ink-3">{assets.coverage}</div>
        )}
      </div>
    </div>
  );
}
