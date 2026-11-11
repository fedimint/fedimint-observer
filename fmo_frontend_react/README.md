# Fedimint Observer frontend

React + TypeScript + Vite frontend of https://observer.fedimint.org.

## Development

Requires Node.js `^20.19` or `>=22.12` (the Nix dev shell does not include Node).

```bash
npm ci
# Without VITE_FMO_API_BASE_URL the frontend uses the production API
VITE_FMO_API_BASE_URL=http://127.0.0.1:3000 npm run dev
```

Run the backend with `just run` from the repository root. It serves the API at
`/federations`, `/config` and `/nostr` directly; the `/api` prefix only exists
behind Nginx.

### Manifold data

The Manifold pages (Guardian directory, operator profiles, FLIP providers)
read `/manifold/*`, which the backend serves from advertisements its collector
verified and stored from Nostr relays (`fmo_core/src/manifold/`). Seats come
only from federations' verified `fedi:fman_seat_bindings`.

## Code layout

```
src/
  pages/          one thin component per route: reads the URL, calls feature hooks, composes components
  features/<x>/   everything for one part of the site
    components/   its components
    hooks/        its data loading and view state
    lib/          pure functions and constants, no React: unit-testable with `node --test`
  components/ui/  building blocks with no domain knowledge (Panel, Tabs, FilterChips, Modal, …)
  components/layout/  the app shell (NavBar)
  hooks/          shared hooks: useApiResource (one request tied to a component), useCursorPages (cursor-paged lists with auto-refresh), useTheme
  services/ types/ utils/   API client, API types, formatting and other helpers shared by features
```

Features are `federations` (the home list), `federation` (a federation's page),
`gateways` (Lightning v1), `gateways-v2`, `wallet-v2` and `nostr`. A feature may
use `components/ui`, `hooks` and `utils`, and another feature's components
(the federation page embeds the wallet-v2 panel); `components/ui` never imports
from a feature.

Import modules by their file path; there are no `index.ts` barrels, which keeps
lazy-loaded pages and charts in their own chunks. A file that exports a
component exports only components (Fast Refresh); constants and helpers belong
in a `lib/` or `utils/` file.

## Paged lists

Lists load one page at a time from the backend: `useCursorPages` asks for
page 1 (refreshed each minute) and, on Next, for the page after the last
cursor; another query or page size starts again at page 1. `CursorPager`
shows previous/next and "Rows per page" (`PAGE_SIZES`, sent as `limit`), which
`usePageSize` remembers per list in this browser. Search, filters and order
are query parameters the backend applies, never filtering in the browser, and
searches are sent once typing pauses (`useDebouncedValue`).

## API errors

Every request goes through `request()` in `src/services/http.ts`, never `fetch`
directly. A failure rejects with an `ApiError` whose `kind` is `network`,
`timeout`, `http` or `invalid_response`, and whose message is fit to show: the
backend's `{ "error": … }` message when it sent one, otherwise a fixed message
for the status. Response bodies that are not that JSON (proxy pages, plain text)
are never shown. Server faults (5xx) append the response's `x-request-id` as
`(ref N)`, which finds the request in the backend's logs. A request its caller
cancels rejects with the caller's abort, as `fetch` does, and every request has a
time limit (`DEFAULT_TIMEOUT_MS`).

Pages say what failed and add the reason: `Could not load the UTXOs:
${errorMessage(err)}`. `isRetryable(err)` tells whether offering Try again makes
sense: a request the server rejected (4xx) fails the same way again.

## Checks

```bash
npm run lint
npm run build   # type-checks with `tsc -b`, then bundles into dist/
npm run test:config && npm run test:pagination && npm run test:charts && npm run test:format && npm run test:gateways && npm run test:http
```

The API types in `src/types/api.ts` mirror the backend's `fmo_api_types` crate
by hand; keep them in sync when an endpoint changes.

## Nix build

`nix build .#fmo_frontend_react_default` builds against
`http://localhost:3000/api`; deployments pass their absolute public API URL to
`fmo_frontend_react` so `llms.txt` contains absolute links (see the root README).
After changing `package-lock.json`, update `npmDepsHash` in
`flake.nix`: `nix run nixpkgs#prefetch-npm-deps -- fmo_frontend_react/package-lock.json`.
