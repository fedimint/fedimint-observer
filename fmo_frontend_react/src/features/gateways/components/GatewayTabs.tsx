import { NavLink } from 'react-router-dom';
import type { ClientConfig } from '../../../types/api';
import type { GatewayPageHandoff } from '../../../utils/gateways';

/**
 * Links between a federation's gateway pages: one per Lightning module, and
 * the view by node, which needs the Lightning v2 module. Nothing while the
 * config loads, or when there is only one page to show.
 */
export function GatewayTabs({ id, name, config }: { id: string; name: string | null; config: ClientConfig | null }) {
  const kinds = Object.values(config?.modules ?? {}).map((module) => module.kind);
  const hasLightningV2 = kinds.includes('lnv2');
  const tabs = [
    kinds.includes('ln') && { path: 'gateways', label: 'Lightning v1' },
    hasLightningV2 && { path: 'gateways/v2', label: 'Lightning v2' },
    hasLightningV2 && { path: 'gateways/nodes', label: 'By node' },
  ].filter((tab) => tab !== false);
  if (!config || tabs.length < 2) return null;

  return (
    <nav aria-label="Gateway views" className="mb-4 flex gap-1 overflow-x-auto border-b border-gray-200 dark:border-gray-700">
      {tabs.map(({ path, label }) => (
        <NavLink
          key={path}
          end
          to={`/federations/${id}/${path}`}
          state={{ id, name, config } satisfies GatewayPageHandoff}
          className={({ isActive }) => `-mb-px whitespace-nowrap border-b-2 px-3 py-2 text-sm font-medium ${isActive
            ? 'border-blue-600 text-blue-700 dark:border-blue-400 dark:text-blue-300'
            : 'border-transparent text-gray-600 hover:border-gray-300 hover:text-gray-900 dark:text-gray-400 dark:hover:text-white'}`}
        >
          {label}
        </NavLink>
      ))}
    </nav>
  );
}
