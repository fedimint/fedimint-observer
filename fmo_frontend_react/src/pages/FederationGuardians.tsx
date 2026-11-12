import { useMemo, useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import { Card, CardHeader } from '../components/ui/Card';
import { DataStamp } from '../components/ui/DataStamp';
import { KeyChip } from '../components/ui/KeyChip';
import { Icon } from '../components/ui/Icon';
import { Segmented } from '../components/ui/Segmented';
import { EmptyState, ErrorState, LoadingBlock } from '../components/ui/States';
import { formatUtcDateTime } from '../utils/time';
import { FederationHeader } from '../features/federation/components/FederationHeader';
import { FederationPageError, FederationPageLoading } from '../features/federation/components/FederationPageState';
import { useFederationDetails } from '../features/federation/hooks/useFederationDetails';
import { useGuardianObservability } from '../features/guardians/hooks/useGuardianObservability';
import { GuardianTable } from '../features/guardians/components/GuardianTable';
import { GuardianDetail } from '../features/guardians/components/GuardianDetail';
import { ConnectivityMatrix } from '../features/guardians/components/ConnectivityMatrix';
import { SeatBindingsPill } from '../features/guardians/components/GuardianBits';
import { seatBindingsExplanation } from '../features/guardians/lib/seatBindings';
import type { GuardianRow } from '../features/guardians/lib/guardianModel';

type Filter = 'all' | 'online' | 'offline' | 'issues';

function matches(row: GuardianRow, filter: Filter, search: string): boolean {
  if (filter === 'online' && row.state !== 'online') return false;
  if (filter === 'offline' && row.state === 'online') return false;
  if (filter === 'issues' && row.warnings.length === 0) return false;
  const needle = search.trim().toLowerCase();
  if (!needle) return true;
  return [String(row.id), row.configName, row.url, row.version, row.binding?.display_name, row.binding?.fman_pubkey]
    .some((value) => value?.toLowerCase().includes(needle));
}

export function FederationGuardians() {
  const { id = '' } = useParams<{ id: string }>();
  const details = useFederationDetails(id);
  const config = details.data?.config ?? null;
  const federation = details.data?.federation ?? null;
  const observability = useGuardianObservability(id, config);
  const { rows, live, seatBindings, matrix, history7d } = observability;
  const [searchParams, setSearchParams] = useSearchParams();
  const [filter, setFilter] = useState<Filter>('all');
  const [search, setSearch] = useState('');

  const selectedId = searchParams.get('guardian');
  const selected = rows.find((row) => String(row.id) === selectedId) ?? null;
  const select = (guardianId: number | null) => {
    const next = new URLSearchParams(searchParams);
    if (guardianId === null) next.delete('guardian');
    else next.set('guardian', String(guardianId));
    setSearchParams(next, { replace: guardianId === null });
  };
  const filtered = useMemo(() => rows.filter((row) => matches(row, filter, search)), [rows, filter, search]);
  const counts = {
    online: rows.filter((r) => r.state === 'online').length,
    offline: rows.filter((r) => r.state !== 'online').length,
    issues: rows.filter((r) => r.warnings.length > 0).length,
  };

  if (details.loading && !details.data) return <FederationPageLoading />;
  if (details.failed || !federation) {
    return <FederationPageError error={details.error} notFound={!details.failed || (details.error instanceof Error && details.error.message === 'Federation not found')} onRetry={details.reload} />;
  }

  const broadcastKeys = config?.raw.global?.broadcast_public_keys ?? {};

  return (
    <div>
      <FederationHeader id={id} federation={federation} config={config} />

      <div className="space-y-4 sm:space-y-5">
        <Card aria-labelledby="guardians-title" padding="p-0">
          <div className="p-4 pb-3 sm:p-5 sm:pb-3">
            <CardHeader
              id="guardians-title"
              title="Guardians"
              description="Select a guardian for its health timeline, peer view and identity details"
              meta={<DataStamp kind="live" at={live.data?.generated_at} staleAfterSeconds={180} />}
              actions={
                <>
                  <label className="relative w-full sm:w-auto">
                    <span className="sr-only">Search guardians</span>
                    <Icon name="filter" className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-ink-3" />
                    <input
                      type="search"
                      value={search}
                      onChange={(event) => setSearch(event.target.value)}
                      placeholder="Name, id, version, key…"
                      className="h-8 w-full rounded-lg border sm:w-48 border-line bg-surface-2 pr-2 pl-8 text-xs text-ink placeholder:text-ink-3 focus:border-accent focus:ring-0"
                    />
                  </label>
                  <Segmented
                    label="Show guardians"
                    value={filter}
                    onChange={setFilter}
                    options={[
                      { value: 'all', label: `All ${rows.length}` },
                      { value: 'online', label: `Online ${counts.online}` },
                      { value: 'offline', label: `Not online ${counts.offline}` },
                      { value: 'issues', label: `Issues ${counts.issues}` },
                    ]}
                  />
                </>
              }
            />
          </div>
          <div className="px-2 pb-3 sm:px-3">
            {live.failed && !live.data && (
              <div className="px-2 pb-3">
                <ErrorState title="Could not load the live guardian status" message="Status and latency are unknown until it loads." onRetry={live.reload} />
              </div>
            )}
            {!config || (live.loading && !live.data) ? (
              <LoadingBlock label="Loading guardians" rows={Math.max(config?.guardians.length ?? 4, 3)} className="px-2" />
            ) : filtered.length === 0 ? (
              <div className="px-2"><EmptyState title="No guardian matches">Try another filter or search.</EmptyState></div>
            ) : (
              <GuardianTable rows={filtered} onSelect={select} />
            )}
          </div>
        </Card>

        <div className="grid gap-4 sm:gap-5 xl:grid-cols-2">
          <Card aria-labelledby="connectivity-title">
            <CardHeader
              id="connectivity-title"
              title="Peer connectivity"
              description="Each answering guardian's own report of its P2P connections"
            />
            {live.data ? <ConnectivityMatrix matrix={matrix} /> : <LoadingBlock label="Loading peer connectivity" rows={3} />}
          </Card>

          <Card aria-labelledby="bindings-title">
            <CardHeader
              id="bindings-title"
              title={<span className="inline-flex items-center gap-2">Manifold seat bindings <span className="rounded bg-manifold-soft px-1.5 py-px text-[0.6rem] font-semibold uppercase tracking-wide text-manifold-ink">Manifold</span></span>}
              description="fedi:fman_seat_bindings from the guardians' consensus metadata, verified by the observer"
              meta={seatBindings.data?.observed_at ? <DataStamp kind="live" at={seatBindings.data.observed_at} staleAfterSeconds={2 * 3600} /> : undefined}
            />
            {seatBindings.failed ? (
              <ErrorState title="Could not load the seat-binding check" onRetry={seatBindings.reload} />
            ) : !seatBindings.data ? (
              <LoadingBlock label="Loading seat bindings" rows={3} />
            ) : (
              <div className="space-y-3">
                <SeatBindingsPill report={seatBindings.data} />
                <p className="text-xs leading-relaxed text-ink-3">{seatBindingsExplanation(seatBindings.data)}</p>
                {seatBindings.data.status === 'verified' && (
                  <div className="overflow-x-auto scrollbar-thin">
                    <table className="w-full min-w-[480px] text-xs">
                      <caption className="sr-only">Verified seat bindings</caption>
                      <thead className="text-left text-[0.68rem] uppercase tracking-wide text-ink-3">
                        <tr>
                          <th scope="col" className="py-1.5 pr-3 font-medium">Seat</th>
                          <th scope="col" className="py-1.5 pr-3 font-medium">Manifold Guardian</th>
                          <th scope="col" className="py-1.5 pr-3 font-medium">Key</th>
                          <th scope="col" className="py-1.5 font-medium">Signed</th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-line">
                        {seatBindings.data.bindings.map((binding) => (
                          <tr key={binding.peer_id}>
                            <td className="py-2 pr-3 font-mono text-ink-2">{binding.peer_id}</td>
                            <td className="py-2 pr-3">
                              <Link to={`/manifold/guardians/${binding.fman_pubkey}`} className="font-mono text-manifold-ink hover:underline">{binding.display_name}</Link>
                            </td>
                            <td className="py-2 pr-3"><KeyChip value={binding.fman_pubkey} label={`Manifold Guardian key of seat ${binding.peer_id}`} /></td>
                            <td className="py-2 text-ink-3">{formatUtcDateTime(binding.issued_at * 1000)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </div>
            )}
          </Card>
        </div>
      </div>

      {selected && (
        <GuardianDetail
          row={selected}
          federation={{ id, name: federation.name || 'Unnamed Federation' }}
          broadcastKey={broadcastKeys[String(selected.id)] ?? null}
          history7d={history7d.data}
          matrix={matrix}
          seatBindings={seatBindings.data}
          liveAt={live.data?.generated_at ?? null}
          onClose={() => select(null)}
        />
      )}
    </div>
  );
}
