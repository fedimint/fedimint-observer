//! Verification of Manifold discovery events, as Manifold's consumer pipeline
//! does it (`crates/nostr/specs/SPEC-mg-nostr-events.md`,
//! `SPEC-flip-advertisement.md` in fedibtc/manifold):
//!
//! - kind 37701, MG advertisement: `{payload, proof}`, a BIP-340 signature by
//!   `payload.fman_id_pubkey` over `SHA256("fedi-fman-advertisement/v1\0" ||
//!   JCS(payload))`
//! - kind 37709, MG advertisement v2: a signed JSON envelope `{signer, payload,
//!   signature}`, signed over `SHA256("fedi-signed-json/mg- advertisement/v1\0"
//!   || payload)` with the payload's exact bytes
//! - kind 37702, FLIP provider advertisement: `{payload, proof}` like 37701,
//!   under `fedi-flip-advertisement/v1\0`, by `payload.provider_pubkey`
//!
//! On top of the outer Nostr event signature, every inner signer must be the
//! event author. Passing proves who published the offer, nothing more: an
//! advertisement is never evidence of federation membership, and its embedded
//! trust credentials are evaluated separately ([`evaluate_credential`],
//! [`assess_credential`]).

use std::collections::BTreeMap;

use bitcoin::hashes::{sha256, Hash, HashEngine};
use bitcoin::secp256k1::{schnorr, Message, Secp256k1, XOnlyPublicKey};
use fmo_api_types::{CredentialCheck, CredentialState, TrustCredential};
use nostr_sdk::Event;
use peerbadge_protocol::{
    CredentialDigest, HolderAuthorization, SignedCredential, VerificationContext,
};
use peerbadge_schemas::parse_trust_score_badge_v1;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::peerbadge::{
    digest_wire, issuer_label, pinned_authorities, revocation_locations, RevocationGap,
    RevocationKind, TrustContext,
};
use super::seat_bindings::{canonical_json, decode_signature};

pub const MG_LEGACY_KIND: u16 = 37701;
pub const MG_V2_KIND: u16 = 37709;
pub const FLIP_KIND: u16 = 37702;

const MG_LEGACY_DOMAIN: &[u8] = b"fedi-fman-advertisement/v1\0";
const MG_V2_PURPOSE: &[u8] = b"fedi-signed-json/mg-advertisement/v1\0";
const FLIP_DOMAIN: &[u8] = b"fedi-flip-advertisement/v1\0";
/// `SIGNED_JSON_ENVELOPE_MAX_BYTES`; also the per-event cap for the others
const MAX_CONTENT_BYTES: usize = 64 * 1024;
/// `SIGNED_JSON_PAYLOAD_MAX_BYTES`
const MAX_PAYLOAD_BYTES: usize = 65_536;
/// The FA's `MG_ADVERTISEMENT_MAX_AGE`
pub const MAX_AD_AGE_SECONDS: u64 = 2 * 60 * 60;
/// Every canonical Manifold environment requires this trust level
pub const REQUIRED_TRUST_LEVEL: u64 = 9;
/// The last second of year 9999. A signed timestamp past it is not a real
/// date, and bounding them keeps every timestamp and the differences of two
/// far inside PostgreSQL's `BIGINT`.
pub const MAX_TIMESTAMP: u64 = 253_402_300_799;

/// `d` and `t` tags each kind must carry
pub fn kind_tags(kind: u16) -> Option<(&'static str, &'static str)> {
    match kind {
        MG_LEGACY_KIND => Some(("fman-ad", "fedi-fman")),
        MG_V2_KIND => Some(("mg-ad", "fedi-mg")),
        FLIP_KIND => Some(("flip-provider-ad", "fedi-flip")),
        _ => None,
    }
}

