import type { GatewayV2Registry } from '../types/api';

type Status = GatewayV2Registry['gateways'][number]['status'];

/** Healthiest first, as "Sort by status" reads */
const STATUS_ORDER: Record<Status, number> = { online: 0, degraded: 1, offline: 2, unknown: 3, removed: 4 };

export function compareGatewayStatus(left: Status, right: Status): number {
  return STATUS_ORDER[left] - STATUS_ORDER[right];
}
