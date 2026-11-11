import { useEffect, useMemo, useState } from 'react';
import { api } from '../../../services/api';
import type { GuardianUtxoReport, UtxoView } from '../../../types/api';
import { errorMessage, isRetryable } from '../../../utils/errors';
import { useApiResource } from '../../../hooks/useApiResource';
import { comparedGuardianIds, guardianFindings, toInventoryRows, UTXOS_PER_PAGE } from '../lib/utxos';

const SEARCH_DEBOUNCE_MS = 300;
const NO_GUARDIANS: GuardianUtxoReport[] = [];

/**
 * One page of the comparison of the observer's UTXOs with the guardians' wallet
 * data; the server filters, counts and pages it. The server asks every guardian
 * and mempool.space for these, so nothing loads until `enabled`.
 */
export function useFederationUtxos(id: string, enabled: boolean) {
  const [view, setView] = useState<UtxoView>('all');
  const [page, setPage] = useState(1);
  const [search, setSearch] = useState('');
  // The search sent to the server, once typing pauses
  const [query, setQuery] = useState('');

  // A new search starts on its first page, once typing pauses
  useEffect(() => {
    const trimmed = search.trim();
    if (trimmed === query) return;
    const timer = setTimeout(() => {
      setQuery(trimmed);
      setPage(1);
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [search, query]);

  // A newer view, search or page aborts the request it replaces. Rows from an
  // earlier request would read as the answer to a failed one, so an error clears them.
  const resource = useApiResource(
    async (signal) => ({ id, response: await api.getFederationUtxos(id, { view, search: query, page, limit: UTXOS_PER_PAGE }, signal) }),
    [id, view, query, page],
    { enabled, resetOnError: true, label: 'UTXOs' },
  );
  const data = resource.data?.id === id ? resource.data.response : null;

  const guardians = data?.guardians ?? NO_GUARDIANS;
  const comparedIds = useMemo(() => comparedGuardianIds(guardians), [guardians]);
  const rows = useMemo(() => toInventoryRows(data), [data]);
  const pageCount = Math.max(1, data?.page_count ?? 1);
  // The server clamps a page past the end, so its answer names the page shown
  const currentPage = data?.page ?? page;
  const threshold = data?.threshold ?? 0;
  const comparedCount = comparedIds.length;

  return {
    data,
    rows,
    loading: resource.loading,
    // A rejected request (404: the federation has no Wallet v1 module) fails the same way again
    error: resource.failed
      ? {
          message: errorMessage(resource.error),
          retryable: isRetryable(resource.error),
        }
      : null,
    retry: resource.reload,
    guardians,
    comparedIds,
    threshold,
    findings: guardianFindings(guardians),
    laggingGuardians: guardians.filter((guardian) => guardian.status === 'lagging'),
    guardianSummaryUnavailable: guardians.length > 0 && comparedCount === 0,
    tooFewGuardians: comparedCount > 0 && comparedCount < threshold,
    counts: data?.counts,
    totalRows: data?.total_rows ?? 0,
    pageCount,
    currentPage,
    pageStart: (currentPage - 1) * (data?.limit ?? UTXOS_PER_PAGE),
    view,
    query,
    search,
    setSearch,
    /** Another status filter starts on its first page */
    selectView: (next: UtxoView) => {
      setView(next);
      setPage(1);
    },
    setPage,
  };
}

export type FederationUtxos = ReturnType<typeof useFederationUtxos>;
