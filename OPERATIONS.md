# Operating the observer

What an operator needs to deploy, upgrade and monitor the observer. How the
code works is documented in the code and in `AGENTS.md`.

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
   schema (currently v14) and the module migrations. Populated databases upgrade
   from core v11 or newer; older ones need the previous release's migrations
   first.
3. Allow time for backfills and index builds. Migrations run transactionally
   with no statement deadline and a 30 second lock deadline, and the indexes are
   regular builds: plan a maintenance window on a large database. Some module
   migrations reset only their own projector cursor to backfill from stored
   sessions (Wallet v2 deposits, Lightning v2 contract activity); the gateway
   API reports `activity_caught_up` once that replay has finished.
4. Check `/ready`, then start API replicas with the same binary, module set and
   sources file (see [Data sources](#data-sources)), and deploy the matching
   frontend.
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

## Admin access

Adding a federation, backfills and module resets are administrative requests,
served by the collector only. Two kinds of admins exist (`fmo_core/src/admin`):

- **Owners** run the observer and can do everything, including granting and
  revoking access. They are whoever holds `FO_ADMIN_AUTH` or a key in
  `FO_ADMIN_NPUBS`. Owners are configuration, never database rows: restoring a
  database can't change them.
- **Delegates** are Nostr keys an owner granted access to, on the frontend's
  `/admin` page or with `PUT /admin/keys/:pubkey`. A delegate can add
  federations, at most 20 attempts in any 24 hours, and nothing else. At most 64
  keys hold a grant. Revoking (`DELETE /admin/keys/:pubkey`) or a grant's expiry
  takes effect with the delegate's next request; there are no sessions to end.

| Environment variable | Default | Purpose |
| --- | ---: | --- |
| `FO_ADMIN_AUTH` | required | Bearer token with owner rights, for scripts. A long random secret, for example from `openssl rand -hex 32`; an empty one is refused |
| `FO_ADMIN_NPUBS` | unset | Owners' Nostr public keys, `npub1…` or hex, separated by commas; at most 16. Needs `FO_PUBLIC_API_URL` |
| `FO_PUBLIC_API_URL` | unset | The API's base URL exactly as browsers reach it, the frontend's `VITE_FMO_API_BASE_URL` (`https://observer.example.com/api`). Setting it enables signing in with Nostr; unset, only the bearer token works. Must be `https`, or `http` on a loopback address |

Admins who sign in with Nostr send no password: their browser extension signs a
NIP-98 event for each request, naming its URL, method and body hash. The
observer accepts an event for 60 seconds around its timestamp and only once, so
keep the server's clock synchronized. The URL is checked against
`FO_PUBLIC_API_URL` and never against the `Host` header; if admin requests fail
with "signed for a different URL", this variable doesn't match the URL the
frontend calls. Give API replicas the same three variables: they authenticate
admins the same way before refusing them with `503`.

`GET /admin/audit` (owners) lists every administrative action with who did it,
kept indefinitely in `admin_audit_log`. Entries are written before the action
runs and get their outcome after. An entry left `started`, shown as "outcome
not recorded", was cut off, for example by the request deadline: the action
may or may not have happened, so check the actual state (is the federation
observed, did the backfill run) before trying it again.

At the reverse proxy:

- Serve admin requests over TLS only, and on the frontend's own origin, or
  allow `PUT`, `DELETE` and the `Authorization` header in its CORS policy.
- Never cache admin answers. The observer sends `Cache-Control: no-store` on
  everything under `/admin/*`, on `PUT /federations`, the backfill and module
  reset routes, and on every refused admin request. Nginx honors it unless
  `proxy_ignore_headers` lists `Cache-Control`; a cache keyed by URL would
  otherwise hand one admin's answer to anyone. Public reads keep their caching.
- Rate limit `/admin/*` and `PUT /federations` per client. A refused request
  costs the observer one signature check and at most one indexed lookup.
- Don't log the `Authorization` header. A signed event is useless after a
  minute, but the bearer token is not.

Owners should use a Nostr key kept for administration, and approve "HTTP auth"
(kind 27235) signing requests only on the observer's own site: any site can ask
an extension to sign one, and a signed request works from anywhere for a
minute.

To take all access from a key that is lost or leaked:

1. Remove it from `FO_ADMIN_NPUBS`, if it is listed there, and restart the
   collector and every API replica.
2. Revoke its grant, if it has one, on the admin page's Access tab or with
   `DELETE /admin/keys/:pubkey`. A key granted delegate access before it was
   made an owner keeps that grant when it stops being an owner. The Access tab
   marks such keys "also an owner", and the collector logs a warning for each
   at startup.

Its next request is refused. Requests it signed in the last minute were
already accepted or are refused as used. If the token leaks, change
`FO_ADMIN_AUTH` and restart.

## Data sources

Services outside the build push observations about federations to
`PUT /sources/:source/federations/:federation_id/:kind` (see `ARCHITECTURE.md`).
Push sources are declared at deploy time:

| Environment variable | Default | Purpose |
| --- | ---: | --- |
| `FO_SOURCES_FILE` | unset | Path of the JSON document declaring the push sources and their kinds (format in `fmo_core/src/source/config.rs`); unset declares none |
| `FO_SOURCE_TOKEN_<ID>` | none | Bearer token of the push source `<id>`, its id in upper case (`FO_SOURCE_TOKEN_UPTIME_PROBE`); 32 to 512 bytes, for example from `openssl rand -hex 32` |

The file holds no secrets; the tokens do, so keep them out of it and out of
logs. Startup fails if a declared source has no token or a short one.

Give the collector and every API replica the same sources file and tokens. Any
instance accepts pushes and serves `GET /federations/:id/sources`, so a replica
without them answers that source's pushes with `401` and leaves its public
observations out. The collector deletes observations past their kind's
retention, and those of sources or kinds it no longer declares 30 days after
their last write; it does so whether or not it declares any sources. `/metrics`
counts each source's writes in
`fmo_source_observations_total{source,result}`, with `result` one of `stored`,
`superseded` or `rejected`.

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
| `FMO_SOURCES_TEST_DATABASE` | `cargo test -p fmo_core source::tests::postgres_ -- --ignored` |
| `FMO_ADMIN_TEST_DATABASE` | `cargo test -p fmo_core admin::tests::postgres_ -- --ignored` |

`fmo_core/tests/run-session-time-repair.sh` creates and drops its own database
to check upgrades from v11; set `PGHOST`, `PGPORT`, and `PGUSER` to a disposable
instance first. A read-only Iroh smoke test takes `FMO_LNV2_LIVE_FEDERATION` and
`FMO_LNV2_LIVE_GATEWAY`:
`cargo test -p fmo_module_lnv2 public_gateway_routing_info -- --ignored --nocapture`.
