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
- `fmo_modules/` - One crate per Fedimint module (`fmo_module_mint`, `_wallet`, `_ln`): decoding and processing of that module's inputs, outputs and consensus items
  - `fmo_module_stability_pool` (Fedi's stability pool v1) is optional and outside the workspace members: `fmo_server --features stability_pool_v1` registers it, and it doesn't build until Fedi's `stability-pool-common` supports this Fedimint version
- `fmo_server/` - The default build: `FedimintObserver::builder().with_module(...)` with the mint, wallet and ln modules
- `fmo_frontend_react/` - Frontend (React + TypeScript)

### Key Patterns
1. **Shared Types**: API types are defined in `fmo_api_types` for the backend; the React frontend mirrors them by hand in `fmo_frontend_react/src/types/api.ts`, so keep both in sync
2. **Database Migrations**: Version-controlled SQL migrations in `fmo_core/schema/` (v0-v12)
3. **Modules**: Core writes the module-agnostic rows of every session item from the summary the owning module returns, then lets the module write its own tables in the same transaction. Kinds without a module in the build are stored without amounts or details, and adding the module later does not fill them in yet (core rows are written once). Each module's tables live in its own schema, `fmo_<kind>` (`fmo_wallet.peg_ins`, `fmo_ln.contracts`, ...), which core creates at startup; module SQL uses schema-qualified names, and a module's own future migrations come from `ObserverModule::migrations()` (tracked in `module_schema_versions`). `block_height_votes` stays in `public` because core's `session_times` is built from it; the wallet module records votes through `ProcessCtx::record_block_height_vote`. A populated database upgrades only from v11 or newer (`MIN_UPGRADE_VERSION`): the v8 and v11 backfills run module code that writes to the `fmo_<kind>` schemas, which exist only from v12 on. Module-specific API endpoints and background tasks (UTXOs, nonces, gateways) still live in `fmo_core`.
4. **Background Monitoring**: `FederationObserver` spawns tasks to monitor multiple federations concurrently
5. **State Management**: Backend uses shared app state with Arc/RwLock for thread safety
6. **Error Handling**: Custom `AppError` type wrapping `anyhow::Error` for consistent error propagation

### Environment Configuration
Required environment variables (`sample.env` is a template; the server does not load `.env` itself, so export it first: `set -a; . ./.env; set +a`):
- `FO_BIND`: Server bind address (e.g., "127.0.0.1:3000")
- `FO_DATABASE`: PostgreSQL connection string
- `FO_ADMIN_AUTH`: Admin authentication password
- `FO_MEMPOOL_URL`: Mempool API URL (default: "https://mempool.space/api")

### API Endpoints
- **Config API** (`/config/*`): Stable API for federation configuration inspection
- **Federations API** (`/federations/*`): Unstable API for federation monitoring data
- **Admin endpoints**: Require bearer token authentication via `FO_ADMIN_AUTH`

### Database Schema
PostgreSQL. Key tables:
- `federations` - Federation configurations
- `sessions` - Consensus sessions
- `transactions` - Transaction records
- `guardian_health` - Guardian monitoring data
- `nostr_*` - Nostr protocol integration
- `fmo_wallet.*` - peg-ins, peg-out transactions/signatures/inputs/outputs and the `utxos` materialized view
- `fmo_ln.contracts` - Lightning contracts

Read models, kept current as data is written so requests never aggregate the full history (v9, v12):
- `session_times`, `federation_daily_activity` - written per new session by `record_new_session()`, in the session's transaction; `rebuild_read_models()` after reprocessing stored sessions
- `federation_totals` - transaction, volume and wallet counters, by triggers on inserts
- `guardian_health_hourly` - by a trigger on `guardian_health`; readers add the partial first hour from raw rows. Uptime weights samples by `poll_interval_s`, since fully offline federations are polled less often
- `utxos` - materialized view, refreshed every minute
