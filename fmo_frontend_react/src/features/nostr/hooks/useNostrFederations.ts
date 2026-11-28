import { useCallback, useEffect, useMemo, useState } from 'react';
import { api } from '../../../services/api';
import type { NostrFederationPage, NostrGroup } from '../../../types/api';
import { useCursorPages } from '../../../hooks/useCursorPages';
import { lookUpNames, type NostrFederation } from '../lib/federationNames';

const REFRESH_MS = 60_000;
const nextAfter = (page: NostrFederationPage) => page.next_after;


export function useNostrFederations(group: NostrGroup, limit: number, lookUpMissingNames = true) {
  const fetchPage = useCallback(
    (after: string | undefined, signal: AbortSignal) => api.getNostrFederationsPage(group, limit, after, signal),
    [group, limit],
  );
  const pages = useCursorPages({ fetchPage, nextCursor: nextAfter, refreshMs: REFRESH_MS });
  const [lookedUpNames, setLookedUpNames] = useState<Map<string, string>>(() => new Map());
  const rows = pages.page?.federations;
  const unnamedKey = rows && lookUpMissingNames
    ? JSON.stringify(rows.filter((fed) => !fed.name).map(({ id, invite }) => [id, invite]))
    : null;

  useEffect(() => {
    if (unnamedKey === null) return;
    const controller = new AbortController();
    const unnamed = (JSON.parse(unnamedKey) as [string, string][]).map(([id, invite]) => ({ id, name: null, invite }));
    void lookUpNames(unnamed, controller.signal, (names) => {
      setLookedUpNames((previous) => new Map([...previous, ...names]));
    });
    return () => controller.abort();
  }, [unnamedKey]);

  const federations = useMemo<NostrFederation[]>(
    () => (rows ?? []).map((fed) => ({
      id: fed.id,
      invite: fed.invite,
      name: fed.name || lookedUpNames.get(fed.id) || null,
      health: fed.health ?? undefined,
      observed: fed.health !== null,
    })),
    [rows, lookedUpNames],
  );

  return {
    ...pages,
    federations,
    total: pages.page?.total ?? null,
    loading: pages.page === null && !pages.error,
    failed: pages.page === null && pages.error !== null,
  };
}
