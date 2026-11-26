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

/** Fetches every section's code, so switching tabs never waits on a download */
export function prefetchFederationPages() {
  for (const load of Object.values(loaders)) {
    load().catch((error: unknown) => console.warn('Could not prefetch a federation page:', error));
  }
}
