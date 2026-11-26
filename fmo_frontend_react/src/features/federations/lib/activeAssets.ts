import type { FedimintTotals } from '../../../types/api';

const SCOPE = 'Bitcoin held by all active federations, whatever the search or page below';

export type ActiveAssetsStat =
  | { kind: 'unavailable'; title: string }
  | { kind: 'held'; msat: number; title: string; coverage: string | null };

/** The Total Assets stat: an unknown total never shows as zero, and a partial one says so */
export function activeAssetsStat(totals: FedimintTotals | null | undefined): ActiveAssetsStat {
  const held = totals?.active_assets;
  const unknown = totals?.active_assets_unknown ?? 0;
  if (held == null) {
    const reason =
      totals?.active_assets_unknown != null && unknown > 0
        ? 'no active federation has reported a balance yet'
        : 'asset data could not be read';
    return { kind: 'unavailable', title: `${SCOPE}. Unavailable: ${reason}.` };
  }
  if (unknown === 0) {
    return { kind: 'held', msat: held, title: `${SCOPE}.`, coverage: null };
  }
  const reporting = totals?.active_assets_reporting;
  const coverage = reporting == null ? `${unknown} not counted` : `${reporting} of ${reporting + unknown} counted`;
  return {
    kind: 'held',
    msat: held,
    title: `${SCOPE}. ${unknown} without a known balance yet are not counted.`,
    coverage,
  };
}
