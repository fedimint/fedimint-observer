# Operating the observer

What an operator needs to deploy, upgrade and monitor the observer. How the
code works is documented in the code and in `CLAUDE.md`.

## Processes

Run one collector instance and any number of API instances against the same
PostgreSQL database. The default instance collects data and serves HTTP. Set
`FO_API_ONLY=true` on additional API instances: they do not run migrations,
collectors, projection, retention, or administrative writes. Route
administrative requests to the collector.

The collector holds a PostgreSQL advisory lock on a dedicated connection for its
lifetime. A second collector exits rather than duplicate work, and losing that
connection stops the collector, so a process manager should restart it. This is
single active ownership, not automatic standby election. Use a direct database
connection or session pooling (transaction pooling cannot keep a session lock),
and budget one extra database connection for it.

The server reads its settings from the environment and does not load `.env`
itself. For local development:

```sh
source scripts/pg_dev/env.sh
set -a; source .env; set +a
just run
```

## Upgrade

1. Take a database backup and verify a restore in an isolated database.
2. Stop the existing collector and start the updated one. It applies the core
   schema (currently v12) and the module migrations. Populated databases upgrade
   from core v11 or newer; older ones need the previous release's migrations
   first.
3. Allow time for backfills and index builds. Migrations run transactionally
   with no statement deadline and a 30 second lock deadline, and the indexes are
   regular builds: plan a maintenance window on a large database. Some module
   migrations reset only their own projector cursor to backfill from stored
   sessions (Wallet v2 deposits, Lightning v2 contract activity); the gateway
   API reports `activity_caught_up` once that replay has finished.
4. Check `/ready`, then start API replicas with the same binary and module set,
   and deploy the matching frontend.
5. Downgrading the binary after migration is refused. Roll back by restoring a
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
| `FO_HEALTH_RETENTION_DAYS` | `90` | Raw guardian history and Lightning v2 gateway health retention; `0` disables pruning |

Retention must be at least 31 days when enabled. Size the sum of the pools
across all replicas below PostgreSQL's connection limit, leaving room for
maintenance. Use `sslmode=require` for remote databases with a trusted system
CA; the driver accepts `disable`, `prefer`, and `require`, not libpq's
`verify-full` spelling.

Application requests have a 30 second deadline. Overload returns `503` with
`Retry-After`; a request deadline returns `504`. `/health`, `/live`, `/ready`,
and `/metrics` bypass admission. Put public TLS termination, per-client rate
limits, and access controls for `/metrics` and administrative paths at the
reverse proxy.

Lightning v2 gateway data comes from guardians and gateways and is bounded:
256 gateway URLs per guardian per poll, 1,024 registered and 1,024 retired
gateways per federation module instance, and probe history kept for
`FO_HEALTH_RETENTION_DAYS` (90 days by default). Contract
activity is attributed by the module key a gateway reports about itself, which
the observer cannot verify; a key claimed by more than one gateway is shown as
contested and credited to neither.

## Egress policy

Gateway endpoints come from guardians, and their Iroh addresses from discovery
records the gateway operator publishes. HTTPS probes are pinned to public
addresses in code; Iroh probes are not, so a published record can direct QUIC
handshake packets to private addresses. Authentication stops another node from
posing as the named gateway, but it does not stop those packets being sent or
handshake and discovery replies being received. The
public config inspector also connects to guardian URLs from user-supplied invite
codes. Enforce this at the network layer: run the observer as a dedicated user,
container or network namespace and drop its outbound traffic to non-public
destinations, allowing first the few internal services it needs. With nftables,
for example:

```
table inet fmo_egress {
  chain output {
    type filter hook output priority 0; policy accept;
    meta skuid != "fmo" accept
    # Replies on connections already allowed, including API responses to a
    # reverse proxy on loopback. Only new outbound connections are restricted.
    ct state established,related accept
    ip daddr 10.0.0.53 udp dport 53 accept   # resolver
    ip daddr 10.0.0.53 tcp dport 53 accept
    ip daddr 10.0.0.20 tcp dport 5432 accept # PostgreSQL
    oifname "lo" tcp dport 5432 accept       # or a local PostgreSQL
    ip daddr { 0.0.0.0/8, 10.0.0.0/8, 100.64.0.0/10, 127.0.0.0/8, 169.254.0.0/16,
               172.16.0.0/12, 192.0.0.0/24, 192.0.2.0/24, 192.88.99.0/24,
               192.168.0.0/16, 198.18.0.0/15, 198.51.100.0/24, 203.0.113.0/24,
               224.0.0.0/4, 240.0.0.0/4 } drop
    ip6 daddr != 2000::/3 drop
    ip6 daddr { 2001::/23, 2001:db8::/32, 2002::/16, 3fff::/20 } drop
  }
}
```

