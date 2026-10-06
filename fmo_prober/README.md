# fmo_prober

A standalone Lightning prober that measures whether Fedimint gateways can actually be paid over Lightning. It runs next to a funded LND node, probes the LN nodes of every gateway known to Fedimint Observer, and pushes the results to Observer. Observer shows them on the gateway details page (`/federations/:id/gateways/:gatewayId`).

The prober is a separate process. It only talks to Observer over HTTP, so the two can be deployed and changed independently.

```
Lightning Network ──probes──▶ fmo_prober (funded LND) ──POST /gateways/probes──▶ Observer ──▶ gateway details page
```

## How probing works

A probe is a payment to the gateway's LN node with a **random payment hash**. Nobody knows the preimage, so the payment can never settle. No funds move and no routing fees are paid. *Where* and *why* the HTLC fails is the measurement:

| Outcome | Meaning | Counts toward gateway score |
|---|---|---|
| `success` | The gateway node itself rejected the payment (usually `INCORRECT_OR_UNKNOWN_PAYMENT_DETAILS`). It is reachable and can receive the amount. | yes |
| `insufficient_liquidity` | `TEMPORARY_CHANNEL_FAILURE` from the gateway's channel peer: the channel into the gateway can't carry the amount. | yes |
| `unreachable` | `UNKNOWN_NEXT_PEER`, `CHANNEL_DISABLED` or `PERMANENT_CHANNEL_FAILURE` on the last hop: the gateway is offline or its channels are disabled. | yes |
| `route_failure` | An intermediate hop failed the HTLC. | yes |
| `no_route` | LND found no route to the gateway. | yes |
| `local_failure` | Failed at the prober's own node, or the amount exceeds what the prober can send. | **no** |
| `timeout` / `error` | The HTLC didn't resolve in time, or the LND API failed. | **no** |

For each gateway the prober sends an **amount ladder** (default 10k / 100k / 1M sat) in ascending order and stops at the first amount that doesn't succeed. Observer measures routing success on the base (smallest) amount only. The largest amount that succeeded is a lower bound on the gateway's inbound liquidity.

Probes run **one at a time**. If several were in flight at once, they would lock up the prober's own outbound liquidity and cause failures that have nothing to do with the gateways.

## Running

```bash
# Observer side: enable ingestion
export FO_PROBER_AUTH=<random secret>

# Prober side
export FMP_OBSERVER_URL=https://observer.example/api
export FMP_OBSERVER_AUTH=<same secret>
export FMP_PROBER_ID=fra-1                 # identifies this prober/location
export FMP_LND_REST_URL=https://127.0.0.1:8080
export FMP_LND_MACAROON=/path/to/prober.macaroon
export FMP_LND_TLS_CERT=/path/to/tls.cert  # pinned, see below
just run_prober                            # or: cargo run -p fmo_prober
```

| Variable | Default | |
|---|---|---|
| `FMP_PROBE_INTERVAL_SECS` | `1800` | Time between the start of two rounds |
| `FMP_PROBE_AMOUNTS_SAT` | `10000,100000,1000000` | Amount ladder |
| `FMP_MAX_FEE_SAT` | `1000` | Fee limit for route selection (never paid) |
| `FMP_PROBE_TIMEOUT_SECS` | `60` | Time to wait for an HTLC to resolve |
| `FMP_NODE_ALLOWLIST` | all | Comma-separated node pubkeys, e.g. to start with a few gateways |

**Macaroon**: a baked macaroon with `info:read offchain:read offchain:write` is enough. You don't need `admin.macaroon`:

```bash
lncli bakemacaroon info:read offchain:read offchain:write --save_to prober.macaroon
```

**TLS**: LND's auto-generated `tls.cert` is self-signed and marked as a CA, and rustls rejects that as a server certificate. When `FMP_LND_TLS_CERT` is set, the prober pins that exact certificate. It still verifies the handshake signatures and never disables verification. Without it, normal webpki roots are used (e.g. behind a reverse proxy with a public certificate).

## Open questions from the design issue

**How much liquidity does the prober need?** Probes never settle, so nothing is spent. Liquidity is only locked while a probe is in flight. The prober needs outbound capacity of at least the largest ladder amount plus `FMP_MAX_FEE_SAT` **in a single channel**, because probes use a single path. One or two well-connected channels of about 1.5M sat each are enough for the default ladder. Amounts above the largest per-channel spendable balance are recorded as `local_failure` (`INSUFFICIENT_LOCAL_BALANCE`) without being sent.

**How is that liquidity bootstrapped and maintained?** Open channels to large, well-connected routing nodes. Probing doesn't drain balances, so the channels don't need rebalancing. The only operational risk is a stuck HTLC: a misbehaving hop can hold it until its CLTV expiry, which locks that amount for up to roughly two weeks in the worst case. In that case later probes for that amount fail locally, and those failures are excluded from gateway scores.

**How are prober failures told apart from gateway failures?** By the failing hop's index: 0 is the prober, the last index is the gateway's channel peer, and anything in between is an intermediate node. In addition, the prober checks its own spendable balance before each round. See the outcome table above.

**How often should gateways be probed, and with which amounts?** The defaults are every 30 minutes with 10k / 100k / 1M sat. At that rate, a few hundred gateways take well under the interval to probe sequentially, and LN nodes see very little extra load.

**How accurate is the liquidity estimate?** It is only a lower bound, with the granularity of the ladder. It also reflects the route the prober's LND picked from its position in the graph: another sender might find a different channel into the gateway with more inbound liquidity. A finer estimate (binary search between the last success and the first failure) is a possible follow-up.

**One prober or several?** Results come from one vantage point and depend on that node's channels and routing. Every result carries a `prober_id`, so more probers in other locations can submit to the same Observer. Per-prober breakdowns aren't shown yet.

**Existing tools?** This is the same technique as LND's own route-fee estimation and tools built on `QueryRoutes` + `SendToRouteV2`. Keeping the prober minimal (one REST client, one classifier) avoids extra dependencies, and the LND client is isolated in `src/lnd.rs`, so other backends could be added later.

## Development

```bash
cargo test -p fmo_prober   # classification and LND JSON parsing tests
```
