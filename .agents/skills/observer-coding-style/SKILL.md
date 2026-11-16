---
name: observer-coding-style
description: Use before writing, modifying, or reviewing Rust or frontend code in Fedimint Observer, including SQL, migrations and background tasks.
---

# Observer Coding Style

Fedimint's coding style applies here too, plus the observer-specific rules
below. Follow existing patterns before inventing new ones.

## Rust

- Never use `unwrap()` in non-test code. Use `?` with `.context("...")`, or
  `expect("...")` with a message explaining why the invariant holds.
- Don't silently drop errors with `.ok()` or `let _ =`; at least log them with
  a reason.
- Use structured `tracing` fields (`warn!(federation_id = %id, ?err, "...")`)
  rather than string interpolation, and log enough for an operator to see why
  something is stuck.
- Use named constants rather than magic numbers, `Duration` rather than raw
  seconds, and enums or newtypes rather than booleans and strings for
  structured data.
- Write new tests in standalone files (`mod tests;` with `src/foo/tests.rs`),
  not inline `mod tests { ... }`. Extract inline tests you modify; don't
  bulk-migrate untouched ones.
- Every new dependency must earn its place.

## Errors and API shape

- Return client-caused errors as `ClientError` (`fmo_core/src/error.rs`);
  never put internal error text, SQL or guardian responses in a response body.
- List endpoints use keyset pagination through `pagination::Page<C>` (limit
  1–500, default 100). No offset pagination and no unbounded lists.
- API changes update `fmo_api_types` and `src/types/api.ts` together, and
  never break `/config/*` response shapes.

## Scalability

The observer has to keep working as federations, sessions and gateways grow.

- Requests must never aggregate the full history. Read from the read models
  (`federation_totals`, `session_times`, `federation_daily_activity`,
  `guardian_health_hourly`, ...) or add one that is updated as data is written.
- Every query on a growing table needs a supporting index and a bound
  (`LIMIT`, keyset cursor or time range). Check new queries with `EXPLAIN` on
  realistic data.
- Never load a whole table or federation history into memory. Process in
  bounded batches, each in one transaction with its cursor move, like the
  projectors in `fmo_core/src/federation/projector.rs`.
- Bound everything that comes from outside: list sizes from guardians and
  gateways, response sizes, retries and queues. Unbounded channels and
  retry loops without a cap are bugs.
- Get guardian API clients from `Services::federation_api()` /
  `lookup_api()`, never one per request. Retry with
  `fedimint_core::util::backoff_util`, not hand-rolled loops.
- Respect the configured concurrency limits (`FO_WORKER_CONCURRENCY`,
  `FO_EXTERNAL_CONCURRENCY`, `FO_HTTP_CONCURRENCY`) rather than spawning
  unbounded tasks. A new limit or `FO_*` variable goes in `OPERATIONS.md`.
- Don't hold a lock or a database transaction across slow network calls.

## Database and migrations

- Use bound parameters (`$1`, `$2`) for every value. `format!` is only for
  trusted identifiers such as a module's own schema name, never for request
  or network data.
- Migrations are append-only: never edit a released migration; add a new one.
  They must work on a populated database from `MIN_UPGRADE_VERSION` and must
  not rewrite huge tables in one transaction; backfill in bounded steps.
- Writes that belong together (rows, cursor move, read-model update) go in one
  transaction. Ask what happens if the process stops between two writes.
- Add a `postgres_` regression test (see `OPERATIONS.md`) for new SQL logic,
  triggers or migrations.

## Frontend (`fmo_frontend_react/`)

- Go through `services/http.ts` / `services/api.ts` for API calls; keep types
  in `types/api.ts`.
- Handle loading, empty and error states, and paginate lists from the API
  rather than fetching everything.
- Never use `dangerouslySetInnerHTML` or `innerHTML` with data from the API,
  guardians, gateways or federation metadata: federation names and meta fields
  are untrusted. Validate URLs from that data (`https:` only) before using
  them in links or images.
- Support light and dark mode and phone widths.
