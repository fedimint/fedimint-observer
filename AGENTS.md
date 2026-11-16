# Agent Instructions

Guidance for AI coding agents and contributors. Load these project-local skills
(`.agents/skills/`) when their descriptions match the work:

- `observer-coding-style`: before writing, modifying, or reviewing Rust or
  frontend code, including SQL, migrations and background tasks.
- `observer-security`: before changing API endpoints, admin authentication,
  outbound connections (guardians, gateways, invite codes), or anything that
  publishes federation, gateway or user data.
- `pr-submissions-checklist`: before creating or updating a pull request.

Keep `OPERATIONS.md` current when a change affects deployment, `FO_*`
configuration, limits, egress, monitoring or upgrades.

## Project Overview

Fedimint Observer monitors Fedimint federations, aiming to be "the
mempool.space for Fedimint": transparency into federation operations without
compromising user privacy.

## Commands

Run inside `nix develop`.

```bash
just pg_start / pg_stop          # local PostgreSQL
just pg_backup / pg_restore FILE
just run                         # run fmo_server
just watch                       # run with auto-reload
just clippy                      # fastest compile and lint check
just clippy-fix
just test                        # all tests; `just test_package fmo_core` for one crate
just format
just typos / typos-fix-all
just lint                        # pre-commit hook checks
just final-check                 # lint, clippy and tests; run before a PR
just build / build_package fmo_server
```

Frontend (`fmo_frontend_react/`): `npm ci`, `npm run dev`, `npm run lint`,
`npm run build`, `npm run test:*`.

Environment (`sample.env` is a template; the server doesn't load `.env`, so
export it: `set -a; . ./.env; set +a`): `FO_BIND` (default `127.0.0.1:3000`),
`FO_DATABASE`, `FO_ADMIN_AUTH`, `FO_MEMPOOL_URL` (default
`https://mempool.space/api`). Limits and other `FO_*` variables are in
`OPERATIONS.md`.

## Architecture

### Workspace

- `fmo_api_types/`: API types. The frontend mirrors them by hand in
  `fmo_frontend_react/src/types/api.ts`; keep both in sync.
- `fmo_core/`: backend library (Axum + PostgreSQL): collection, storage, API,
  shared services, and the `ObserverModule` trait and `ModuleRegistry`
  (`module.rs`).