/// A discovery event whose signatures and author binding all verified
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedAdvertisement {
    pub kind: u16,
    /// Lowercase x-only hex
    pub author: String,
    pub event_id: String,
    pub created_at: u64,
    pub issued_at: u64,
    pub expires_at: u64,
    /// The signed payload, as a JSON object
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryError {
    UnknownKind,
    MissingTags,
    InvalidEvent,
    TooLarge,
    Malformed,
    AuthorMismatch,
    InvalidSignature,
    /// Correctly signed, but not storable: a timestamp past
    /// [`MAX_TIMESTAMP`], or a NUL character, which PostgreSQL's `JSONB`
    /// cannot hold. Rejected here, it never reaches a sync's transaction.
    Unstorable,
}

/// Verifies one relay event end to end
pub fn verify_event(event: &Event) -> Result<VerifiedAdvertisement, DiscoveryError> {
    let kind = event.kind().as_u16();
    let (d_tag, t_tag) = kind_tags(kind).ok_or(DiscoveryError::UnknownKind)?;
    let has_tag = |name: &str, value: &str| {
        event.tags().iter().any(|tag| {
            let parts = tag.as_vec();
            parts.len() >= 2 && parts[0] == name && parts[1] == value
        })
    };
    // Other applications publish under the same kinds; only these tags mark
    // a Manifold advertisement
    if !has_tag("d", d_tag) || !has_tag("t", t_tag) {
        return Err(DiscoveryError::MissingTags);
    }
    if event.content().len() > MAX_CONTENT_BYTES {
        return Err(DiscoveryError::TooLarge);
    }
    event.verify().map_err(|_| DiscoveryError::InvalidEvent)?;

    let author = event.author().to_hex();
    let author_key = XOnlyPublicKey::from_slice(&event.author().to_bytes())
        .map_err(|_| DiscoveryError::InvalidEvent)?;
    let payload = match kind {
        MG_V2_KIND => verify_envelope(event.content(), &author, &author_key)?,
        MG_LEGACY_KIND => verify_proof(
            event.content(),
            "fman_id_pubkey",
            MG_LEGACY_DOMAIN,
            &author,
            &author_key,
        )?,
        _ => verify_proof(
            event.content(),
            "provider_pubkey",
            FLIP_DOMAIN,
            &author,
            &author_key,
        )?,
    };
    let time = |key: &str| {
        let time = payload
            .get(key)
            .and_then(Value::as_u64)
            .ok_or(DiscoveryError::Malformed)?;
        storable_time(time)
    };
    let issued_at = time("issued_at")?;
    let expires_at = time("expires_at")?;
    let created_at = storable_time(event.created_at().as_u64())?;
    if contains_nul(&payload) {
        return Err(DiscoveryError::Unstorable);
    }
    Ok(VerifiedAdvertisement {
        kind,
        author,
        event_id: event.id().to_hex(),
        created_at,
        issued_at,
        expires_at,
        payload,
    })
}

fn storable_time(time: u64) -> Result<u64, DiscoveryError> {
    if time <= MAX_TIMESTAMP {
        Ok(time)
    } else {
        Err(DiscoveryError::Unstorable)
    }
}

/// Whether any key or string in `value` holds U+0000
fn contains_nul(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains('\0'),
        Value::Array(items) => items.iter().any(contains_nul),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key.contains('\0') || contains_nul(value)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

fn digest(domain: &[u8], bytes: &[u8]) -> Message {
    let mut engine = sha256::Hash::engine();
    engine.input(domain);
    engine.input(bytes);
    Message::from_digest(sha256::Hash::from_engine(engine).to_byte_array())
}

/// `{payload, proof: {signature}}`, signed over the JCS form of the payload
/// by the key the payload names in `signer_field`, which must be the author
fn verify_proof(
    content: &str,
    signer_field: &str,
    domain: &[u8],
    author: &str,
    author_key: &XOnlyPublicKey,
) -> Result<Value, DiscoveryError> {
    let document: Value = serde_json::from_str(content).map_err(|_| DiscoveryError::Malformed)?;
    let payload = document
        .get("payload")
        .filter(|p| p.is_object())
        .ok_or(DiscoveryError::Malformed)?;
    if payload.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(DiscoveryError::Malformed);
    }
    if payload.get(signer_field).and_then(Value::as_str) != Some(author) {
        return Err(DiscoveryError::AuthorMismatch);
    }
    let signature = document
        .pointer("/proof/signature")
        .and_then(proof_signature)
        .ok_or(DiscoveryError::InvalidSignature)?;
    let canonical = canonical_json(payload).map_err(|_| DiscoveryError::Malformed)?;
    Secp256k1::verification_only()
        .verify_schnorr(
            &signature,
            &digest(domain, canonical.as_bytes()),
            author_key,
        )
        .map_err(|_| DiscoveryError::InvalidSignature)?;
    Ok(payload.clone())
}

/// MG proofs carry the 64 signature bytes as base64url without padding
/// (PeerBadge's `SchnorrSignatureProof`); FLIP proofs as a JSON array of
/// bytes (serde's default for the FLIP crate's signature type)
fn proof_signature(value: &Value) -> Option<schnorr::Signature> {
    match value {
        Value::String(encoded) => decode_signature(encoded),
        Value::Array(items) if items.len() == 64 => {
            let bytes: Vec<u8> = items
                .iter()
                .map(|byte| byte.as_u64().and_then(|b| u8::try_from(b).ok()))
                .collect::<Option<_>>()?;
            schnorr::Signature::from_slice(&bytes).ok()
        }
        _ => None,
    }
}

fn lower_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    hex::decode(text).ok()?.try_into().ok()
}

