import { createElement, lazy, useState, type ComponentType } from 'react';

/** A lazy page that renders at once when its code has arrived, skipping React's 300 ms Suspense reveal delay */
function preloadable<P extends object>(load: () => Promise<ComponentType<P>>) {
  let loaded: ComponentType<P> | undefined;
  let pending: Promise<ComponentType<P>> | undefined;
  const preload = () => (pending ??= load().then(
    (component) => (loaded = component),
    (error: unknown) => {
      pending = undefined;
      throw error;
    },
  ));
  const Lazy = lazy(() => preload().then((component) => ({ default: component })));
  function Page(props: P) {
    const [Component] = useState<ComponentType<P>>(() => loaded ?? Lazy);
    return createElement(Component, props);
  }
  return { Page, preload };
}

const pages = {
  layout: preloadable(() => import('./FederationLayout').then((m) => m.FederationLayout)),
  detail: preloadable(() => import('./FederationDetail').then((m) => m.FederationDetail)),
  section: preloadable(() => import('./FederationSection').then((m) => m.FederationSection)),
  gateways: preloadable(() => import('./FederationGateways').then((m) => m.FederationGateways)),
  guardians: preloadable(() => import('./FederationGuardians').then((m) => m.FederationGuardians)),
  reliability: preloadable(() => import('./FederationReliability').then((m) => m.FederationReliability)),
};

export const FederationLayout = pages.layout.Page;
export const FederationDetail = pages.detail.Page;
export const FederationSection = pages.section.Page;
export const FederationGateways = pages.gateways.Page;
export const FederationGuardians = pages.guardians.Page;
export const FederationReliability = pages.reliability.Page;

/** Start a tab's code download; the open tab calls it so the code arrives alongside its data */
export function loadFederationTab(section: string) {
  const tab = section.replace(/^\//, '');
  const page = tab === '' ? pages.detail
    : tab === 'guardians' ? pages.guardians
    : tab === 'reliability' ? pages.reliability
    : tab.startsWith('gateways') ? pages.gateways
    : pages.section;
  page.preload().catch((error: unknown) => console.warn('Could not load the federation tab:', error));
}

/** Warm just the tab a user points to or focuses. */
export function prefetchFederationTab(section: string) {
  const connection = (navigator as Navigator & { connection?: { saveData?: boolean; effectiveType?: string } }).connection;
  if (connection?.saveData || connection?.effectiveType?.endsWith('2g')) return;
  loadFederationTab(section);
}
