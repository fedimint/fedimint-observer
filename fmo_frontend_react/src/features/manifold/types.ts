/**
 * Manifold discovery data as the observer's collector verified and stored it
 * (fmo_core `manifold`, mirrored from fmo_api_types), following the Manifold
 * specifications (fedibtc/manifold):
 * - MG advertisements, Nostr kinds 37701 (legacy) and 37709 (v2):
 *   `crates/nostr/specs/SPEC-mg-nostr-events.md`
 * - Trust credentials (PeerBadge holder authorizations):
 *   `SPEC-holder-trust-envelope.md`, `SPEC-peer-badge-verifier.md`
 * - FLIP provider advertisements, kind 37702: `SPEC-flip-advertisement.md`
 *
 * Seat bindings are not here: the observer verifies those itself, from
 * consensus meta (`ManifoldSeatBindingsReport` in types/api.ts).
 */

/** A discovery answer with its provenance */
export interface Sourced<T> {
  /** The collector's last successful relay sync (RFC 3339); null before the first */
  observed_at: string | null;
  /** The relays it reads */
  relays: string[];
  data: T;
  /** Where a paged list stands; list answers only */
  page?: { next_after: string | null; total: number };
  /** The directory's filter choices, over every listed guardian */
  facets?: { guardians: number; versions: string[]; sizes: number[] };
}

/**
 * Advertisement freshness by the FA's rule: fresh only while
 * `issued_at <= now < expires_at` and `now - issued_at <= 2h`.
 */
export type AdFreshness = 'fresh' | 'stale' | 'expired' | 'not_yet_valid';

/** `Plan`, externally tagged in the payload; prices in msat */
export type MgPlan =
  | { kind: 'infinite_best_effort'; price_msats: number }
  | { kind: 'subscription'; initial_price_msats: number; renewal_price_msats: number; period: string };

export interface MgAdvertisement {
  /** 37709 (v2) when a fresh one exists, else 37701; fields are never merged across the two */
  event_kind: 37701 | 37709;
  event_id: string;
  /** Unix seconds */
  issued_at: number;
  expires_at: number;
  /** Payload and event signatures both verified under the author's key */
  signature_valid: boolean;
  service_pubkey: string;
  /** e.g. iroh://<endpoint-id> */
  api_endpoints: { transport: 'iroh'; url: string }[];
  availability: {
    /** SemVer; only `+fedi` builds are admitted by the FA */
    fedimintd_version: string;
    federation_sizes: number[];
  };
  plans: MgPlan[];
  /** v2 only, and asserted by the MG itself: display, never evidence. Absent = unreported */
  self_reported_uptime_permille: { '1d'?: number; '7d'?: number; '30d'?: number } | null;
  /** v2 only, self-asserted */
  software: { mg_version: string; api_version: number } | null;
  /** Holder authorizations carried in the ad */
  holder_authorization_count: number;
}

/** Outcome of checking a PeerBadge trust credential, fail-closed */
export type CredentialState =
  /** Every required check passed */
  | 'verified'
  /** A check failed: insufficient level, revoked, wrong subject, bad proof, ... */
  | 'failed'
  /** Could not complete the checks (e.g. revocation lookup incomplete), so not verified */
  | 'unavailable'
  /** The advertisement carries no credential */
  | 'none';

export interface CredentialCheck {
  id: string;
  label: string;
  /** null when the check could not run */
  passed: boolean | null;
  detail?: string;
}

export interface TrustCredential {
  state: CredentialState;
  /** Issuer key, when a credential was presented */
  issuer_pubkey: string | null;
  /** The issuer's name in a pinned canonical profile; none is pinned yet */
  issuer_label: string | null;
  /** The operator: the PeerBadge holder key. Never the MG key */
  holder_pubkey: string | null;
  trust_level: number | null;
  /** Minimum accepted level (9 in every canonical environment) */
  required_trust_level: number;
  schema: string | null;
  checked_at: string | null;
  checks: CredentialCheck[];
  /** Why the state is not `verified`, in a sentence */
  reason: string | null;
}

/** A seat the MG operates, only from a verified `fedi:fman_seat_bindings` directory */
export interface OperatedSeat {
  federation_id: string;
  federation_name: string | null;
  peer_id: number;
  /** Guardian seats in the federation's config, for operator-concentration checks */
  federation_guardians: number;
  /** Unix seconds the binding was signed */
  bound_at: number;
  /** When the observer last verified the directory */
  verified_at: string;
}

export interface VersionChange {
  observed_at: string;
  fedimintd_version: string;
  mg_version: string | null;
}

/** Advertisement presence per UTC day, from the observer's own relay polls */
export interface AdObservationDay {
  date: string;
  /** Polls that found a fresh advertisement, of all polls that day */
  fresh_polls: number;
  polls: number;
}

export interface ManifoldGuardianSummary {
  /** The MG identity: lowercase x-only hex, the Nostr author of its ads */
  fman_pubkey: string;
  /** Manifold's deterministic `MgName` for the key; names can collide */
  display_name: string;
  advertisement: MgAdvertisement | null;
  freshness: AdFreshness | null;
  credential: TrustCredential;
  /** Seats from verified seat bindings */
  seat_count: number;
  federation_count: number;
}

export interface ManifoldGuardianProfile extends ManifoldGuardianSummary {
  nostr_relays: string[];
  seats: OperatedSeat[];
  version_history: VersionChange[];
  observation_history: AdObservationDay[];
}

/** Advertised liquidity sources, e.g. 'gateway' or 'stability_pool' */
export type FlipSource = string;

export interface FlipProvider {
  provider_pubkey: string;
  event_id: string;
  issued_at: number;
  expires_at: number;
  freshness: AdFreshness;
  signature_valid: boolean;
  /** Self-asserted and optional; shown as the provider's own words */
  display: { name: string | null; website: string | null; contact: string | null } | null;
  supported_sources: FlipSource[];
  supported_networks: string[];
  accepted_attester_policies: { attester_pubkey: string; verification_requirement: string }[];
  api_endpoints: string[];
  api_versions: number[];
  relay_hints: string[];
  credential: TrustCredential;
  /** Federations where the provider's service was independently confirmed; empty when none */
  confirmed_federations: unknown[];
}
