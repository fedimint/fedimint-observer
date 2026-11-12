import { Icon } from '../../../components/ui/Icon';
import { EmptyState } from '../../../components/ui/States';
import type { ConnectivityMatrix as Matrix, PeerCell } from '../lib/guardianModel';

const CELL: Record<PeerCell, { label: string; classes: string; mark: string }> = {
  connected: { label: 'Connected', classes: 'bg-ok-soft text-ok-ink', mark: '✓' },
  disconnected: { label: 'Disconnected', classes: 'bg-crit-soft text-crit-ink', mark: '✕' },
  flagged: { label: 'Flagged: not contributing', classes: 'bg-warn-soft text-warn-ink', mark: '!' },
  self: { label: 'Itself', classes: 'cell-empty text-ink-3', mark: '' },
  no_report: { label: 'No report', classes: 'bg-surface-3 text-ink-3', mark: '–' },
};

/**
 * How each guardian sees its peers' P2P connections, as reported in its own
 * status. Rows report, columns are reported on; a guardian that does not
 * answer has no row.
 */
export function ConnectivityMatrix({ matrix }: { matrix: Matrix }) {
  if (matrix.reporters.length === 0) {
    return (
      <EmptyState icon="signal" title="No peer reports right now">
        Guardians report their peer connections in their status. None of them answered with one in the last 5 minutes.
      </EmptyState>
    );
  }
  return (
    <div>
      <div className="overflow-x-auto scrollbar-thin">
        <table className="text-xs">
          <caption className="sr-only">Peer connectivity as each answering guardian reports it. Rows are the reporting guardians, columns the peers.</caption>
          <thead>
            <tr>
              <th scope="col" className="px-2 py-1.5 text-left font-medium text-ink-3">Reporter ↓ / Peer →</th>
              {matrix.peers.map((peer) => (
                <th key={peer} scope="col" className="min-w-9 px-1 py-1.5 text-center font-mono font-semibold text-ink-2">{peer}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {matrix.reporters.map((reporter) => (
              <tr key={reporter}>
                <th scope="row" className="px-2 py-1 text-left font-medium whitespace-nowrap text-ink-2">Guardian {reporter}</th>
                {matrix.peers.map((peer) => {
                  const cell = CELL[matrix.cell(reporter, peer)];
                  return (
                    <td key={peer} className="p-[2px]">
                      <span title={`Guardian ${reporter} → ${peer}: ${cell.label}`} className={`flex size-8 items-center justify-center rounded-[3px] font-semibold ${cell.classes}`}>
                        <span aria-hidden="true">{cell.mark}</span>
                        <span className="sr-only">{cell.label}</span>
                      </span>
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <ul className="mt-3 flex flex-wrap gap-x-3 gap-y-1 text-[0.7rem] text-ink-3" aria-label="Legend">
        {(['connected', 'disconnected', 'flagged', 'no_report'] as const).map((key) => (
          <li key={key} className="inline-flex items-center gap-1.5">
            <span aria-hidden="true" className={`inline-flex size-4 items-center justify-center rounded-[2px] text-[0.6rem] font-bold ${CELL[key].classes}`}>{CELL[key].mark}</span>
            {CELL[key].label}
          </li>
        ))}
      </ul>
      <p className="mt-2 flex items-start gap-1.5 text-[0.7rem] text-ink-3">
        <Icon name="info" className="mt-px size-3" />
        Reported by the guardians themselves. The observer cannot see the P2P network, only what each guardian says about it.
      </p>
    </div>
  );
}
