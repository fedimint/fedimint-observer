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

## Checks

```bash
npm run lint
npm run build   # type-checks with `tsc -b`, then bundles into dist/
npm run test:config && npm run test:pagination && npm run test:charts && npm run test:format && npm run test:gateways
```

The API types in `src/types/api.ts` mirror the backend's `fmo_api_types` crate
by hand; keep them in sync when an endpoint changes.

## Nix build

`nix build .#fmo_frontend_react_default` builds against
`http://localhost:3000/api`; deployments pass their absolute public API URL to
`fmo_frontend_react` so `llms.txt` contains absolute links (see the root README).
After changing `package-lock.json`, update `npmDepsHash` in
`flake.nix`: `nix run nixpkgs#prefetch-npm-deps -- fmo_frontend_react/package-lock.json`.