/// The v2 signed JSON envelope; the signature covers the payload's exact bytes
fn verify_envelope(
    content: &str,
    author: &str,
    author_key: &XOnlyPublicKey,
) -> Result<Value, DiscoveryError> {
    let envelope: BTreeMap<String, Value> =
        serde_json::from_str(content).map_err(|_| DiscoveryError::Malformed)?;
    if envelope.len() != 3 {
        return Err(DiscoveryError::Malformed);
    }
    let field = |key: &str| {
        envelope
            .get(key)
            .and_then(Value::as_str)
            .ok_or(DiscoveryError::Malformed)
    };
    let signer = field("signer")?;
    lower_hex::<32>(signer).ok_or(DiscoveryError::Malformed)?;
    if signer != author {
        return Err(DiscoveryError::AuthorMismatch);
    }
    let payload = field("payload")?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(DiscoveryError::TooLarge);
    }
    let signature = lower_hex::<64>(field("signature")?)
        .and_then(|bytes| schnorr::Signature::from_slice(&bytes).ok())
        .ok_or(DiscoveryError::Malformed)?;
    Secp256k1::verification_only()
        .verify_schnorr(
            &signature,
            &digest(MG_V2_PURPOSE, payload.as_bytes()),
            author_key,
        )
        .map_err(|_| DiscoveryError::InvalidSignature)?;
    serde_json::from_str::<Value>(payload)
        .ok()
        .filter(Value::is_object)
        .ok_or(DiscoveryError::Malformed)
}

/// Freshness by the FA's rule
pub fn freshness(issued_at: u64, expires_at: u64, now: u64) -> fmo_api_types::AdFreshness {
    use fmo_api_types::AdFreshness;
    if issued_at > now {
        AdFreshness::NotYetValid
    } else if expires_at <= now {
        AdFreshness::Expired
    } else if now - issued_at > MAX_AD_AGE_SECONDS {
        AdFreshness::Stale
    } else {
        AdFreshness::Fresh
    }
}

fn check(id: &str, label: &str, outcome: Outcome) -> CredentialCheck {
    CredentialCheck {
        id: id.into(),
        label: label.into(),
        passed: outcome.passed,
        detail: outcome.detail,
    }
}

/// One check's result: `passed` is None when it did not run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub passed: Option<bool>,
    pub detail: Option<String>,
}

impl Outcome {
    fn new(passed: Option<bool>, detail: Option<String>) -> Self {
        Outcome { passed, detail }
    }
}

/// What the PeerBadge SDK found checking a credential against its issuer's
/// pinned authority
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinnedEvidence {
    /// Whether the SDK admitted the pinned authority
    pub authority_admitted: bool,
    /// The issuance proof, checked when the credential parses and the
    /// authority was admitted
    pub issuance: Outcome,
    /// Whether the SDK's combined check, as Manifold runs it, accepted the
    /// credential and authorization, apart from the authorization time
    pub sdk_accepts: bool,
    /// The verified authorization's time, which the SDK requires not to be
    /// in the future
    pub sdk_authorized_at: Option<u64>,
}