- `fmo_modules/`: one crate per Fedimint module (`fmo_module_mint`, `_wallet`,
  `_ln` and their `_mintv2`, `_walletv2`, `_lnv2` counterparts), decoding and
  processing its inputs, outputs and consensus items.
  `fmo_module_stability_pool` (Fedi's stability pool v1) is outside the
  workspace: `fmo_server --features stability_pool_v1` registers it, and it
  won't build until Fedi's `stability-pool-common` supports this Fedimint
  version.
- `fmo_server/`: the default build, `FedimintObserver::builder().with_module(...)`
  with the v1 and v2 mint, wallet and ln modules.
- `fmo_frontend_react/`: React + TypeScript frontend.

### API

- `/config/*`: stable configuration-inspection API; don't change existing
  response shapes.
- `/federations/*`: unstable monitoring API.
- Admin endpoints need the `FO_ADMIN_AUTH` bearer token.
- Errors: `AppError` carries an HTTP status and a client-safe message;
  unexpected errors are logged and returned as a sanitized `500`. Errors the
  client caused or should see (unknown federation, bad parameters, unreachable
  guardians) are `ClientError::{not_found, bad_request, bad_gateway,
  gateway_timeout, unavailable}` returned through `anyhow`; they keep their
  status through `?` and `.context()`.

### Collection and modules

- Core stores every session and writes the module-agnostic rows of each item
  from the summary its module returns. Kinds with no module in the build are
  stored without amounts or details.
- Each module's own tables are filled afterwards by its projector
  (`fmo_core/src/federation/projector.rs`), one per module and federation. It
  runs the module's `process_*` over items of its kind in batches of sessions,
  each batch in one transaction with the module's cursor in `module_cursors`.
  So a module added later fills its tables from session 0, and a failing
  module holds back only itself.
- When a module catches up, triggers keep `federation_totals` correct, and
  session times and daily activity are rebuilt from `module_cursors.repair_from`.
- Admins rebuild a module's tables with `POST /federations/:id/modules/:kind/reset`
  (`{"session_start": 0, "clean": true}` first deletes its rows through
  `ObserverModule::reset_federation`). `GET /federations/:id` reports each
  module's progress in `module_sessions`.
- Module tables live in the module's own schema, `fmo_<kind>`
  (`fmo_wallet.peg_ins`, `fmo_lnv2.gateways`, ...), which core creates at
  startup. Module SQL uses schema-qualified names; a module's migrations come
  from `ObserverModule::migrations()`, tracked in `module_schema_versions`.
- `block_height_votes` stays in `public` because core builds `session_times`
  from it; the wallet module reports votes through
  `ObserverModule::block_height_vote`.
- Modules own their routes and tasks. `federation_routes()` mounts under
  `/federations/:federation_id` (wallet `/utxos`, mint `/nonces/spend`, ln
  `/gateways/overview`, lnv2 `/gateways/v2`, walletv2 `/wallet-v2/history`);
  `config_routes()` under `/config/:invite` (ln `/gateways`).
  `federation_task()` and `task()` run supervised like core's tasks (ln:
  gateway polling and overview refresh; lnv2: registry polling, probes and
  retention; wallet: Bitcoin enrichment jobs; walletv2: balance, history and
  deposit collection).
- Handlers and tasks get a `Services` handle (database, connectors, mempool
  client, federation configs). Guardian API clients come from
  `Services::federation_api()` (observed federations) and
  `Services::lookup_api()` (invite lookups): one shared client per guardian
  set, never one per request, because each client keeps its guardian
  connections open.

### Database

PostgreSQL. Core migrations are `fmo_core/schema/v0.sql` to `v12.sql`; some
modules also keep SQL in `fmo_modules/<module>/schema/`. A populated database
upgrades only from v11 or newer (`MIN_UPGRADE_VERSION`); older ones need the
previous release's backfills. Old migrations stay for fresh setups.

Key tables:

- Core: `federations`, `sessions`, `transactions`, `guardian_health`,
  `nostr_*`, `federation_observations` (metadata and other observations, kept
  on failed polls), `federation_session_counts`, `session_time_repairs`,
  `module_cursors`.
- `fmo_wallet`: peg-ins, peg-out transactions, signatures, inputs and outputs,
  the `utxos` materialized view, and `enrichment_jobs` (leased Bitcoin lookups,
  kept out of projection transactions).
- `fmo_ln`: the v1 `gateways` registry and `gateway_poll_snapshots`; contract
  activity is read from core's `transaction_outputs` and
  `transaction_input_details`.
- `fmo_lnv2`: `gateways`, `gateway_keys`, `gateway_polls`,
  `gateway_health_hourly`, `contract_activity`, `contract_resolutions`.
- `fmo_walletv2`: `wallets` (balance and history position), `chain` (on-chain
  history), `deposit_jobs`.

Read models, updated as data is written so requests never aggregate the full
history:

- `session_times`, `federation_daily_activity`: written per new session by
  `record_new_session()` in the session's transaction. After stored sessions
  are reprocessed, `schedule_read_model_rebuild()` queues a rebuild that the
  collector runs in bounded steps (`FederationObserver::rebuild_read_models`).
- `federation_totals`: transaction, volume and wallet counters, kept by
  insert triggers.
- `guardian_health_hourly`: kept by a trigger on `guardian_health`; readers
  add the partial first hour from raw rows. Uptime weights samples by
  `poll_interval_s`, since offline federations are polled less often.
- `utxos`: materialized view, refreshed every minute.
