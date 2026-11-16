// Pure helpers for the combined gateway page: joining each Lightning v1
// registration to its numbers from the v1 overview, and naming and ordering
// nodes. No React, so they can be tested alone.
//
// The two protocols' numbers measure different things and stay side by side:
// Lightning v1 uptime is presence in the guardians' registry across polls,
// Lightning v2 reachability is the share of probes the endpoint answered.
import type { GatewayActivityMetrics, GatewayInfo, GatewayLnv1Registration, GatewayLnv2Registration, GatewayNode, GatewayRegistration } from '../../../types/api';
import { ApiError } from '../../../services/http.ts';
import { shortMiddle } from '../../../utils/format.ts';

/** The Lightning v1 overview's gateways by id; empty when it is missing, such as without the Lightning v1 module */
export function lnv1ById(gateways: GatewayInfo[] | undefined | null): Map<string, GatewayInfo> {
  return new Map((gateways ?? []).map((gateway) => [gateway.gateway_id, gateway]));
}

export function lnv1Registrations(node: GatewayNode): GatewayLnv1Registration[] {
  return node.registrations.filter((registration): registration is GatewayLnv1Registration => registration.protocol === 'lnv1');
}

export function lnv2Registrations(node: GatewayNode): GatewayLnv2Registration[] {
  return node.registrations.filter((registration): registration is GatewayLnv2Registration => registration.protocol === 'lnv2');
}

/** Unique within a response: a v1 gateway id, or a v2 endpoint within its module instance */
export function registrationKey(registration: GatewayRegistration): string {
  return registration.protocol === 'lnv1'
    ? `lnv1:${registration.gateway_id}`
    : `lnv2:${registration.module_instance_id}:${registration.api_endpoint}`;
}

/** The alias a Lightning v1 registration of the node announces, or a short form of its key */
export function nodeName(node: GatewayNode, lnv1: Map<string, GatewayInfo>): string {
  const alias = lnv1Registrations(node).map((registration) => lnv1.get(registration.gateway_id)?.lightning_alias).find(Boolean);
  return alias || `Node ${shortMiddle(node.node_pub_key, 8, 6)}`;
}

/** Share of successful probes in percent, or null without any */
export function reachabilityPct(samples: number, onlineSamples: number): number | null {
  return samples > 0 ? onlineSamples / samples * 100 : null;
}

/** Successful probes in percent, with how many there were */
export function formatReachability(samples: number, onlineSamples: number): string {
  const pct = reachabilityPct(samples, onlineSamples);
  return pct === null ? 'No probe history' : `${pct.toFixed(1)}% of ${samples.toLocaleString()} probes`;
}

export interface Lnv1Numbers {
  /**
   * Estimated registry uptime in percent, or null when it wasn't measured: the
   * overview lacks the gateway, or no poll in the window sampled it
   */
  uptimePct: number | null;
  /** Share of the window the polls observed, in percent, when the uptime was measured */
  coveragePct: number | null;
  activity: GatewayActivityMetrics | null;
}

/** A Lightning v1 registration's numbers for the window, from the v1 overview */
export function lnv1Numbers(registration: GatewayLnv1Registration, lnv1: Map<string, GatewayInfo>): Lnv1Numbers {
  const gateway = lnv1.get(registration.gateway_id);
  // Without sampled time the server still reports 0% uptime, so coverage
  // tells a measured 0% from none, as in the Lightning v1 table
  const measured = (gateway?.coverage_pct ?? 0) > 0;
  return {
    uptimePct: measured ? gateway?.estimated_uptime_pct ?? null : null,
    coveragePct: measured ? gateway?.coverage_pct ?? null : null,
    activity: gateway?.activity_window ?? null,
  };
}

/** Whether a Lightning v2 registration claims the node without a Lightning v1 registration announcing its endpoint */
export function hasUnverified(node: GatewayNode): boolean {
  return lnv2Registrations(node).some((registration) => !registration.url_matches_lnv1);
}

/** Ordered by name, then key, so the order holds across refreshes */
export function sortNodes(nodes: GatewayNode[], lnv1: Map<string, GatewayInfo>): GatewayNode[] {
  return [...nodes].sort((left, right) => nodeName(left, lnv1).localeCompare(nodeName(right, lnv1))
    || left.node_pub_key.localeCompare(right.node_pub_key));
}

/** Whether the observer has no such route or federation: a 404 */
export function isMissingRoute(error: unknown): boolean {
  return error instanceof ApiError && error.status === 404;
}
