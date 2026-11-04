# Running the observer with bounded resources

Run one collector instance and any number of API instances against the same
PostgreSQL database. The default instance collects data and serves HTTP. Set
`FO_API_ONLY=true` on additional API instances. They do not run migrations,
collectors, projection, retention, or administrative writes.

The collector holds a dedicated PostgreSQL advisory lock for its lifetime. A
second collector exits rather than duplicate work. Loss of that connection
cancels the collector's tasks and stops its server. A heartbeat runs every five
seconds with a five second deadline to detect a silent connection failure. A process manager should
restart it. This is single active ownership, not automatic standby election.
Use a direct database connection or session pooling; transaction pooling cannot
preserve a session advisory lock. Budget one extra database connection for it.

## Upgrade

1. Take a database backup and verify a restore in an isolated database. For
   production, use PostgreSQL backups/WAL archiving to separate storage with a
   defined recovery point and recovery time. The development backup script is
   not an off-host disaster recovery system.
2. Stop the existing collector. Start the updated collector to apply core schema
   v13 and the wallet enrichment migration. Core v11 or newer is supported;
   older populated databases need the previous release's migrations first.
3. Allow time for the session counter backfill and new indexes to finish.
   Migrations run transactionally with no statement deadline and a 30 second
   lock deadline. These are regular index builds: plan a maintenance window on
   a large database, rather than assuming zero downtime.
4. Check `/ready`, then start API replicas with the same binary/module set.
   Deploy the matching frontend: balances and guardian block heights are now
   nullable. Route administrative requests to the collector.
5. Check snapshot collection and projection positions before directing traffic.
   Downgrading the binary after migration is refused; roll back by restoring a
   verified backup, or ship a forward fix.

## Limits

| Environment variable | Default | Purpose |
| --- | ---: | --- |
| `FO_API_ONLY` | `false` | Serve API without collectors or migrations |
| `FO_API_DB_CONNECTIONS` | `16` | Maximum API database connections per process |
| `FO_WORKER_DB_CONNECTIONS` | `8` | Maximum background database connections |
| `FO_WORKER_CONCURRENCY` | `4` | Concurrent ingestion/projection/backfill batches |
| `FO_EXTERNAL_CONCURRENCY` | `8` | Concurrent metadata, Wallet v2, and enrichment operations |
| `FO_HTTP_CONCURRENCY` | `128` | Concurrent application requests per process |
| `FO_HEALTH_RETENTION_DAYS` | `90` | Raw guardian history retention; `0` disables pruning |

Retention must be at least 31 days when enabled. Pruning deletes at most 5,000
raw samples per minute and preserves each guardian's latest sample. Hourly
rollups are retained. This bounds routine growth, but a large existing backlog
can take time to drain. Alert on disk usage and monitor autovacuum/bloat.

API connections have a 5 second statement deadline and 2 second pool wait;
background connections have a 120 second statement deadline and 10 second
pool wait. Both have 5 second lock and 60 second idle transaction deadlines.
These settings override URL `options`. Size the sum of pools across all
replicas below PostgreSQL's connection limit, leaving room for maintenance.
Use `sslmode=require` for remote databases and configure a trusted system CA.
The native TLS connector verifies certificates and hostnames; `require` prevents
plaintext fallback. This driver accepts `disable`, `prefer`, and `require`, not
libpq's `verify-full` spelling. Local Unix sockets also work. The container
includes system CA certificates.

Application requests have a 30 second deadline. Overload returns `503` with
`Retry-After`; a request deadline returns `504`. Config inspector requests also
have a separate concurrency limit of four. `/health`, `/live`, `/ready`, and
`/metrics` bypass admission. Put public TLS termination, per-client rate limits,
and access controls for `/metrics` and administrative paths at the reverse
proxy. Concurrency limits do not replace network egress policy for untrusted
federation URLs or public inspector endpoints.

## Latency and consistency

* Federation summaries and observed federation metadata use database snapshots,
  with no guardian HTTP/RPC on the request path. Metadata refreshes every five
  minutes after each pass; consensus fields take priority over external
  overrides, then configured fields. Failed sources preserve their last value.