The `ct state` rule must come before the destination drops: the observer's
replies to a reverse proxy (for example Nginx on `127.0.0.1`) are outbound
packets from the observer's user, and without it they would hit the
`127.0.0.0/8` drop. A new connection to a private address is still dropped on
its first packet, so it never becomes established. Replace the example
addresses with your resolver and database, and add any other internal
dependency (a self-hosted mempool instance, metrics push) before the drop
rules. These ranges match the HTTPS policy in
`fmo_modules/fmo_module_lnv2/src/public_http.rs`. Check the rules after enabling
them: probes should keep succeeding for public gateways, the observer should
still reach its database and resolver, and the API should still answer through
the proxy.

## Monitoring

`/live` (and legacy `/health`) is liveness. `/ready` checks the database and
local worker ownership; it does not assert that every federation is
synchronized. `/metrics` exposes HTTP request, error and rejection counters, a
latency histogram, pool usage, and local worker health. Responses carry an
`x-request-id`.

Monitor snapshot ages, queue backlog, and module lag alongside HTTP percentiles:

```sql
SELECT kind, max(now() - observed_at) AS oldest FROM federation_observations GROUP BY kind;
SELECT count(*) AS pending_timestamp_repairs FROM session_time_repairs;
SELECT count(*) AS pending, min(retry_at) AS next_retry, max(attempts) FROM fmo_wallet.enrichment_jobs;
SELECT count(*) AS pending, min(retry_at) AS next_retry FROM fmo_walletv2.deposit_jobs;
SELECT c.module_kind, encode(c.federation_id, 'hex') AS federation,
       s.session_count - c.next_session AS sessions_behind, c.updated_at
FROM module_cursors c JOIN federation_session_counts s USING (federation_id);
```

Also alert on disk usage: raw guardian samples are pruned gradually, so a large
backlog takes time to drain.

## Frontend delivery

Serve `robots.txt` and the generated `llms.txt` as files. Return a real `404`
when either is absent so the SPA fallback does not serve HTML. The build uses
its API base URL in `llms.txt`; pass an absolute public URL for deployments.
Use `Cache-Control: no-cache` for HTML and long-lived immutable caching for
hashed `/assets/` files. Nix store files share
timestamps, so disable mtime-based conditional responses for HTML to avoid a
false `304` between two builds. For Nginx:

```nginx
location = /robots.txt { try_files $uri =404; }
location = /llms.txt { try_files $uri =404; }
location = /index.html {
    add_header Cache-Control "no-cache" always;
    etag off;
    if_modified_since off;
}
location / {
    add_header Cache-Control "no-cache" always;
    etag off;
    if_modified_since off;
    try_files $uri $uri/ /index.html;
}
```

Keep `/api/` proxy routing and its cache policy separate, and validate with
`nginx -t` before reloading.

## Tests

```sh
nix develop -c cargo test --workspace --locked
cd fmo_frontend_react && npm ci && npm run lint && npm run build
```

The database regressions are ignored by default and run in CI. Each needs an
empty, isolated database, rolls back its fixtures, and does not use
`FO_DATABASE`:

| Variable | Command |
| --- | --- |
| `FMO_WORK_TEST_DATABASE` | `cargo test -p fmo_core postgres_ -- --ignored` |
| `FMO_WALLETV2_TEST_DATABASE` | `cargo test -p fmo_module_walletv2 postgres_ -- --ignored` |
| `FMO_LNV2_TEST_DATABASE` | `cargo test -p fmo_module_lnv2 postgres_ -- --ignored` |

`fmo_core/tests/run-session-time-repair.sh` creates and drops its own database
to check upgrades from v11; set `PGHOST`, `PGPORT`, and `PGUSER` to a disposable
instance first. A read-only Iroh smoke test takes `FMO_LNV2_LIVE_FEDERATION` and
`FMO_LNV2_LIVE_GATEWAY`:
`cargo test -p fmo_module_lnv2 public_gateway_routing_info -- --ignored --nocapture`.
