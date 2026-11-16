---
name: observer-security
description: Use before changing Fedimint Observer API endpoints, admin authentication, outbound connections to guardians or gateways, or anything that publishes federation, gateway or user data.
---

# Observer Security and Privacy

The observer is a public service that connects to addresses it learns from
untrusted sources. Treat every input as hostile.

## Privacy

Fedimint's value rests on ecash privacy, and the observer is public. Never
publish data that can tell who holds or moves how many sats.

- For any new metric or endpoint, ask whether it can point at one user or one
  operator. Publish totals, percentiles and buckets, not individual rows tied
  to a time.
- No lookups by payment hash or other per-payment identifiers.
- Bucket and delay per-gateway flows and liquidity; suppress figures built
  from too few transactions, as in small federations.
- Describe the privacy impact in the PR when a change publishes new data.

## Untrusted inputs

- Guardian responses, gateway responses, federation metadata, Nostr events and
  invite codes are untrusted: validate them, bound their size and count, and
  handle malformed data without panicking.
- Use bound SQL parameters for all of them (see `observer-coding-style`).
- The frontend renders them as text only.

## Outbound connections (SSRF)

- Invite codes, guardian-reported gateway URLs and published Iroh records can
  point anywhere, including private addresses. HTTPS probes must stay pinned
  to public addresses using the policy in
  `fmo_modules/fmo_module_lnv2/src/public_http.rs`; reuse it for any new
  outbound HTTP client.
- Set timeouts on every outbound call and cap response sizes.
- If a change adds a new kind of outbound connection, update "Egress policy" in
  `OPERATIONS.md`.

## Authentication and admin endpoints

- Admin endpoints require the `FO_ADMIN_AUTH` bearer token, compared in
  constant time (`subtle::ConstantTimeEq`), as in
  `fmo_core/src/federation/observer.rs`. Every new endpoint that changes state
  or triggers work must be admin-only.
- Never log, return or embed secrets (`FO_ADMIN_AUTH`, database URLs).

## Availability

- A single request must not be able to trigger unbounded work. Keep request
  work within the 30-second deadline and the pagination limits.
- Expensive or work-triggering endpoints must be admin-only or served from
  precomputed data.
- Public rate limiting and TLS are done at the reverse proxy (see
  `OPERATIONS.md`); don't rely on them to bound in-process work.
