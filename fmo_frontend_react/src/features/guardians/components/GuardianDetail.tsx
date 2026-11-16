import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { GuardianHealthHistory, ManifoldSeatBindingsReport } from '../../../types/api';
import { Drawer } from '../../../components/ui/Drawer';
import { DataStamp } from '../../../components/ui/DataStamp';
import { KeyChip } from '../../../components/ui/KeyChip';
import { Icon } from '../../../components/ui/Icon';
import { Skeleton } from '../../../components/ui/States';
import { StatusPill } from '../../../components/ui/Status';
import { formatUtcDateTime } from '../../../utils/time';
import type { ConnectivityMatrix, GuardianRow } from '../lib/guardianModel';
import { formatMs } from '../lib/format';
import { HeatmapGrid, HeatmapLegend } from './AvailabilityHeatmap';
import { GuardianStatePill } from './GuardianBits';
import { seatBindingsExplanation } from '../lib/seatBindings';
import { OperationalWarnings } from './OperationalWarnings';

function Section({ title, children, meta }: { title: string; children: ReactNode; meta?: ReactNode }) {
  return (
    <section className="rounded-xl border border-line bg-surface p-4">
      <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-semibold text-ink">{title}</h3>
        {meta}
      </div>
      {children}
    </section>
  );
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[8.5rem_minmax(0,1fr)] items-start gap-3 py-1.5 text-xs">
      <dt className="text-ink-3">{label}</dt>
      <dd className="min-w-0 text-ink">{children}</dd>
    </div>
  );
}

interface GuardianDetailProps {
  row: GuardianRow;
  federation: { id: string; name: string };
  broadcastKey: string | null;
  history7d: GuardianHealthHistory | null;
  matrix: ConnectivityMatrix;
  seatBindings: ManifoldSeatBindingsReport | null;
  liveAt: string | null;
  onClose: () => void;
}

export function GuardianDetail({ row, federation, broadcastKey, history7d, matrix, seatBindings, liveAt, onClose }: GuardianDetailProps) {
  const ownView = row.live?.peers ?? [];
  const othersView = matrix.reporters
    .filter((reporter) => reporter !== row.id)
    .map((reporter) => ({ reporter, cell: matrix.cell(reporter, row.id) }));

  return (
    <Drawer
      title={row.configName ? `Guardian ${row.id} · ${row.configName}` : `Guardian ${row.id}`}
      subtitle={<span className="inline-flex items-center gap-2"><GuardianStatePill state={row.state} /> {federation.name}</span>}
      onClose={onClose}
    >
      {row.warnings.length > 0 && <OperationalWarnings warnings={row.warnings} limit={6} />}

      <Section title="Identity">
        <dl className="divide-y divide-line">
          <Field label="Fedimint peer id"><span className="font-mono">{row.id}</span></Field>
          <Field label="Name in config">{row.configName ?? <span className="text-ink-3">Not set</span>}</Field>
          <Field label="API endpoint"><span className="break-all font-mono text-[0.72rem] text-ink-2">{row.url}</span></Field>
          <Field label="Guardian key">
            {broadcastKey ? <KeyChip value={broadcastKey} label="guardian broadcast public key" /> : <span className="text-ink-3">Not in config</span>}
          </Field>
          <Field label="Manifold Guardian">
            {row.binding ? (
              <div className="space-y-1.5">
                <Link to={`/manifold/guardians/${row.binding.fman_pubkey}`} className="inline-flex items-center gap-1 font-mono text-manifold-ink hover:underline">
                  <Icon name="key" className="size-3" />
                  {row.binding.display_name}
                </Link>
                <div><KeyChip value={row.binding.fman_pubkey} label="Manifold Guardian public key" /></div>
                <div className="text-ink-3">Bound {formatUtcDateTime(row.binding.issued_at * 1000)}</div>
              </div>
            ) : (
              <span className="text-ink-3">{seatBindings?.status === 'verified' ? 'Not bound' : 'No verified seat binding'}</span>
            )}
          </Field>
          <Field label="Operator">
            <span className="text-ink-3">
              {row.binding
                ? 'Not established. The operator is the trust-credential holder behind the Manifold Guardian; the observer does not verify credentials yet.'
                : 'Not established. The observer only knows operators through verified Manifold seat bindings and credentials.'}
            </span>
          </Field>
        </dl>
        {!row.binding && <p className="mt-2 text-[0.7rem] leading-relaxed text-ink-3">{seatBindingsExplanation(seatBindings)}</p>}
      </Section>

      <Section title="Block height" meta={<DataStamp kind="live" at={liveAt} staleAfterSeconds={180} />}>
        <p className="text-xs text-ink">
          {row.blockHeight === null ? '—' : (
            <span className="inline-flex items-center gap-2 tabular">
              {row.blockHeight.toLocaleString('en-US')}
              {row.blockOutdated && <StatusPill tone="warn">Behind</StatusPill>}
            </span>
          )}
        </p>
      </Section>

      <Section title="Health timeline" meta={<DataStamp kind="historical" at={history7d?.generated_at} scope="7d" />}>
        <p className="mb-3 text-xs text-ink-3">30-day mean latency <span className="tabular text-ink">{formatMs(row.avgLatency30d)}</span></p>
        {history7d ? (
          <>
            <HeatmapGrid history={history7d} rows={[row]} compact />
            <div className="mt-2"><HeatmapLegend /></div>
          </>
        ) : <Skeleton className="h-6 w-full" />}
      </Section>

      <Section title="Peer connectivity" meta={<DataStamp kind="live" at={row.live?.answered_at} staleAfterSeconds={300} />}>
        <div className="grid gap-4 sm:grid-cols-2">
          <div>
            <h4 className="mb-2 text-xs font-medium text-ink-2">How this guardian sees its peers</h4>
            {row.state !== 'online' || ownView.length === 0 ? (
              <p className="text-xs text-ink-3">No recent report from this guardian.</p>
            ) : (
              <ul className="space-y-1">
                {ownView.map((peer) => (
                  <li key={peer.peer_id} className="flex items-center justify-between gap-2 text-xs">
                    <span className="text-ink-2">Guardian {peer.peer_id}</span>
                    <StatusPill tone={peer.flagged ? 'warn' : peer.connected ? 'ok' : 'crit'}>
                      {peer.flagged ? 'Flagged' : peer.connected ? 'Connected' : 'Disconnected'}
                    </StatusPill>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div>
            <h4 className="mb-2 text-xs font-medium text-ink-2">How its peers see it</h4>
            {othersView.length === 0 ? (
              <p className="text-xs text-ink-3">No other guardian reported recently.</p>
            ) : (
              <ul className="space-y-1">
                {othersView.map(({ reporter, cell }) => (
                  <li key={reporter} className="flex items-center justify-between gap-2 text-xs">
                    <span className="text-ink-2">Guardian {reporter}</span>
                    {cell === 'no_report' ? <span className="text-ink-3">No report</span> : (
                      <StatusPill tone={cell === 'flagged' ? 'warn' : cell === 'connected' ? 'ok' : 'crit'}>
                        {cell === 'flagged' ? 'Flagged' : cell === 'connected' ? 'Connected' : 'Disconnected'}
                      </StatusPill>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </Section>

    </Drawer>
  );
}
