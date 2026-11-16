import { useMemo } from 'react';
import { api } from '../../../services/api';
import type { GatewayWindow } from '../../../types/api';
import { useApiResource } from '../../../hooks/useApiResource';
import { lnv1ById } from '../lib/gatewayNodes';

/**
 * A federation's gateways grouped by node, and the Lightning v1 overview for
 * the same window, which holds the v1 registrations' numbers. Without the v1
 * overview (no Lightning v1 module, or it failed) the v1 numbers are missing.
 */
export function useGatewayNodes(id: string, timeWindow: GatewayWindow) {
  const nodes = useApiResource((signal) => api.getGatewayNodes(id, timeWindow, signal), [id, timeWindow], {
    resetOnReload: true,
    label: 'gateway nodes',
  });
  const overview = useApiResource((signal) => api.getFederationGatewayOverview(id, timeWindow, signal), [id, timeWindow], {
    resetOnReload: true,
    label: 'gateway overview',
  });
  const lnv1 = useMemo(() => lnv1ById(overview.data?.gateways), [overview.data]);
  return { nodes, lnv1, overview };
}
