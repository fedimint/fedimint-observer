import type { PageSize } from '../../../utils/pageSize';
import type { FederationUtxosResponse, GuardianClaimedUtxoState, GuardianUtxoReport, UtxoView } from '../../../types/api';
import { shortMiddle } from '../../../utils/format';

export const UTXOS_PER_PAGE: PageSize = 25;
export const UTXO_VIEWS = ['all', 'verified', 'pending', 'mismatch'] as const satisfies readonly UtxoView[];

export type UtxoKind = Exclude<UtxoView, 'all'>;

export interface UtxoInventoryRow {
  outPoint: string;
  amount: number;
  address: string | null;
  /** State each responding guardian lists this output in */
  guardianStates: Map<number, GuardianClaimedUtxoState>;
  kind: UtxoKind;
  detail?: string;
}

export const UTXO_KIND_STYLES: Record<UtxoKind, { label: string; pill: string; detail?: string }> = {
  verified: {
    label: 'Verified',
    pill: 'bg-ok-soft text-ok-ink',
    detail: 'text-ink-3',
  },
  pending: {
    label: 'Pending',
    pill: 'bg-warn-soft text-warn-ink',
    detail: 'text-warn-ink',
  },
  mismatch: {
    label: 'Mismatch',
    pill: 'bg-crit-soft text-crit-ink',
    detail: 'text-crit-ink',
  },
};

/** The rows of the page shown, reshaped for display */
export function toInventoryRows(response: FederationUtxosResponse | null): UtxoInventoryRow[] {
  return (response?.utxos ?? []).map((row) => ({
    outPoint: row.out_point,
    amount: row.amount,
    address: row.address,
    guardianStates: new Map(
      Object.entries(row.guardian_states).map(([guardianId, state]) => [Number(guardianId), state])
    ),
    kind: row.status,
    detail: row.detail ?? undefined,
  }));
}

/** Guardians whose wallet data was compared, in id order */
export function comparedGuardianIds(guardians: GuardianUtxoReport[]): number[] {
  return guardians
    .filter((guardian) => guardian.status === 'ok')
    .map((guardian) => guardian.guardian_id)
    .sort((left, right) => left - right);
}

function pluralOutputs(count: number): string {
  return `${count} ${count === 1 ? 'output' : 'outputs'}`;
}

/** Deviations from what a threshold of guardians agrees on, one sentence each */
export function guardianFindings(guardians: GuardianUtxoReport[]): string[] {
  return guardians.flatMap((guardian) => [
    guardian.missing_outputs > 0
      && `Guardian ${guardian.guardian_id} is missing ${pluralOutputs(guardian.missing_outputs)} the other guardians agree the federation holds.`,
    guardian.extra_outputs > 0
      && `Guardian ${guardian.guardian_id} lists ${pluralOutputs(guardian.extra_outputs)} the other guardians agree the federation does not hold.`,
    guardian.wrong_amounts > 0
      && `Guardian ${guardian.guardian_id} reports a different amount for ${pluralOutputs(guardian.wrong_amounts)}.`,
  ].filter((finding): finding is string => Boolean(finding)));
}

/** `txid:vout` with the txid shortened in the middle; the full value belongs in a title */
export function shortOutpoint(outPoint: string): string {
  const [txid, vout] = outPoint.split(':');
  return vout === undefined ? shortMiddle(outPoint, 12, 12) : `${shortMiddle(txid, 12, 12)}:${vout}`;
}
