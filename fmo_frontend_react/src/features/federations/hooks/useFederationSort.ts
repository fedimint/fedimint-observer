import { useSearchParams } from 'react-router-dom';
import {
  DEFAULT_SORT_KEY,
  defaultDirectionFor,
  isSortKey,
  type SortDirection,
  type SortKey,
} from '../../../utils/sortFederations';

/**
 * The federation list's order. It lives in the URL, so it survives opening a
 * federation and coming back, and can be shared. Defaults are left out of the URL.
 */
export function useFederationSort() {
  const [params, setParams] = useSearchParams();
  const requestedSort = params.get('sort');
  const sortKey: SortKey = isSortKey(requestedSort) ? requestedSort : DEFAULT_SORT_KEY;
  const requestedOrder = params.get('order');
  const sortDirection: SortDirection = requestedOrder === 'asc' || requestedOrder === 'desc'
    ? requestedOrder
    : defaultDirectionFor(sortKey);
  const setSort = (key: SortKey, direction: SortDirection) => setParams((previous) => {
    const next = new URLSearchParams(previous);
    if (key === DEFAULT_SORT_KEY) next.delete('sort'); else next.set('sort', key);
    if (direction === defaultDirectionFor(key)) next.delete('order'); else next.set('order', direction);
    return next;
  }, { replace: true });

  return { sortKey, sortDirection, setSort };
}
