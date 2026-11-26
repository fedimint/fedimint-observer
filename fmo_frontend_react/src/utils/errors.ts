import { ApiError } from '../services/http';


export function errorMessage(err: unknown): string {
  return err instanceof Error && err.message ? err.message : 'Unknown error';
}

/** Whether trying the same thing again may work; a request the server rejected won't */
export function isRetryable(err: unknown): boolean {
  return !(err instanceof ApiError) || err.retryable;
}

/** Whether the request never reached the observer, so the viewer's connection is the likely cause */
export function isNetworkError(err: unknown): boolean {
  return err instanceof ApiError && err.kind === 'network';
}
