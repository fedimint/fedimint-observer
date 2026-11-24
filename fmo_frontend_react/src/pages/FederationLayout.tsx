import { Suspense, useEffect, useMemo } from 'react';
import { Outlet, useLocation, useParams } from 'react-router-dom';
import { ErrorBoundary, ReloadMessage } from '../components/ui/ErrorBoundary';
import { Skeleton } from '../components/ui/States';
import { FederationHeader } from '../features/federation/components/FederationHeader';
import { FederationPageError, FederationPageLoading } from '../features/federation/components/FederationPageState';
import { useFederationDetails } from '../features/federation/hooks/useFederationDetails';
import type { FederationContext } from '../features/federation/hooks/useFederation';
import { prefetchFederationPages } from './federationPages';

/** Wait before fetching other sections' code, so it never competes with this page's data */
const PREFETCH_DELAY_MS = 1500;

function SectionLoading() {
  return (
    <div role="status" aria-label="Loading section" className="space-y-4">
      <div className="grid gap-3 md:grid-cols-3">
        <Skeleton className="h-28 w-full rounded-xl" />
        <Skeleton className="h-28 w-full rounded-xl" />
        <Skeleton className="h-28 w-full rounded-xl" />
      </div>
      <Skeleton className="h-64 w-full rounded-xl" />
    </div>
  );
}


export function FederationLayout() {
  // The router remounts this layout per federation (App.tsx), so id never changes here
  const { id = '' } = useParams<{ id: string }>();
  const { pathname } = useLocation();
  const details = useFederationDetails(id);

  useEffect(() => {
    const timer = window.setTimeout(prefetchFederationPages, PREFETCH_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  const context = useMemo<FederationContext | null>(
    () => (details.data ? { id, ...details.data } : null),
    [id, details.data],
  );

  if (details.loading && !context) return <FederationPageLoading />;
  if (!context) {
    const notFound = details.error instanceof Error && details.error.message === 'Federation not found';
    return <FederationPageError error={details.error} notFound={notFound || !details.failed} onRetry={details.reload} />;
  }

  return (
    <div>
      <FederationHeader id={id} federation={context.federation} config={context.config} />
      <ErrorBoundary key={pathname} fallback={<ReloadMessage message="Something went wrong while showing this section." />}>
        <Suspense fallback={<SectionLoading />}>

          <div key={pathname} className="fmo-section-enter">
            <Outlet context={context} />
          </div>
        </Suspense>
      </ErrorBoundary>
    </div>
  );
}