* Wallet v2 balances and the latest 20 on-chain operations refresh in the
  background every minute after each poll. After five minutes without success,
  balance responses use `null` and history returns `503`. Successful empty
  wallets still have a zero balance. A new collector needs its first poll.
* Wallet v1 projection enqueues durable Bitcoin enrichment in the same database
  transaction as the signatures and cursor. A separate worker fetches Bitcoin
  transactions with a 30 second deadline, at most four at once, and retries with
  backoff. It holds no database connection during the fetch. A two minute lease
  recovers interrupted jobs; clean resets invalidate old job IDs. Derived
  withdrawals/UTXOs can lag the projector while enrichment is pending.
* Session and transaction lists default to 100 results and accept `limit=1..500`.
  Continue with `after=<last session index>` or `after=<last transaction ID>`.
  Ordering is ascending by cursor, and response shapes are unchanged. Clients
  that relied on downloading the entire list must now paginate.
* Session and transaction counts use incremental database read models. Session
  history is append-only: direct SQL deletion requires rebuilding its counter.
* Guardian latency measures a timed status RPC after a warm-up request. Missing
  Wallet v1 block height no longer makes Wallet v2 guardians appear offline.
  Latest health older than five minutes is not reported as currently online.
* The detail page fetches expensive UTXO comparisons only when the Assets tab is
  opened and does not repeat the whole page load when module detection finishes.

Successful federation GET responses permit ten seconds of cache reuse plus
thirty seconds of stale-while-revalidate. Gzip is available. Upstream Wallet v2
currently supplies its entire transaction chain per poll; persisting only 20
rows bounds public responses, not that upstream payload. Wallet v1 UTXO
comparison still performs bounded remote checks on a cold cache. These paths,
gateway aggregation, full view refreshes, and long historical backfills remain
candidates for profiling at production volume.

Use bounded ranges for administrative backfills. Each batch publishes restored
rows and read models atomically; cancellation rolls back the current batch.
Previously completed batches remain committed and can be safely replayed.
Module reset locks its cursor against active projection, including resets to
the same numeric position.

## Verification and monitoring

```sh
nix develop -c cargo test --workspace --locked
nix develop -c bash scripts/test-db.sh
cd fmo_frontend_react
npm ci
npm run lint
npm run build
```

The database test creates and deletes its own PostgreSQL cluster and never uses
`FO_DATABASE` or `.pg_dev`. It checks schema upgrades, idempotent session counts,
transaction rollback, duplicate collector rejection, lease loss, pool deadlines,
same-position reset locking, and enrichment reset fencing/retry eligibility.
HTTP unit tests check overload, probe availability, deadlines, and permit release.

`/live` (and legacy `/health`) is liveness. `/ready` checks the database and local
worker ownership; it does not assert that every federation is synchronized.
`/metrics` exposes HTTP request/error/rejection counters, a latency histogram,
pool sizes/availability/waiters, and local worker health. Requests receive an
`x-request-id`; unexpected internal errors are logged and sanitized for clients.

Monitor snapshot ages, queue backlog, and module lag alongside HTTP percentiles:

```sql
SELECT kind, max(now() - observed_at) AS oldest FROM federation_observations GROUP BY kind;
SELECT count(*) AS pending, min(retry_at) AS next_retry, max(attempts) FROM fmo_wallet.enrichment_jobs;
SELECT c.module_kind, encode(c.federation_id, 'hex') AS federation,
       s.session_count - c.next_session AS sessions_behind, c.updated_at
FROM module_cursors c JOIN federation_session_counts s USING (federation_id);
```

Before setting a production capacity or latency SLO, load-test a restored,
representative database with cold and warm caches, an enrichment backlog,
unreachable guardians, and several API replicas. Measure p50/p95/p99 latency,
rejections, pool waits, projection lag, database CPU/I/O, and recovery after
collector termination. Unit and integration tests establish invariants; they
do not establish a production throughput figure or disaster-recovery guarantee.
