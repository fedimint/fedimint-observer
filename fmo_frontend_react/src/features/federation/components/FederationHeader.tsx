import type { ReactNode } from 'react';
import { NavLink } from 'react-router-dom';
import type { FederationSummary } from '../../../types/api';
import type { FederationConfig } from '../../../utils/config';
import { PageHeader } from '../../../components/ui/PageHeader';
import { StatusPill, type Tone } from '../../../components/ui/Status';
import { KeyChip } from '../../../components/ui/KeyChip';

const HEALTH: Record<FederationSummary['health'], { tone: Tone; label: string; title: string }> = {
  online: { tone: 'ok', label: 'Online', title: 'More guardians answer than consensus needs' },
  degraded: { tone: 'warn', label: 'Degraded', title: 'Exactly as many guardians answer as consensus needs' },
  offline: { tone: 'crit', label: 'Offline', title: 'Too few guardians answer to reach consensus' },
};

interface FederationHeaderProps {
  id: string;
  federation: FederationSummary;
  config: FederationConfig | null;
  /** Shown under the title, e.g. what the page is about */
  description?: ReactNode;
  actions?: ReactNode;
}

/** The federation's name, health and section tabs, shared by its pages */
export function FederationHeader({ id, federation, config, description, actions }: FederationHeaderProps) {
  const base = `/federations/${encodeURIComponent(id)}`;
  const health = HEALTH[federation.health];
  const tabs = [
    { to: base, label: 'Overview', end: true },
    { to: `${base}/guardians`, label: 'Guardians' },
    { to: `${base}/reliability`, label: 'Reliability' },
    ...(config?.modules.includes('ln') ? [{ to: `${base}/gateways`, label: 'Gateways', end: true }] : []),
    ...(config?.modules.includes('lnv2') ? [{ to: `${base}/gateways/v2`, label: 'Gateways v2' }] : []),
    { to: `${base}/activity`, label: 'Activity' },
    ...(config?.modules.includes('wallet') ? [{ to: `${base}/utxos`, label: 'UTXOs' }] : []),
    ...(config?.modules.includes('walletv2') ? [{ to: `${base}/assets`, label: 'Wallet Assets' }] : []),
    { to: `${base}/config`, label: 'Config' },
  ];

  return (
    <div className="mb-5">
      <PageHeader
        eyebrow="Federation"
        title={federation.name || 'Unnamed Federation'}
        badges={<>
          <StatusPill tone={health.tone} title={health.title} size="md">{health.label}</StatusPill>
          {config && config.network !== 'unknown' && <StatusPill tone="neutral" icon={null} size="md">{config.network}</StatusPill>}
        </>}
        description={description ?? <KeyChip value={id} label="federation id" head={10} tail={8} />}
        actions={actions}
      />
      <nav aria-label="Federation sections" className="-mt-1 overflow-x-auto border-b border-line scrollbar-thin">
        <ul className="flex gap-1">
          {tabs.map((tab) => (
            <li key={tab.to}>
              <NavLink
                to={tab.to}
                end={tab.end}
                className={({ isActive }) =>
                  `inline-flex items-center border-b-2 px-3 py-2.5 text-sm whitespace-nowrap transition-colors ${
                    isActive ? 'border-accent font-medium text-ink' : 'border-transparent text-ink-3 hover:border-line-strong hover:text-ink'
                  }`
                }
              >
                {tab.label}
              </NavLink>
            </li>
          ))}
        </ul>
      </nav>
    </div>
  );
}
