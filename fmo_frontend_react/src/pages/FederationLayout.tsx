import { Suspense, useMemo } from 'react';
import { Outlet, useLocation, useParams } from 'react-router-dom';
import { ErrorBoundary, ReloadMessage } from '../components/ui/ErrorBoundary';
import { ErrorState, Skeleton } from '../components/ui/States';
import { FederationHeader } from '../features/federation/components/FederationHeader';
import { FederationPageError, FederationPageLoading } from '../features/federation/components/FederationPageState';
import { useFederationDetails } from '../features/federation/hooks/useFederationDetails';
import type { FederationContext } from '../features/federation/hooks/useFederation';
import { errorMessage } from '../utils/errors';
import { ApiError } from '../services/http';

/** Sections that render from the summary alone; every other one waits for the config */
const SUMMARY_ONLY_SECTIONS = new Set(['activity', 'gateways']);

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
  const { summary, config } = useFederationDetails(id);
  const section = pathname.split('/').slice(3).filter(Boolean).join('/').toLowerCase();
  const needsConfig = !SUMMARY_ONLY_SECTIONS.has(section);

  const context = useMemo<FederationContext | null>(
    () => (summary.data ? { id, ...summary.data, config: config.data } : null),
    [id, summary.data, config.data],
  );

  if (summary.loading && !context) return <FederationPageLoading />;
  if (!context) {
    const notFound = summary.error instanceof ApiError && summary.error.status === 404;
    return <FederationPageError error={summary.error} notFound={notFound} onRetry={summary.reload} />;
  }

  return (
    <div>
      <FederationHeader id={id} federation={context.federation} config={context.config} />
      {config.failed && (
        <div className="mb-4">
          <ErrorState title="Could not load the federation config" message={errorMessage(config.error)} onRetry={config.reload} />
        </div>
      )}
      <ErrorBoundary key={pathname} fallback={<ReloadMessage message="Something went wrong while showing this section." />}>
        <Suspense fallback={<SectionLoading />}>

          <div key={pathname} className="fmo-section-enter">
            {needsConfig && !context.config
              ? config.loading && <SectionLoading />
              : <Outlet context={context} />}
          </div>
        </Suspense>
      </ErrorBoundary>
    </div>
  );
}
