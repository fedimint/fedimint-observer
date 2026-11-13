import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import type { FederationSummary } from '../../../types/api';
import type { FederationConfig } from '../../../utils/config';
import type { GatewayPageHandoff } from '../../../utils/gateways';
import { Badge } from '../../../components/ui/Badge';
import { Panel } from '../../../components/ui/Panel';
import { InviteCode } from './InviteCode';

interface FederationInfoPanelProps {
  id: string;
  federation: FederationSummary;
  config: FederationConfig | null;
  /** Whether to offer the invite code; joining fails while every guardian is offline */
  showInvite: boolean;
  /** Whether the guardians could not be checked, so the invite may or may not work */
  guardiansUnchecked: boolean;
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <div className="text-xs sm:text-sm text-gray-500 dark:text-gray-400 mb-1">{label}</div>
      {children}
    </div>
  );
}

/** Network, modules and deposit confirmations, links to the gateway pages, and the invite code */
export function FederationInfoPanel({ id, federation, config, showInvite, guardiansUnchecked }: FederationInfoPanelProps) {
  const hasLightningV1 = config?.modules.includes('ln') ?? false;
  const hasLightningV2 = config?.modules.includes('lnv2') ?? false;
  const shuttingDown = Boolean(federation.expiry_timestamp);

  return (
    <Panel title="Federation">
      <div className="space-y-3 sm:space-y-4">
        <Field label="Network">
          <div className="text-sm sm:text-base text-gray-900 dark:text-white">
            {config?.network || 'Unknown'}
          </div>
        </Field>
        <Field label="Modules">
          <div className="flex gap-2 flex-wrap">
            {config?.modules.map((module) => (
              <Badge key={module} level="info">
                {module}
              </Badge>
            ))}
          </div>
        </Field>
        <Field label="Confirmations Required">
          <div className="text-sm sm:text-base text-gray-900 dark:text-white">
            {config?.confirmationsRequired || 'N/A'}
          </div>
        </Field>
        {hasLightningV1 && (
          <div>
            <Link
              to={`/federations/${id}/gateways`}
              className="inline-flex items-center justify-center w-full px-4 py-2.5 text-sm font-medium text-white bg-blue-700 hover:bg-blue-800 rounded-lg focus:ring-4 focus:outline-hidden focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800"
            >
              Gateway Details
            </Link>
          </div>
        )}
        {hasLightningV2 && (
          <Link to={`/federations/${id}/gateways/v2`} state={{ id, name: federation.name ?? null, config: config?.raw } satisfies GatewayPageHandoff} className="inline-flex w-full items-center justify-center rounded-lg bg-blue-700 px-4 py-2.5 text-sm font-medium text-white hover:bg-blue-800 dark:bg-blue-600 dark:hover:bg-blue-700">
            Lightning v2 Gateway Details
          </Link>
        )}
        {federation.invite && showInvite && !shuttingDown && (
          <div>
            <div className="text-xs sm:text-sm text-gray-500 dark:text-gray-400 mb-2">Invite Link</div>
            {guardiansUnchecked && (
              <p className="mb-2 text-xs text-yellow-700 dark:text-yellow-300">
                Guardian status could not be checked, so whether this federation accepts new users right now is unknown.
              </p>
            )}
            <InviteCode invite={federation.invite} />
          </div>
        )}
      </div>
    </Panel>
  );
}