/// Everything about an advertisement's credential that does not change
/// over time: what it names, and the outcome of every cryptographic check.
/// The collector works it out once per credential ([`evaluate_credential`]);
/// requests combine it with the trusted issuers, the revocation state and
/// the time ([`assess_credential`]), so no request verifies a signature.
///
/// It does not depend on which issuers are trusted: a credential whose
/// issuer has a pinned authority is checked against it either way, and
/// [`assess_credential`] ignores that result unless the issuer is trusted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialEvidence {
    /// False when the advertisement carries no holder authorization
    pub presented: bool,
    /// As published
    pub issuer: Option<String>,
    pub holder: Option<String>,
    pub subject: Option<String>,
    pub authorized_at: Option<u64>,
    pub schema: Option<String>,
    pub blind_msg: Option<String>,
    pub trust_level: Option<u64>,
    /// The credential digest's wire form, when the credential parses
    pub digest: Option<String>,
    pub credential_parses: bool,
    pub holder_authorization: Outcome,
    pub digest_matches: Outcome,
    pub schema_check: Outcome,
    /// Present when the issuer has a pinned authority
    pub pinned: Option<PinnedEvidence>,
}

impl CredentialEvidence {
    /// An advertisement without a holder authorization
    pub fn absent() -> Self {
        CredentialEvidence {
            presented: false,
            issuer: None,
            holder: None,
            subject: None,
            authorized_at: None,
            schema: None,
            blind_msg: None,
            trust_level: None,
            digest: None,
            credential_parses: false,
            holder_authorization: Outcome::new(None, None),
            digest_matches: Outcome::new(None, None),
            schema_check: Outcome::new(None, None),
            pinned: None,
        }
    }

    /// The issuer key in lowercase hex, as trust roots are compared
    pub fn issuer_key(&self) -> Option<String> {
        self.issuer.as_deref().map(str::to_lowercase)
    }

    /// Whether every check that does not depend on the trusted issuers, the
    /// revocation state or the time passed for `author`: the credential is
    /// genuine, issued under a pinned authority and authorizes `author`.
    /// The collector keeps such advertisements ahead of anonymous ones.
    pub fn sound_for(&self, author: &str) -> bool {
        let passed = |outcome: &Outcome| outcome.passed == Some(true);
        self.credential_parses
            && self.pinned.as_ref().is_some_and(|pinned| {
                pinned.authority_admitted && passed(&pinned.issuance) && pinned.sdk_accepts
            })
            && passed(&self.holder_authorization)
            && passed(&self.digest_matches)
            && passed(&self.schema_check)
            && self.holder.is_some()
            && self.blind_msg == self.holder
            && self.subject.as_deref() == Some(author)
            && self
                .trust_level
                .is_some_and(|level| level >= REQUIRED_TRUST_LEVEL)
            && self.digest.is_some()
    }
}

fn malformed() -> Option<String> {
    Some("The credential does not parse as a PeerBadge credential".to_owned())
}

