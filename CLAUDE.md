# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Fedimint Observer is a Rust-based monitoring platform for Fedimint federations, aiming to be "the mempool.space for Fedimint". It provides transparency into federation operations while respecting privacy constraints.

## Development Commands

### Essential Commands
```bash
# Enter development environment (requires Nix)
nix develop

# Start local PostgreSQL for development
just pg_start

# Run backend server with auto-reload
just watch

# Check code compilation and common issues (preferred during development)
just clippy

# Run all tests
just test

# Format code
just format

# Run all checks before PR
just final-check
```

### Build Commands
```bash
just build              # Build everything
just build_package fmo_server
```

### Code Quality Commands
```bash
just clippy            # Run clippy linter (preferred for quick compilation checks)
just clippy-fix        # Run clippy with auto-fix
just check             # Run cargo check
just lint              # Run pre-commit linting
just typos             # Check for typos
just typos-fix-all     # Fix all typos
```

### Database Commands
```bash
just pg_start          # Start local PostgreSQL
just pg_stop           # Stop PostgreSQL
just pg_backup         # Backup database
just pg_restore BACKUP_FILE
```

### Testing Specific Components
```bash
just test_package fmo_core
```

## Architecture Overview

### Workspace Structure
- `fmo_api_types/` - Shared API types between frontend and backend
- `fmo_core/` - Backend library (Axum + PostgreSQL): fetching, storage, API and shared services
  - `/config/*` endpoints - Federation configuration API (stable)
  - `/federations/*` endpoints - Federation monitoring API (unstable)
  - Background tasks for monitoring federations and syncing data
  - `module.rs` - the `ObserverModule` trait and `ModuleRegistry`
