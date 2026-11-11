import type { ClientConfig } from '../types/api';

export interface ConfigGuardian {
  id: number;
  url: string;
  /** The guardian's name as the federation's config gives it; null when absent */
  name: string | null;
}

// What the pages show of a federation's client config
export interface FederationConfig {
  guardians: ConfigGuardian[];
  modules: string[];
  network: string;
  confirmationsRequired: number;
  raw: ClientConfig;
}

export function parseConfig(config: ClientConfig): FederationConfig {
  const guardians = Object.entries(config.global?.api_endpoints ?? {}).map(([id, endpoint]) => ({
    id: parseInt(id),
    url: endpoint.url || 'Unknown',
    name: endpoint.name?.trim() || null,
  }));

  const moduleConfigs = Object.values(config.modules ?? {});
  const modules = moduleConfigs.map((mod) => mod.kind || 'unknown');

  // Network and confirmations come from the wallet module
  const walletModule = moduleConfigs.find((mod) => mod.kind === 'wallet');
  const networkModule = walletModule ?? moduleConfigs.find((mod) => mod.kind === 'walletv2');

  return {
    guardians,
    modules,
    network: networkModule?.network || 'unknown',
    // A deposit needs finality_delay + 1 confirmations
    confirmationsRequired: walletModule ? (walletModule.finality_delay || 0) + 1 : 0,
    raw: config,
  };
}

// Guardians that must agree for the federation to act (fedimint's NumPeers::threshold):
// all but the (n - 1) / 3 it tolerates being faulty
export function guardianThreshold(guardianCount: number): number {
  return guardianCount - Math.floor(Math.max(guardianCount - 1, 0) / 3);
}
