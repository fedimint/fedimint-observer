import { errorMessage } from '../../../utils/errors';
import { ErrorState, Skeleton } from '../../../components/ui/States';

/** Loading skeleton for a federation page before its details arrive */
export function FederationPageLoading() {
  return (
    <div role="status" aria-label="Loading federation" className="space-y-4">
      <Skeleton className="h-4 w-24" />
      <Skeleton className="h-8 w-72 max-w-full" />
      <Skeleton className="h-10 w-full" />
      <div className="grid gap-3 md:grid-cols-3">
        <Skeleton className="h-28 w-full rounded-xl" />
        <Skeleton className="h-28 w-full rounded-xl" />
        <Skeleton className="h-28 w-full rounded-xl" />
      </div>
      <Skeleton className="h-64 w-full rounded-xl" />
    </div>
  );
}

export function FederationPageError({ error, notFound, onRetry }: { error: unknown; notFound: boolean; onRetry: () => void }) {
  return (
    <div className="py-8">
      <ErrorState
        title={notFound ? 'Federation not found' : 'Could not load the federation'}
        message={notFound ? 'The observer does not follow a federation with this id.' : errorMessage(error)}
        onRetry={notFound ? undefined : onRetry}
      />
    </div>
  );
}
