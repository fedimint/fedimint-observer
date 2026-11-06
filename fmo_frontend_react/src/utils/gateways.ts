import type { ClientConfig, GatewayV2Registry } from '../types/api';

/**
 * Router state the federation page hands to its Lightning v2 gateway page, so
 * that page needs neither the full federation list nor the config again.
 * Direct links arrive without it and fetch both.
 */
export interface GatewayPageHandoff {
  id: string;
  name: string | null;
  config?: ClientConfig;
}

type Status = GatewayV2Registry['gateways'][number]['status'];

/** Healthiest first, as "Sort by status" reads */
const STATUS_ORDER: Record<Status, number> = { online: 0, degraded: 1, offline: 2, unknown: 3, removed: 4 };

export function compareGatewayStatus(left: Status, right: Status): number {
  return STATUS_ORDER[left] - STATUS_ORDER[right];
}