- `fmo_modules/` - One crate per Fedimint module (`fmo_module_mint`, `_wallet`, `_ln`, and their `_mintv2`, `_walletv2`, `_lnv2` counterparts): decoding and processing of that module's inputs, outputs and consensus items
  - `fmo_module_stability_pool` (Fedi's stability pool v1) is optional and outside the workspace members: `fmo_server --features stability_pool_v1` registers it, and it doesn't build until Fedi's `stability-pool-common` supports this Fedimint version
- `fmo_server/` - The default build: `FedimintObserver::builder().with_module(...)` with both v1 and v2 mint, wallet and ln modules
- `fmo_frontend_react/` - Frontend (React + TypeScript)

### Key Patterns
1. **Shared Types**: API types are defined in `fmo_api_types` for the backend; the React frontend mirrors them by hand in `fmo_frontend_react/src/types/api.ts`, so keep both in sync
2. **Database Migrations**: Version-controlled SQL migrations in `fmo_core/schema/` (v0-v12), with module-owned migrations in `fmo_modules/<module>/schema/`
3. **Modules**: Core writes the module-agnostic rows of every session item from the summary the owning module returns, Each module's own tables are derived from the stored sessions afterwards by its projector (`fmo_core/src/federation/projector.rs`): one per module and federation, it runs the module's `process_*` on the items of its kind in batches of sessions, each batch in one transaction with the move of the module's cursor in `module_cursors`, so a module added later fills its tables from session 0 and a failing module holds back only itself. Kinds without a module in the build are stored without amounts or details; adding the module later completes them, as its projector catches up (triggers keep `federation_totals` right, and session times and daily activity are rebuilt once it has caught up, from `module_cursors.repair_from`). Admins rebuild a module's tables with `POST /federations/:id/modules/:kind/reset` (`{"session_start": 0, "clean": true}` first deletes its rows through `ObserverModule::reset_federation`); `GET /federations/:id` reports `module_sessions`, how far each module is. Each module's tables live in its own schema, `fmo_<kind>` (`fmo_wallet.peg_ins`, `fmo_lnv2.gateways`, ...), which core creates at startup; module SQL uses schema-qualified names, and a module's own future migrations come from `ObserverModule::migrations()` (tracked in `module_schema_versions`). `block_height_votes` stays in `public` because core's `session_times` is built from it; the wallet module reports them through `ObserverModule::block_height_vote` and core records them while storing the session. Modules also own their endpoints and background tasks: `federation_routes()` mounts under `/federations/:federation_id` (wallet: `/utxos`; mint: `/nonces/spend`; ln: `/gateways/overview`; lnv2: `/gateways/v2`; walletv2: `/wallet-v2/history`), `config_routes()` under `/config/:invite` (ln: `/gateways`), and `federation_task()`/`task()` run supervised like core's tasks (ln: gateway polling and overview refresh; lnv2: registry polling, probes and retention; wallet: Bitcoin enrichment jobs; walletv2: balance, history and deposit collection). Handlers and tasks get a `Services` handle (database connection, connectors, mempool client, federation configs). Guardian API clients come from `Services::federation_api()` (observed federations) and `Services::lookup_api()` (invite lookups), one shared client per set of guardians: never build one per request, because each client's connection pool keeps its guardian connections open for good. A populated database upgrades only from v11 or newer (`MIN_UPGRADE_VERSION`); older versions need the previous release's data backfills. Historical SQL migrations remain for fresh database setup.
4. **Background Monitoring**: `FederationObserver` spawns tasks to monitor multiple federations concurrently
5. **State Management**: Backend uses shared app state with Arc/RwLock for thread safety
6. **Error Handling**: Custom `AppError` with HTTP status and client-safe messages; unexpected errors are logged and returned as sanitized internal errors. For errors the client caused or should see (unknown federation, bad parameters, unreachable guardians), return `ClientError::{not_found, bad_request, bad_gateway, gateway_timeout, unavailable}` through `anyhow`: it keeps its status through `?` and `.context()`

### Environment Configuration
Required environment variables (`sample.env` is a template; the server does not load `.env` itself, so export it first: `set -a; . ./.env; set +a`):
- `FO_BIND`: Server bind address (e.g., "127.0.0.1:3000")
- `FO_DATABASE`: PostgreSQL connection string
- `FO_ADMIN_AUTH`: Admin authentication password
- `FO_MEMPOOL_URL`: Mempool API URL (default: "https://mempool.space/api")
- `FO_MANIFOLD_RELAYS`: Comma-separated relays the Manifold discovery collector reads (default: Manifold's production relays); empty turns the collector off
- `FO_MANIFOLD_TRUSTED_ISSUERS`: Comma-separated PeerBadge issuer keys treated as trust roots (default: none, as Manifold's production environment pins none yet)

### API Endpoints
- **Config API** (`/config/*`): Stable API for federation configuration inspection
- **Federations API** (`/federations/*`): Unstable API for federation monitoring data
- **Admin endpoints**: Require bearer token authentication via `FO_ADMIN_AUTH`
- **Paging**: every list the frontend shows is paged by the backend (`?limit=` 1-500, default 100, and an opaque `?after=` cursor; responses carry `next_after`, null on the last page, and `total`). Lists whose order is computed (health, uptime, freshness, price) page with `pagination::keyset_page`, whose cursor is the last row's sort key, so rows changing between requests never shift a page. Search, filters and order run on the backend: `/federations/page` (`group`, `search`, `sort`, `dir`), `/nostr/federations/page`, `/manifold/guardians` (`search`, `verification`, `version`, `size`, `fresh_only`, `sort`), `/manifold/flip-providers`, `/federations/:id/gateways/overview` (`search`, `status`, `sort`, `dir`). `GET /federations` and `GET /nostr/federations` stay unpaged for existing API clients; one federation's summary is `GET /federations/:id/summary`

### Database Schema
PostgreSQL. Key tables:
- `federations` - Federation configurations
- `sessions` - Consensus sessions
- `transactions` - Transaction records
- `guardian_health` - Guardian monitoring data
- `nostr_*` - Nostr protocol integration
- `fmo_wallet.*` - peg-ins, peg-out transactions/signatures/inputs/outputs and the `utxos` materialized view
- `fmo_wallet.enrichment_jobs` - leased Bitcoin lookups for peg-out data, kept out of projection transactions
- `fmo_ln.*` - the Lightning v1 `gateways` registry with its `gateway_poll_snapshots` (contract activity is read from core's `transaction_outputs`/`transaction_input_details`)
- `fmo_lnv2.*` - gateway registry (`gateways`, `gateway_keys`, `gateway_polls`), probe history (`gateway_health_hourly`) and contract activity (`contract_activity`, `contract_resolutions`)
- `fmo_walletv2.*` - `wallets` (balance and history position), the on-chain `chain` history and `deposit_jobs`
- `federation_observations` - persisted metadata and other per-federation observations, kept on failed polls
- `federation_session_counts`, `session_time_repairs` - incremental session counts and queued session-time rebuilds
- `module_cursors` - the next stored session each module's projector processes, per federation

Read models, kept current as data is written so requests never aggregate the full history (v9, v12):
- `session_times`, `federation_daily_activity` - written per new session by `record_new_session()`, in the session's transaction; after stored sessions are reprocessed, `schedule_read_model_rebuild()` queues a rebuild that the collector runs in bounded steps (`FederationObserver::rebuild_read_models` in `projector.rs`)
- `federation_totals` - transaction, volume and wallet counters, by triggers on inserts
- `guardian_health_hourly` - by a trigger on `guardian_health`; readers add the partial first hour from raw rows. Uptime weights samples by `poll_interval_s`, since fully offline federations are polled less often
- `utxos` - materialized view, refreshed every minute
