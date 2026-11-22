import { Navigate, useNavigate, useSearchParams } from 'react-router-dom';
import { ExpiryNotice } from '../features/federation/components/ExpiryNotice';
import { FederationInfoPanel } from '../features/federation/components/FederationInfoPanel';
import { useFederation } from '../features/federation/hooks/useFederation';
import { InfrastructureHealth } from '../features/guardians/components/InfrastructureHealth';
import { AvailabilityHeatmap } from '../features/guardians/components/AvailabilityHeatmap';
import { useGuardianObservability } from '../features/guardians/hooks/useGuardianObservability';

const SECTIONS = ['activity', 'utxos', 'assets', 'config'] as const;

export function FederationDetail() {
  const { id, federation, config, successor } = useFederation();
  const navigate = useNavigate();
  // Sections that used to be tabs here (?tab=utxos) have their own pages now
  const [searchParams] = useSearchParams();
  const legacyTab = SECTIONS.find((section) => section === searchParams.get('tab'));

  const observability = useGuardianObservability(id, config);
  const guardians = observability.health;

  if (legacyTab) return <Navigate replace to={`/federations/${encodeURIComponent(id)}/${legacyTab}`} />;

  const expiry = federation.expiry_timestamp ? new Date(federation.expiry_timestamp * 1000) : null;
  // The invite is hidden while every guardian is known to be offline, since joining
  // would fail. When the guardians could not be checked, the federation list's own
  // health decides: an unknown status is no reason to withhold a working invite.
  const showInvite = guardians.status === 'loaded'
    ? guardians.hasOnlineGuardian
    : guardians.status === 'failed' && federation.health !== 'offline';

  const selectGuardian = (guardianId: number) =>
    navigate(`/federations/${encodeURIComponent(id)}/guardians?guardian=${guardianId}`);

  return (
    <div>
      {expiry && <ExpiryNotice federation={federation} expiry={expiry} successor={successor} />}

      <div className="space-y-4 sm:space-y-5">
        <InfrastructureHealth federationId={id} observability={observability} onSelectGuardian={selectGuardian} />

        {/* Equal heights: both cards stretch to the taller one */}
        <div className="grid grid-cols-1 items-stretch gap-4 sm:gap-5 lg:grid-cols-3">
          <div className="min-w-0 lg:col-span-2">
            <AvailabilityHeatmap federationId={id} rows={observability.rows} className="h-full" />
          </div>
          <div className="min-w-0">
            <FederationInfoPanel
              federation={federation}
              config={config}
              showInvite={showInvite}
              guardiansUnchecked={guardians.status === 'failed'}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
