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
just test_package fmo_server
```

## Architecture Overview

### Workspace Structure
- `fmo_api_types/` - Shared API types between frontend and backend
- `fmo_server/` - Backend server (Axum + PostgreSQL)
  - `/config/*` endpoints - Federation configuration API (stable)
  - `/federations/*` endpoints - Federation monitoring API (unstable)
  - Background tasks for monitoring federations and syncing data
- `fmo_frontend_react/` - Frontend (React + TypeScript)
- `fmo_prober/` - Standalone LN prober (LND REST) measuring gateway reachability/latency/liquidity, pushes results to `POST /gateways/probes`

### Key Patterns
1. **Shared Types**: All API types are defined in `fmo_api_types` and used by both frontend and backend
2. **Database Migrations**: Version-controlled SQL migrations in `fmo_server/schema/` (v0-v11)
3. **Background Monitoring**: `FederationObserver` spawns tasks to monitor multiple federations concurrently
4. **State Management**: Backend uses shared app state with Arc/RwLock for thread safety
5. **Error Handling**: Custom `AppError` type wrapping `anyhow::Error` for consistent error propagation

### Environment Configuration
Required environment variables (see `sample.env`):
- `FO_BIND`: Server bind address (e.g., "127.0.0.1:3000")
- `FO_DATABASE`: PostgreSQL connection string
- `FO_ADMIN_AUTH`: Admin authentication password
- `FO_MEMPOOL_URL`: Mempool API URL (default: "https://mempool.space/api")
- `ALLOW_CONFIG_CORS`: Enable CORS for config endpoints
- `FO_PROBER_AUTH`: Bearer token for probe result ingestion (optional, ingestion disabled if unset)

### API Endpoints
- **Config API** (`/config/*`): Stable API for federation configuration inspection
- **Federations API** (`/federations/*`): Unstable API for federation monitoring data
- **Admin endpoints**: Require bearer token authentication via `FO_ADMIN_AUTH`

### Database Schema
PostgreSQL with materialized views and complex indexes. Key tables:
- `federations` - Federation configurations
- `sessions` - Consensus sessions
- `transactions` - Transaction records
- `guardian_health_*` - Guardian monitoring data
- `gateways`, `gateway_poll_snapshots` - Gateway registry history
- `gateway_probes` - LN probe results, keyed by gateway LN node pubkey
- `nostr_*` - Nostr protocol integration
