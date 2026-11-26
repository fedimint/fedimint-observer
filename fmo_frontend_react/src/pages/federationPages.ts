import { lazy } from 'react';

const loaders = {
  layout: () => import('./FederationLayout'),
  detail: () => import('./FederationDetail'),
  section: () => import('./FederationSection'),
  gateways: () => import('./FederationGateways'),
  guardians: () => import('./FederationGuardians'),
  reliability: () => import('./FederationReliability'),
};

export const FederationLayout = lazy(() => loaders.layout().then((m) => ({ default: m.FederationLayout })));
export const FederationDetail = lazy(() => loaders.detail().then((m) => ({ default: m.FederationDetail })));
export const FederationSection = lazy(() => loaders.section().then((m) => ({ default: m.FederationSection })));
export const FederationGateways = lazy(() => loaders.gateways().then((m) => ({ default: m.FederationGateways })));
export const FederationGuardians = lazy(() => loaders.guardians().then((m) => ({ default: m.FederationGuardians })));
export const FederationReliability = lazy(() => loaders.reliability().then((m) => ({ default: m.FederationReliability })));

/** Warm just the tab a user points to or focuses. */
export function prefetchFederationTab(section: string) {
  const connection = (navigator as Navigator & { connection?: { saveData?: boolean; effectiveType?: string } }).connection;
  if (connection?.saveData || connection?.effectiveType?.endsWith('2g')) return;

  const load = section === '' ? loaders.detail
    : section === '/guardians' ? loaders.guardians
    : section === '/reliability' ? loaders.reliability
    : section.startsWith('/gateways') ? loaders.gateways
    : loaders.section;
  load().catch((error: unknown) => console.warn('Could not prefetch the federation tab:', error));
}
