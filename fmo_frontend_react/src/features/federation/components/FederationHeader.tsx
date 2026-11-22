import { useLayoutEffect, useRef, useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import type { FederationSummary } from '../../../types/api';
import type { FederationConfig } from '../../../utils/config';
import { PageHeader } from '../../../components/ui/PageHeader';
import { StatusPill, type Tone } from '../../../components/ui/Status';
import { KeyChip } from '../../../components/ui/KeyChip';

const HEALTH: Record<FederationSummary['health'], { tone: Tone; label: string; title: string }> = {
  online: { tone: 'ok', label: 'Online', title: 'More guardians answer the observer than consensus needs' },
  degraded: { tone: 'warn', label: 'Degraded', title: 'Exactly as many guardians answer the observer as consensus needs' },
  offline: { tone: 'crit', label: 'Offline', title: 'Fewer guardians answer the observer than consensus needs' },
};

interface FederationHeaderProps {
  id: string;
  federation: FederationSummary;
  config: FederationConfig | null;
}

/** Where the selected tab sits in the tab row, for the underline that slides to it */
function useTabIndicator(tabCount: number) {
  const list = useRef<HTMLUListElement>(null);
  const { pathname } = useLocation();
  const [indicator, setIndicator] = useState<{ left: number; width: number } | null>(null);
  // Slides only between tabs, never in from the edge on first paint
  const [animate, setAnimate] = useState(false);

  useLayoutEffect(() => {
    const row = list.current;
    if (!row) return;
    const measure = () => {
      const active = row.querySelector<HTMLElement>('a[aria-current="page"]');
      setIndicator(active ? { left: active.offsetLeft, width: active.offsetWidth } : null);
    };
    measure();
    // A tab scrolled out of view on a narrow screen is brought back when chosen
    row.querySelector<HTMLElement>('a[aria-current="page"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    const frame = requestAnimationFrame(() => setAnimate(true));
    const resize = new ResizeObserver(measure);
    resize.observe(row);
    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
    };
  }, [pathname, tabCount]);

  return { list, indicator, animate };
}

/** The federation's name, health and section tabs, kept while its sections change */
export function FederationHeader({ id, federation, config }: FederationHeaderProps) {
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
  const { list, indicator, animate } = useTabIndicator(tabs.length);

  return (
    <div className="mb-5">
      <PageHeader
        eyebrow="Federation"
        title={federation.name || 'Unnamed Federation'}
        badges={<>
          <StatusPill tone={health.tone} title={health.title} size="md">{health.label}</StatusPill>
          {config && config.network !== 'unknown' && <StatusPill tone="neutral" icon={null} size="md">{config.network}</StatusPill>}
        </>}
        description={<KeyChip value={id} label="federation id" head={10} tail={8} />}
      />
      <nav aria-label="Federation sections" className="-mt-1 overflow-x-auto border-b border-line scrollbar-thin">
        <ul ref={list} className="relative flex gap-1">
          {tabs.map((tab) => (
            <li key={tab.to}>
              <NavLink
                to={tab.to}
                end={tab.end}
                className={({ isActive }) =>
                  // One weight for every tab, so labels never shift when the selection moves
                  `relative inline-flex items-center rounded-t-md px-3 py-2.5 text-sm font-medium whitespace-nowrap transition-colors duration-200 ${
                    isActive ? 'text-ink' : 'text-ink-3 hover:bg-surface-2 hover:text-ink'
                  }`
                }
              >
                {tab.label}
              </NavLink>
            </li>
          ))}
          {indicator && (
            <li
              aria-hidden="true"
              className={`pointer-events-none absolute bottom-0 left-0 h-0.5 rounded-full bg-accent ${
                animate ? 'transition-[transform,width] duration-300 ease-out motion-reduce:transition-none' : ''
              }`}
              style={{ width: indicator.width, transform: `translateX(${indicator.left}px)` }}
            />
          )}
        </ul>
      </nav>
    </div>
  );
}
