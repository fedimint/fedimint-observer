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
//! trust credentials are evaluated separately ([`credential`]).

use std::collections::BTreeMap;

use bitcoin::hashes::{sha256, Hash, HashEngine};
use bitcoin::secp256k1::{schnorr, Message, Secp256k1, XOnlyPublicKey};
use fmo_api_types::{CredentialCheck, CredentialState, TrustCredential};
use nostr_sdk::Event;
use serde_json::Value;

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
/// A holder authorization may be dated at most this far in the future
const AUTHORIZATION_FUTURE_SKEW: u64 = 60 * 60;

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
        payload
            .get(key)
            .and_then(Value::as_u64)
            .ok_or(DiscoveryError::Malformed)
    };
    Ok(VerifiedAdvertisement {
        kind,
        author,
        event_id: event.id().to_hex(),
        created_at: event.created_at().as_u64(),
        issued_at: time("issued_at")?,
        expires_at: time("expires_at")?,
        payload,
    })
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

fn check(id: &str, label: &str, passed: Option<bool>, detail: Option<String>) -> CredentialCheck {
    CredentialCheck {
        id: id.into(),
        label: label.into(),
        passed,
        detail,
    }
}

/// What the observer can establish about the first holder authorization in
/// an advertisement. It checks the claims it can check itself, and never
/// reports `Verified`: the PeerBadge issuance and authorization proofs and
/// the revocation lookup need Manifold's verifier, so those checks stay
/// unrun, and Manifold fails closed on any check that does not complete.
pub fn credential(
    payload: &Value,
    author: &str,
    now: u64,
    trusted_issuers: &[String],
) -> TrustCredential {
    let envelope = payload
        .get("holder_authorizations")
        .and_then(Value::as_array)
        .and_then(|list| list.first());
    let Some(envelope) = envelope else {
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
    };
    let text = |pointer: &str| {
        envelope
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let holder = text("/holder_authorization/authorization/holder_id_pubkey");
    let subject = text("/holder_authorization/authorization/subject_pubkey");
    let authorized_at = envelope
        .pointer("/holder_authorization/authorization/issued_at")
        .and_then(Value::as_u64);
    let issuer = text("/signed_credential/credential/issuer_id_pubkey");
    let schema = text("/signed_credential/credential/info/schema");
    let trust_level = envelope
        .pointer("/signed_credential/credential/info/trust_level")
        .and_then(Value::as_u64);
    let blind_msg = text("/signed_credential/credential/blind_msg");

    let pinned = issuer
        .as_ref()
        .is_some_and(|issuer| trusted_issuers.contains(issuer));
    let needs_verifier =
        || Some("Needs Manifold's PeerBadge verifier, which the observer does not run".to_owned());
    // With no trust roots configured there is nothing to compare the issuer
    // with, so the check is not run rather than failed
    let checks = vec![
        check(
            "issuer_root",
            "Issuer is one of the configured trust roots",
            (!trusted_issuers.is_empty()).then_some(pinned),
            if trusted_issuers.is_empty() {
                Some("No trust root is configured; Manifold's production environment pins no PeerBadge issuer yet".to_owned())
            } else {
                (!pinned).then(|| "Not one of the configured issuers".to_owned())
            },
        ),
        check(
            "issuer_pinned",
            "Issuer authority matches the pinned canonical profile",
            None,
            needs_verifier(),
        ),
        check(
            "issuance_proof",
            "Credential issuance proof verifies",
            None,
            needs_verifier(),
        ),
        check(
            "holder_authorization",
            "Holder authorization proof verifies",
            None,
            needs_verifier(),
        ),
        check(
            "digest",
            "Authorized credential digest matches the credential",
            None,
            needs_verifier(),
        ),
        check(
            "blind_msg",
            "Credential is bound to the holder key",
            Some(holder.is_some() && blind_msg == holder),
            None,
        ),
        check(
            "subject",
            "Authorization names this advertiser as subject",
            Some(subject.as_deref() == Some(author)),
            None,
        ),
        check(
            "not_future",
            "Authorization is not dated in the future",
            Some(authorized_at.is_some_and(|at| at <= now + AUTHORIZATION_FUTURE_SKEW)),
            None,
        ),
        check(
            "revocation",
            "No revocation found at every issuer revocation location",
            None,
            Some("Revocation lookup not performed".into()),
        ),
        check(
            "trust_level",
            &format!("Trust level is at least {REQUIRED_TRUST_LEVEL}"),
            Some(trust_level.is_some_and(|level| level >= REQUIRED_TRUST_LEVEL)),
            None,
        ),
    ];

    // A claim the observer could check and that is false fails the
    // credential; otherwise it stays unverified, never verified
    let failed = checks
        .iter()
        .find(|c| c.passed == Some(false) && c.id != "issuer_root");
    let (state, reason) = match failed {
        Some(failed) => (
            CredentialState::Failed,
            format!("Check failed: {}.", failed.label.to_lowercase()),
        ),
        None => (
            CredentialState::Unavailable,
            if pinned {
                "The issuer is pinned, but the observer cannot run the PeerBadge proof and revocation checks, so the credential is not verified.".to_owned()
            } else if trusted_issuers.is_empty() {
                "Not verified: no PeerBadge issuer is pinned as a trust root yet, and the observer does not run the PeerBadge proof and revocation checks.".to_owned()
            } else {
                "Not verified: the issuer is not one of the configured trust roots, and the observer does not run the PeerBadge proof and revocation checks.".to_owned()
            },
        ),
    };

    TrustCredential {
        state,
        issuer_pubkey: issuer,
        issuer_label: None,
        holder_pubkey: holder,
        trust_level,
        required_trust_level: REQUIRED_TRUST_LEVEL,
        schema,
        checked_at: None,
        checks,
        reason: Some(reason),
    }
}

#[cfg(test)]
mod tests;
