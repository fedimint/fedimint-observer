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

## Checks

```bash
npm run lint
npm run build   # type-checks with `tsc -b`, then bundles into dist/
```

The API types in `src/types/api.ts` mirror the backend's `fmo_api_types` crate
by hand; keep them in sync when an endpoint changes.

## Nix build

`nix build .#fmo_frontend_react_default` builds against
`http://localhost:3000/api`; deployments build `fmo_frontend_react "/api"` (see
the root README). After changing `package-lock.json`, update `npmDepsHash` in
`flake.nix`: `nix run nixpkgs#prefetch-npm-deps -- fmo_frontend_react/package-lock.json`.
