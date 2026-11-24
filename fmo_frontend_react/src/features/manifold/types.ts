


export interface Sourced<T> {

  observed_at: string | null;

  relays: string[];
  data: T;

  page?: { next_after: string | null; total: number };

  facets?: { guardians: number; versions: string[]; sizes: number[] };
}


export type AdFreshness = 'fresh' | 'stale' | 'expired' | 'not_yet_valid';


export type MgPlan =
  | { kind: 'infinite_best_effort'; price_msats: number }
  | { kind: 'subscription'; initial_price_msats: number; renewal_price_msats: number; period: string };

export interface MgAdvertisement {
  /** 37709 (v2) when a fresh one exists, else 37701; fields are never merged across the two */
  event_kind: 37701 | 37709;
  event_id: string;

  issued_at: number;
  expires_at: number;

  signature_valid: boolean;
  service_pubkey: string;

  api_endpoints: { transport: 'iroh'; url: string }[];
  availability: {

    fedimintd_version: string;
    federation_sizes: number[];
  };
  plans: MgPlan[];
  /** v2 only, and asserted by the MG itself: display, never evidence. Absent = unreported */
  self_reported_uptime_permille: { '1d'?: number; '7d'?: number; '30d'?: number } | null;

  software: { mg_version: string; api_version: number } | null;

  holder_authorization_count: number;
}


export type CredentialState =

  | 'verified'

  | 'failed'

  | 'unavailable'

  | 'none';

export interface CredentialCheck {
  id: string;
  label: string;

  passed: boolean | null;
  detail?: string;
}

export interface TrustCredential {
  state: CredentialState;

  issuer_pubkey: string | null;

  issuer_label: string | null;
  /** The operator: the PeerBadge holder key. Never the MG key */
  holder_pubkey: string | null;
  trust_level: number | null;

  required_trust_level: number;
  schema: string | null;
  checked_at: string | null;
  checks: CredentialCheck[];

  reason: string | null;
}


export interface OperatedSeat {
  federation_id: string;
  federation_name: string | null;
  peer_id: number;

  federation_guardians: number;

  bound_at: number;

  verified_at: string;
}

export interface VersionChange {
  observed_at: string;
  fedimintd_version: string;
  mg_version: string | null;
}


export interface AdObservationDay {
  date: string;

  fresh_polls: number;
  polls: number;
}

export interface ManifoldGuardianSummary {

  fman_pubkey: string;

  display_name: string;
  advertisement: MgAdvertisement | null;
  freshness: AdFreshness | null;
  credential: TrustCredential;

  seat_count: number;
  federation_count: number;
}

export interface ManifoldGuardianProfile extends ManifoldGuardianSummary {
  nostr_relays: string[];
  seats: OperatedSeat[];
  version_history: VersionChange[];
  observation_history: AdObservationDay[];
}


export type FlipSource = string;

export interface FlipProvider {
  provider_pubkey: string;
  event_id: string;
  issued_at: number;
  expires_at: number;
  freshness: AdFreshness;
  signature_valid: boolean;

  display: { name: string | null; website: string | null; contact: string | null } | null;
  supported_sources: FlipSource[];
  supported_networks: string[];
  accepted_attester_policies: { attester_pubkey: string; verification_requirement: string }[];
  api_endpoints: string[];
  api_versions: number[];
  relay_hints: string[];
  credential: TrustCredential;

  confirmed_federations: unknown[];
}
