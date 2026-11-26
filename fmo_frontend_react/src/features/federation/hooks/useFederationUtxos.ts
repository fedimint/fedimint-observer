import { useEffect, useMemo, useState } from 'react';
import { api } from '../../../services/api';
import type { GuardianUtxoReport, UtxoView } from '../../../types/api';
import { errorMessage, isRetryable } from '../../../utils/errors';
import { useApiResource } from '../../../hooks/useApiResource';
import { usePageSize } from '../../../hooks/usePageSize';
import type { PageSize } from '../../../utils/pageSize';
import { comparedGuardianIds, guardianFindings, toInventoryRows, UTXOS_PER_PAGE } from '../lib/utxos';

const SEARCH_DEBOUNCE_MS = 300;
const NO_GUARDIANS: GuardianUtxoReport[] = [];


export function useFederationUtxos(id: string, enabled: boolean) {
  const [view, setView] = useState<UtxoView>('all');
  const [page, setPage] = useState(1);
  const [pageSize, choosePageSize] = usePageSize('utxos', UTXOS_PER_PAGE);
  const [search, setSearch] = useState('');
  const [query, setQuery] = useState('');

  useEffect(() => {
    const trimmed = search.trim();
    if (trimmed === query) return;
    const timer = setTimeout(() => {
      setQuery(trimmed);
      setPage(1);
    }, SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [search, query]);

  const resource = useApiResource(
    async (signal) => ({ id, response: await api.getFederationUtxos(id, { view, search: query, page, limit: pageSize }, signal) }),
    [id, view, query, page, pageSize],
    { enabled, resetOnError: true, label: 'UTXOs' },
  );
  const data = resource.data?.id === id ? resource.data.response : null;

  const guardians = data?.guardians ?? NO_GUARDIANS;
  const comparedIds = useMemo(() => comparedGuardianIds(guardians), [guardians]);
  const rows = useMemo(() => toInventoryRows(data), [data]);
  const pageCount = Math.max(1, data?.page_count ?? 1);
  const currentPage = data?.page ?? page;
  const threshold = data?.threshold ?? 0;
  const comparedCount = comparedIds.length;

  return {
    data,
    rows,
    loading: resource.loading,
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
    pageStart: (currentPage - 1) * (data?.limit ?? pageSize),
    pageSize,

    setPageSize: (next: PageSize) => {
      choosePageSize(next);
      setPage(1);
    },
    view,
    query,
    search,
    setSearch,

    selectView: (next: UtxoView) => {
      setView(next);
      setPage(1);
    },
    setPage,
  };
}

export type FederationUtxos = ReturnType<typeof useFederationUtxos>;
