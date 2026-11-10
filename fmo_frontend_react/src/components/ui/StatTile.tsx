import { card, muted } from './styles';

/** A headline figure with its label above and a note below */
export function StatTile({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className={`${card} p-4`}>
      <p className={`text-xs ${muted}`}>{label}</p>
      <p className="mt-2 text-2xl font-bold">{value}</p>
      <p className={`mt-1 text-xs ${muted}`}>{detail}</p>
    </div>
  );
}
