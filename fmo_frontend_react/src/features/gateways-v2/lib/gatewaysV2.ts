import type { GatewayV2Registry, GatewayV2RoutingInfo, GatewayWindow } from '../../../types/api';
import { compareGatewayStatus } from '../../../utils/gateways';
import { formatMsatsAsSats, shortMiddle } from '../../../utils/format';
import { formatUtcDateTime } from '../../../utils/time';

export type GatewayV2 = GatewayV2Registry['gateways'][number];
export type GatewayV2Status = GatewayV2['status'];

export const GATEWAY_V2_WINDOWS: readonly GatewayWindow[] = ['24h', '7d', '30d', '90d'];
// 'all' means every registered gateway; removed ones have their own filter
export const GATEWAY_V2_FILTERS = ['all', 'online', 'degraded', 'offline', 'unknown', 'removed'] as const;
export type GatewayV2Filter = (typeof GATEWAY_V2_FILTERS)[number];
export const GATEWAY_V2_SORTS = ['name', 'status', 'latency', 'activity'] as const;
export type GatewayV2Sort = (typeof GATEWAY_V2_SORTS)[number];

export const STATUS_COLORS: Record<GatewayV2Status, string> = {
  online: 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-300',
  degraded: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-300',
  offline: 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300',
  unknown: 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300',
  removed: 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300',
};

/** The gateway's Lightning alias, or a short form of its key or endpoint */
export function gatewayName(gateway: GatewayV2): string {
  return gateway.routing_info?.lightning_alias || `Gateway ${shortMiddle(gateway.routing_info?.module_public_key ?? gateway.api_endpoint.replace(/^\w+:\/\//, ''), 8, 6)}`;
}

export function observedDate(value: string | null): string {
  return formatUtcDateTime(value, 'Not yet observed');
}

export function formatFee(value?: GatewayV2RoutingInfo['receive_fee']): string {
  return value ? `${formatMsatsAsSats(value.base)} + ${value.parts_per_million.toLocaleString()} ppm` : 'Awaiting node details';
}

/** The gateways of one page matching the filter and search, in the chosen order */
export function visibleGateways(gateways: GatewayV2[], filter: GatewayV2Filter, search: string, sort: GatewayV2Sort): GatewayV2[] {
  const query = search.trim().toLowerCase();
  const visible = gateways.filter((gateway) => (filter === 'all' ? gateway.registered : gateway.status === filter)
    && `${gatewayName(gateway)} ${gateway.routing_info?.lightning_public_key ?? ''} ${gateway.api_endpoint}`.toLowerCase().includes(query));
  const contracts = (gateway: GatewayV2) => gateway.activity.incoming_contracts + gateway.activity.outgoing_contracts;
  return visible.sort((left, right) => sort === 'latency'
    ? (left.latency_ms ?? Infinity) - (right.latency_ms ?? Infinity)
    : sort === 'activity'
      ? contracts(right) - contracts(left)
      : sort === 'status'
        ? compareGatewayStatus(left.status, right.status) || gatewayName(left).localeCompare(gatewayName(right))
        : gatewayName(left).localeCompare(gatewayName(right)));
}
