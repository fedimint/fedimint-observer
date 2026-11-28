import type { ClientConfig, FederationSummary } from '../types/api';

export interface ConfigGuardian {
  id: number;
  url: string;

  name: string | null;
}

export interface FederationConfig {
  guardians: ConfigGuardian[];
  modules: string[];
  network: string;
  confirmationsRequired: number;
  raw: ClientConfig;
}

/** Module kind by instance id */
export function kindsByInstance(modules: ClientConfig['modules']): Record<string, string> {
  return Object.fromEntries(Object.entries(modules ?? {}).map(([instance, module]) => [instance, module.kind || 'unknown']));
}

/** Module kind by instance id and network, from the summary's outline or else the config; the kinds stay undefined until either is known */
export function federationOutline(federation: Pick<FederationSummary, 'module_kinds' | 'network'>, config: FederationConfig | null | undefined) {
  return {
    moduleKinds: federation.module_kinds ?? (config ? kindsByInstance(config.raw.modules) : undefined),
    network: config?.network ?? federation.network ?? 'unknown',
  };
}

export function instancesOf(kinds: Record<string, string>, kind: string): number[] {
  return Object.entries(kinds).filter(([, moduleKind]) => moduleKind === kind).map(([instance]) => Number(instance));
}

/** Which config lookup a page shows: the stored one, or the invite lookup once it failed. Retry always restarts the stored one */
export function configSource(storedFailed: boolean, invite: string | undefined): 'stored' | 'invite' {
  return storedFailed && invite ? 'invite' : 'stored';
}

export function parseConfig(config: ClientConfig): FederationConfig {
  const guardians = Object.entries(config.global?.api_endpoints ?? {}).map(([id, endpoint]) => ({
    id: parseInt(id),
    url: endpoint.url || 'Unknown',
    name: endpoint.name?.trim() || null,
  }));

  const moduleConfigs = Object.values(config.modules ?? {});
  const modules = Object.values(kindsByInstance(config.modules));

  const walletModule = moduleConfigs.find((mod) => mod.kind === 'wallet');
  const networkModule = walletModule ?? moduleConfigs.find((mod) => mod.kind === 'walletv2');

  return {
    guardians,
    modules,
    network: networkModule?.network || 'unknown',
    confirmationsRequired: walletModule ? (walletModule.finality_delay || 0) + 1 : 0,
    raw: config,
  };
}

// Guardians that must agree for the federation to act (fedimint's NumPeers::threshold): all but the (n - 1) / 3 it tolerates being faulty
export function guardianThreshold(guardianCount: number): number {
  return guardianCount - Math.floor(Math.max(guardianCount - 1, 0) / 3);
}
