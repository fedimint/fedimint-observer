# Architecture

Fedimint Observer watches Fedimint federations and serves what it sees over an
HTTP API and a React frontend. This document is for contributors: how the code
is laid out, how data moves through it, and where new code belongs.

## Workspace

| Crate | Role |
|---|---|
| `fmo_core` | The module-agnostic backend: collection, processing, storage, recommendations and the HTTP API |
| `fmo_modules/fmo_module_*` | One crate per Fedimint module (mint, wallet, ln and their v2 versions). Each one decodes its module's items, derives its own tables and serves its own endpoints |
| `fmo_server` | The default build: `FedimintObserver::builder().with_module(..)` for every module |
| `fmo_api_types` | Types of the JSON API. The frontend mirrors them by hand in `fmo_frontend_react/src/types/api.ts` |
| `fmo_frontend_react` | The web frontend |

## Layers of `fmo_core`

```
            guardians        mempool        Nostr relays
                │               │                │
   ┌────────────▼───────────────▼────────────────▼──────────────┐
   │ Collection   federation/ingest.rs   guardians.rs           │
   │              metadata.rs            recommend/ (sync)      │
   └────────────┬───────────────────────────────────────────────┘
                │ writes through
   ┌────────────▼───────────────────────────────────────────────┐
   │ Storage      store/ (schema, rows, db, observations)       │
   └────────────┬───────────────────────────▲───────────────────┘
                │ stored sessions           │ module tables, read models
   ┌────────────▼───────────────────────────┴───────────────────┐
   │ Processing   federation/projector.rs + each module's       │
   │              process_* (fmo_modules)                       │
   └────────────────────────────────────────────────────────────┘
                                │ read by
   ┌────────────────────────────▼───────────────────────────────┐
   │ Presentation api.rs → federation/mod.rs, summary.rs,       │
   │              session.rs, transaction.rs, config/,          │
   │              recommend/, modules' federation_routes()      │
   └────────────────────────────────────────────────────────────┘
```

| Layer | Files | What it does |
|---|---|---|
| Collection | `federation/ingest.rs` | Fetches every observed federation's signed sessions from its guardians and stores each one, with core's rows of its items, in one transaction. Also fetches block times and runs admin backfills |
| | `federation/guardians.rs` | Polls guardian health, backs off for offline federations, deletes old samples |
| | `federation/metadata.rs` | Refreshes each federation's consensus metadata and override document every 5 minutes |
| Processing | `federation/projector.rs` | One projector per module and federation. It runs the module's `process_*` over stored sessions in batches, moving the module's cursor in the same transaction. It also rebuilds read models after reprocessing |
| Storage | `store/schema.rs` | Core migrations, module schemas and migrations, materialized-view refresh |
| | `store/rows.rs` | Writers of core's rows of session items, shared by ingest and projectors |
| | `store/db.rs`, `store/observations.rs` | Row types of `federations` and `sessions`; last-known-good per-federation snapshots |
| Recommendations | `recommend/` | Nostr federation announcements and ratings: relay sync, storage, publishing, routes |
| Presentation | `api.rs` | Assembles every layer's routes and the shared middleware (`http.rs`) |
| | `federation/mod.rs`, `summary.rs`, `session.rs`, `transaction.rs`, `meta.rs` | `/federations/*` handlers, each next to the read queries it serves |
| | `config/` | The stable `/config/*` inspector for any federation, by invite code |
| Wiring | `federation/observer.rs`, `lib.rs` | Pools, guardian and mempool clients, the task group, the registry of observed federations; startup |

Shared infrastructure sits beside the layers. `module.rs` defines the module
interface. `runtime.rs` holds deployment limits and the worker lock.
`task.rs` supervises background tasks, `work.rs` delivers database work
notifications, and `error.rs` defines the error types.

### Rules between layers

- **Collectors write; requests read.** A collector stores raw or
  last-known-good data, and requests serve what was stored, so a slow or
  offline guardian doesn't slow down pages. Few endpoints need live answers:
  `/config/*` inspects federations that aren't observed, and an observed
  federation's `/meta` and the wallet's `/utxos` comparison ask guardians on
  request. Each of these goes through a cache with a deadline and a bounded
  budget. New endpoints should read stored data unless live data is the
  point.
- **Writes of shared tables go through `store/`.** Core's session rows are
  written only by `store/rows.rs`, so ingest and projectors can't drift apart.
- **Requests never aggregate full history.** Totals, activity and session
  times come from read models (`federation_totals`, `federation_daily_activity`,
  `session_times`, `guardian_health_hourly`). These are kept current by
  triggers, or in the transaction that stores a session. If a new endpoint
  needs an aggregate, add a read model instead of a scan.
- **Modules and other plug-ins reach core only through `Services`**
  (`module.rs`), never through `FederationObserver` internals.

## Process model

One process is the **worker**. It holds a Postgres advisory lock over a
dedicated connection (`runtime.rs`), runs migrations at startup and runs every
collector and projector. Any number of **API replicas** (`FO_API_ONLY=true`)
serve requests from the same database. They refuse to start until the worker
has migrated the schema, and they never start background tasks. If the worker
loses its lock connection, it stops its tasks and exits with an error, so a
process manager restarts it.

Background tasks run under `task::supervise`, which restarts a task 30 seconds
after it fails or panics. Durable work, such as repair queues and module job
tables, lives in database rows. Triggers send `pg_notify('fmo_work', topic)`,
and `work.rs` turns these into wake-ups, so workers don't poll. The rows stay
authoritative: a missed notification only delays work.

Concurrency is bounded everywhere: database pools, `FO_WORKER_CONCURRENCY`
for heavy writes, `FO_EXTERNAL_CONCURRENCY` for calls to outside services,
`FO_HTTP_CONCURRENCY` with fast rejection for requests, and shared guardian
API clients, one per set of guardians (see `CLAUDE.md`).

## Storage conventions

- Core migrations are `fmo_core/schema/vN.sql`. They are only ever appended,
  never changed, and each one inserts its version into `schema_version`.
  A populated database upgrades from v11 or newer.
- Each module's tables live in its own schema, `fmo_<kind>`, created by core.
  The module's migrations come from `ObserverModule::migrations()` and are
  tracked in `module_schema_versions`.
- A module added to a build later fills its tables from session 0, because its
  projector starts at cursor 0. Admins rebuild a module with
  `POST /federations/:id/modules/:kind/reset`.

## Where new code goes

| You want to… | Put it in |
|---|---|
| Support a new Fedimint module | A new `fmo_modules/fmo_module_<kind>` crate implementing `ObserverModule`, registered in `fmo_server` |
| Collect more from guardians about every federation | A collector in `federation/`, started from `FederationObserver::new` under `supervise` |
| Derive tables from sessions | The owning module's `process_*`, or for core tables `projector.rs` |
| Add an endpoint about observed federations | A handler in `federation/` (or the module's `federation_routes()`), reading stored data or read models |
| Change the schema | A new `fmo_core/schema/vN+1.sql` added to `store/schema.rs` |

## Privacy

Observer is public, and Fedimint's value rests on ecash privacy. Publish
federation-level totals, percentiles and buckets. Never publish anything that
ties funds or payments to one user or one operator, and suppress figures
built from too few transactions.