/// Works out [`CredentialEvidence`] for the first holder authorization in an
/// advertisement, as the Fedi app's Manifold verifier checks it
/// ([`super::peerbadge`]). This runs the PeerBadge SDK's signature checks:
/// call it off the async executor, once per credential.
pub fn evaluate_credential(payload: &Value) -> CredentialEvidence {
    let envelope = payload
        .get("holder_authorizations")
        .and_then(Value::as_array)
        .and_then(|list| list.first());
    let Some(envelope) = envelope else {
        return CredentialEvidence::absent();
    };
    let text = |pointer: &str| {
        envelope
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let issuer = text("/signed_credential/credential/issuer_id_pubkey");

    // The SDK's own types: what does not parse into them cannot verify
    let signed_credential = envelope
        .get("signed_credential")
        .and_then(|value| serde_json::from_value::<SignedCredential>(value.clone()).ok());
    let authorization = envelope
        .get("holder_authorization")
        .and_then(|value| serde_json::from_value::<HolderAuthorization>(value.clone()).ok());
    let digest = signed_credential
        .as_ref()
        .and_then(|signed| signed.credential.digest().ok())
        .map(CredentialDigest);
    let badge = signed_credential
        .as_ref()
        .map(|signed| parse_trust_score_badge_v1(&signed.credential));
    let trust_level = match &badge {
        Some(Ok(badge)) => Some(badge.trust_level),
        _ => envelope
            .pointer("/signed_credential/credential/info/trust_level")
            .and_then(Value::as_u64),
    };

    let statement = authorization
        .as_ref()
        .map(|authorization| authorization.verify());
    let holder_authorization = match &statement {
        None => Outcome::new(
            Some(false),
            Some("The holder authorization does not parse".to_owned()),
        ),
        Some(Ok(_)) => Outcome::new(Some(true), None),
        Some(Err(error)) => Outcome::new(Some(false), Some(format!("{error}"))),
    };
    let digest_matches = match (&statement, &digest) {
        (Some(Ok(statement)), Some(digest)) => {
            Outcome::new(Some(statement.credential_digest == *digest), None)
        }
        _ => Outcome::new(
            None,
            Some("Needs a verified holder authorization and a parsed credential".to_owned()),
        ),
    };
    let schema_check = match &badge {
        None => Outcome::new(Some(false), malformed()),
        Some(Ok(_)) => Outcome::new(Some(true), None),
        Some(Err(error)) => Outcome::new(Some(false), Some(format!("{error}"))),
    };

    let pinned = issuer
        .as_deref()
        .map(str::to_lowercase)
        .and_then(|issuer| pinned_authorities().get(&issuer))
        .map(|authority| {
            let mut verifier = VerificationContext::new();
            let authority_admitted = verifier.add_issuer_authority(authority).is_ok();
            let issuance = match (&signed_credential, authority_admitted) {
                (None, _) => Outcome::new(Some(false), malformed()),
                (Some(_), false) => Outcome::new(
                    Some(false),
                    Some("The pinned issuer authority does not verify".to_owned()),
                ),
                (Some(signed), true) => match verifier.verify_credential(signed) {
                    Ok(()) => Outcome::new(Some(true), None),
                    Err(error) => Outcome::new(Some(false), Some(format!("{error}"))),
                },
            };
            // The SDK's combined check, exactly as Manifold runs it, at the
            // latest possible time: its only time-dependent step is that the
            // authorization is not dated after the check, which
            // `assess_credential` applies with the time of each request
            let sdk_accepts = authority_admitted
                && match (&signed_credential, &authorization) {
                    (Some(signed), Some(authorization)) => verifier
                        .verify_credential_authorization_at_time(signed, authorization, u64::MAX)
                        .is_ok(),
                    _ => false,
                };
            PinnedEvidence {
                authority_admitted,
                issuance,
                sdk_accepts,
                sdk_authorized_at: match &statement {
                    Some(Ok(statement)) => Some(statement.issued_at.as_secs()),
                    _ => None,
                },
            }
        });

    CredentialEvidence {
        presented: true,
        holder: text("/holder_authorization/authorization/holder_id_pubkey"),
        subject: text("/holder_authorization/authorization/subject_pubkey"),
        authorized_at: envelope
            .pointer("/holder_authorization/authorization/issued_at")
            .and_then(Value::as_u64),
        schema: text("/signed_credential/credential/info/schema"),
        blind_msg: text("/signed_credential/credential/blind_msg"),
        issuer,
        trust_level,
        digest: digest.as_ref().and_then(digest_wire),
        credential_parses: signed_credential.is_some(),
        holder_authorization,
        digest_matches,
        schema_check,
        pinned,
    }
}

/// The credential of an advertisement by `author`, from its stored
/// [`CredentialEvidence`]. Each check is reported; the credential is
/// `Verified` only when every one ran and passed, `Failed` when one the
/// observer ran failed, and `Unavailable` otherwise (an untrusted issuer, an
/// issuer without a pinned authority, or revocation state that is missing or
/// out of date). Runs no cryptography: only the trust, revocation and time
/// checks happen here.
pub fn assess_credential(
    evidence: &CredentialEvidence,
    author: &str,
    now: u64,
    trust: &TrustContext,
) -> TrustCredential {
    if !evidence.presented {
        return TrustCredential {
            state: CredentialState::None,
            issuer_pubkey: None,
            issuer_label: None,
            holder_pubkey: None,
            trust_level: None,
            required_trust_level: REQUIRED_TRUST_LEVEL,
            schema: None,
            checked_at: None,
            checks: Vec::new(),
            reason: Some("The advertisement carries no holder authorization, so there is no credential to check.".into()),
        };
    }

    let issuer_key = evidence.issuer_key();
    let trusted = issuer_key
        .as_ref()
        .is_some_and(|issuer| trust.issuers.contains(issuer));
    let authority = issuer_key
        .as_ref()
        .filter(|_| trusted)
        .and_then(|issuer| pinned_authorities().get(issuer));
    let pinned = evidence.pinned.as_ref().filter(|_| authority.is_some());
    let authority_admitted = pinned.map(|pinned| pinned.authority_admitted);
    let without_authority = || {
        Some(if trusted {
            "Only checked with a pinned issuer authority, which this issuer does not have"
                .to_owned()
        } else {
            "Only checked for a trusted issuer".to_owned()
        })
    };

    let issuance = match (evidence.credential_parses, pinned) {
        (false, _) => Outcome::new(Some(false), malformed()),
        (true, None) => Outcome::new(None, without_authority()),
        (true, Some(pinned)) => pinned.issuance.clone(),
    };
    let revocation = revocation_check(
        issuer_key.as_deref(),
        authority,
        evidence.digest.as_deref(),
        now,
        trust,
    );
    let (revocation, revocation_checked_at) =
        (Outcome::new(revocation.0, revocation.1), revocation.2);
    let trust_level = evidence.trust_level;

    let checks = vec![
        check(
            "issuer_root",
            "Issuer is a trusted PeerBadge issuer",
            Outcome::new(
                (!trust.issuers.is_empty()).then_some(trusted),
                if trust.issuers.is_empty() {
                    Some(
                        "No issuer is trusted: this observer's trusted issuer list is set empty"
                            .to_owned(),
                    )
                } else {
                    (!trusted).then(|| {
                        "Not one of the trusted issuers (by default, Manifold's production issuers)"
                            .to_owned()
                    })
                },
            ),
        ),
        check(
            "issuer_pinned",
            "Issuer authority matches the pinned canonical profile",
            Outcome::new(
                authority_admitted,
                authority_admitted
                    .is_none()
                    .then(without_authority)
                    .flatten(),
            ),
        ),
        check(
            "issuance_proof",
            "Credential issuance proof verifies",
            issuance,
        ),
        check(
            "holder_authorization",
            "Holder authorization proof verifies",
            evidence.holder_authorization.clone(),
        ),
        check(
            "digest",
            "Authorized credential digest matches the credential",
            evidence.digest_matches.clone(),
        ),
        check(
            "blind_msg",
            "Credential is bound to the holder key",
            Outcome::new(
                Some(evidence.holder.is_some() && evidence.blind_msg == evidence.holder),
                None,
            ),
        ),
        check(
            "subject",
            "Authorization names this advertiser as subject",
            Outcome::new(Some(evidence.subject.as_deref() == Some(author)), None),
        ),
        check(
            "not_future",
            "Authorization is not dated in the future",
            Outcome::new(
                Some(evidence.authorized_at.is_some_and(|at| at <= now)),
                None,
            ),
        ),
        check(
            "schema",
            "Credential is a fedi-trust-score-v1.0 badge",
            evidence.schema_check.clone(),
        ),
        check(
            "revocation",
            "No revocation found at every issuer revocation location",
            revocation,
        ),
        check(
            "trust_level",
            &format!("Trust level is at least {REQUIRED_TRUST_LEVEL}"),
            Outcome::new(
                Some(trust_level.is_some_and(|level| level >= REQUIRED_TRUST_LEVEL)),
                None,
            ),
        ),
    ];

    // The SDK's combined check, exactly as Manifold runs it, must agree
    let sdk_accepts = pinned.is_some_and(|pinned| {
        pinned.authority_admitted
            && pinned.sdk_accepts
            && pinned.sdk_authorized_at.is_some_and(|at| at <= now)
    });
    let failed = checks
        .iter()
        .find(|c| c.passed == Some(false) && c.id != "issuer_root");
    let not_run = checks.iter().find(|c| c.passed != Some(true));
    let (state, reason) = if let Some(failed) = failed {
        (
            CredentialState::Failed,
            format!("Check failed: {}.", failed.label.to_lowercase()),
        )
    } else if not_run.is_none() && sdk_accepts {
        (
            CredentialState::Verified,
            "Verified as the Fedi app verifies it: a trusted issuer issued this badge to the key that authorized this advertiser, and had not revoked it at the last revocation check.".to_owned(),
        )
    } else if !trusted {
        (
            CredentialState::Unavailable,
            if trust.issuers.is_empty() {
                "Not verified: this observer trusts no PeerBadge issuer.".to_owned()
            } else {
                "Not verified: the issuer is not a trusted PeerBadge issuer.".to_owned()
            },
        )
    } else {
        let detail = not_run
            .and_then(|check| check.detail.clone())
            .unwrap_or_else(|| "the PeerBadge SDK did not accept the credential".to_owned());
        (
            CredentialState::Unavailable,
            format!(
                "Not verified: {}.",
                detail.trim_end_matches('.').to_lowercase()
            ),
        )
    };

    TrustCredential {
        state,
        issuer_label: evidence.issuer.as_deref().and_then(issuer_label),
        issuer_pubkey: evidence.issuer.clone(),
        holder_pubkey: evidence.holder.clone(),
        trust_level,
        required_trust_level: REQUIRED_TRUST_LEVEL,
        schema: evidence.schema.clone(),
        checked_at: (state == CredentialState::Verified)
            .then_some(revocation_checked_at)
            .flatten(),
        checks,
        reason: Some(reason),
    }
}

/// [`evaluate_credential`] and [`assess_credential`] in one step, as tests
/// check a credential
#[cfg(test)]
pub fn credential(
    payload: &Value,
    author: &str,
    now: u64,
    trust: &TrustContext,
) -> TrustCredential {
    assess_credential(&evaluate_credential(payload), author, now, trust)
}

/// Whether the issuer revoked the credential, by the collector's latest
/// complete read of this issuer's revocation locations. Not run unless that
/// read covered this issuer, at the locations its pinned authority names now,
/// within [`super::peerbadge::REVOCATION_MAX_AGE`]; on success, also returns
/// when that read was.
fn revocation_check(
    issuer: Option<&str>,
    authority: Option<&peerbadge_protocol::IssuerAuthority>,
    digest: Option<&str>,
    now: u64,
    trust: &TrustContext,
) -> (
    Option<bool>,
    Option<String>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    let (Some(issuer), Some(authority), Some(digest)) = (issuer, authority, digest) else {
        return (
            None,
            Some("Needs a parsed credential from an issuer with a pinned authority".to_owned()),
            None,
        );
    };
    let locations = match revocation_locations(authority) {
        Ok(locations) if !locations.is_empty() => locations,
        _ => {
            return (
                None,
                Some("The issuer names no usable Nostr revocation location".to_owned()),
                None,
            )
        }
    };
    let now = chrono::DateTime::from_timestamp(i64::try_from(now).unwrap_or(i64::MAX), 0)
        .unwrap_or(chrono::DateTime::<chrono::Utc>::MAX_UTC);
    let checked_at = match trust.revocations.current_check(issuer, &locations, now) {
        Ok(checked_at) => checked_at,
        Err(gap) => {
            let detail = match gap {
                RevocationGap::NeverRead => {
                    "The issuer's revocations have not been read in full yet".to_owned()
                }
                RevocationGap::OtherLocations => {
                    "The issuer's revocations were read from other locations than its authority names"
                        .to_owned()
                }
                RevocationGap::Stale => format!(
                    "The issuer's revocations were last read in full over {} minutes ago",
                    super::peerbadge::REVOCATION_MAX_AGE.num_minutes()
                ),
            };
            return (None, Some(detail), None);
        }
    };
    match trust
        .revocations
        .entries
        .get(&(issuer.to_owned(), digest.to_owned()))
    {
        None => (Some(true), None, Some(checked_at)),
        Some(RevocationKind::Revoked) => {
            (Some(false), Some("Revoked by its issuer".to_owned()), None)
        }
        Some(RevocationKind::Invalid) => (
            None,
            Some(
                "The issuer published a revocation event for this credential that does not verify"
                    .to_owned(),
            ),
            None,
        ),
    }
}

#[cfg(test)]
mod tests;
