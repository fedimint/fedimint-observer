import type { ReactNode } from 'react';
import type { FederationSummary } from '../../../types/api';
import type { FederationConfig } from '../../../utils/config';
import { Card, CardHeader } from '../../../components/ui/Card';
import { InviteCode } from './InviteCode';

interface FederationInfoPanelProps {
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
      <dt className="mb-1 text-xs text-ink-3">{label}</dt>
      <dd className="text-sm text-ink">{children}</dd>
    </div>
  );
}

/** Modules, deposit confirmations and the invite code; network and gateway pages are in the header */
export function FederationInfoPanel({ federation, config, showInvite, guardiansUnchecked }: FederationInfoPanelProps) {
  const shuttingDown = Boolean(federation.expiry_timestamp);

  return (
    // Fills its grid cell, so it matches the availability card beside it
    <Card aria-labelledby="federation-info-title" className="flex h-full flex-col">
      <CardHeader id="federation-info-title" title="Federation" />
      <dl className="space-y-3">
        <Field label="Modules">
          <div className="flex flex-wrap gap-1.5">
            {config?.modules.map((module) => (
              <span key={module} className="rounded-md bg-surface-3 px-2 py-0.5 font-mono text-xs text-ink-2">{module}</span>
            ))}
          </div>
        </Field>
        <Field label="Confirmations required">
          <span className="tabular">{config?.confirmationsRequired || 'N/A'}</span>
        </Field>
      </dl>
      {federation.invite && showInvite && !shuttingDown && (
        <div className="mt-4 flex flex-1 flex-col justify-end border-t border-line pt-4">
          <div className="mb-2 text-xs text-ink-3">Invite link</div>
          {guardiansUnchecked && (
            <p className="mb-2 text-xs text-warn-ink">
              Guardian status could not be checked, so whether this federation accepts new users right now is unknown.
            </p>
          )}
          <InviteCode invite={federation.invite} />
        </div>
      )}
    </Card>
  );
}
