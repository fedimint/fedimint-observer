//! Verification of the `fedi:fman_seat_bindings` consensus meta field, as
//! Manifold specifies it (`crates/domain/src/mg_seat_bindings.rs`,
//! `mg_peer_attestation.rs` and `federation_config.rs` in fedibtc/manifold).
//!
//! The field binds each guardian seat to the Manifold Guardian (MG) whose key
//! signed for it. Validation fails closed and all or nothing, like
//! Manifold's own readers: a missing, extra, conflicting or unverifiable
//! binding invalidates the whole directory, and nothing from an invalid one is
//! reported as bound.
//!
//! What a verified binding does not establish: that the MG's operator holds a
//! trust credential. Standing comes from PeerBadge credentials, resolved
//! separately and not by this module.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use bitcoin::hashes::{sha256, Hash, HashEngine};
use bitcoin::secp256k1::{schnorr, Message, PublicKey, Secp256k1, XOnlyPublicKey};
use fedimint_core::config::ClientConfig;
use fedimint_core::encoding::Encodable;
use serde_json::{Map, Value};

pub const SEAT_BINDINGS_META_KEY: &str = "fedi:fman_seat_bindings";
const MAX_SEAT_BINDINGS: usize = 64;
const MAX_VALUE_BYTES: usize = 65_536;
const ATTESTATION_DOMAIN: &[u8] = b"fedi-fman-peer-attestation/v1\0";
const ATTESTATION_TYPE: &str = "fedi.fman.peer-attestation";
const CONFIG_HASH_DOMAIN: &[u8] = b"fedi-federation-config-hash/v1\0";
const FEE_ACCOUNT_TYPE: &str = "BtcDepositor";
/// Integers above this lose their decimal form under JCS (RFC 8785 formats
/// numbers as IEEE doubles), so no canonical value carries one
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// What a federation's final config says its seats are
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederationSeats {
    federation_id: String,
    config_hash: [u8; 32],
    /// Guardian identity (compressed broadcast key, hex) by peer id
    seats: BTreeMap<u16, String>,
}

impl FederationSeats {
    pub fn from_config(config: &ClientConfig) -> Result<Self, SeatBindingsError> {
        let broadcast_keys = config
            .global
            .broadcast_public_keys
            .as_ref()
            .ok_or(SeatBindingsError::ConfigWithoutGuardianKeys)?;
        if config.global.api_endpoints.is_empty() {
            return Err(SeatBindingsError::ConfigWithoutGuardianKeys);
        }
        let seats = config
            .global
            .api_endpoints
            .keys()
            .map(|peer| {
                let key = broadcast_keys
                    .get(peer)
                    .ok_or(SeatBindingsError::ConfigWithoutGuardianKeys)?;
                Ok((peer.to_usize() as u16, key.to_string()))
            })
            .collect::<Result<_, SeatBindingsError>>()?;
        Ok(Self {
            federation_id: config.calculate_federation_id().to_string(),
            config_hash: federation_config_hash(config),
            seats,
        })
    }
}

/// SHA-256 over the domain separator and the consensus encoding of the config
/// with its meta cleared, so later meta changes keep the hash
pub fn federation_config_hash(config: &ClientConfig) -> [u8; 32] {
    let mut hashable = config.clone();
    hashable.global.meta = BTreeMap::new();
    let mut engine = sha256::Hash::engine();
    engine.input(CONFIG_HASH_DOMAIN);
    engine.input(&hashable.consensus_encode_to_vec());
    sha256::Hash::from_engine(engine).to_byte_array()
}

/// A binding that verified against the federation's config
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSeatBinding {
    pub peer_id: u16,
    pub fman_pubkey: String,
    pub guardian_identity: String,
    pub guardian_fee_pubkey: String,
    pub issued_at: u64,
}

/// Why a directory value is invalid. The messages are fit to show: they name
/// seats and checks, never key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatBindingsError {
    Empty,
    TooManySeatBindings,
    ValueTooLarge,
    MalformedJson,
    NonCanonical,
    NonCanonicalPeerId,
    DuplicatePeerId(u16),
    InvalidSignature(u16),
    FederationMismatch(u16),
    ConfigHashMismatch(u16),
    UnknownPeerId(u16),
    GuardianIdentityMismatch(u16),
    InvalidGuardianFeeAccount(u16),
    DuplicateGuardianFeeAccount(u16),
    UnboundPeerId(u16),
    /// Not the directory's fault: the observer's copy of the config lacks
    /// the guardian keys bindings are checked against
    ConfigWithoutGuardianKeys,
}

impl fmt::Display for SeatBindingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("The seat-binding directory is empty"),
            Self::TooManySeatBindings => {
                f.write_str("The seat-binding directory has more than 64 bindings")
            }
            Self::ValueTooLarge => f.write_str("The seat-binding directory exceeds 64 KiB"),
            Self::MalformedJson => {
                f.write_str("The seat-binding directory does not have the specified shape")
            }
            Self::NonCanonical => {
                f.write_str("The seat-binding directory is not in canonical form")
            }
            Self::NonCanonicalPeerId => {
                f.write_str("A seat binding has a peer id that is not canonical decimal")
            }
            Self::DuplicatePeerId(peer) => {
                write!(f, "Guardian seat {peer} is bound more than once")
            }
            Self::InvalidSignature(peer) => write!(
                f,
                "The binding for guardian seat {peer} has an invalid signature"
            ),
            Self::FederationMismatch(peer) => write!(
                f,
                "The binding for guardian seat {peer} names another federation"
            ),
            Self::ConfigHashMismatch(peer) => write!(
                f,
                "The binding for guardian seat {peer} names another federation config"
            ),
            Self::UnknownPeerId(peer) => write!(
                f,
                "A binding names peer {peer}, which is not a guardian seat"
            ),
            Self::GuardianIdentityMismatch(peer) => write!(
                f,
                "The binding for guardian seat {peer} names another guardian key"
            ),
            Self::InvalidGuardianFeeAccount(peer) => write!(
                f,
                "The binding for guardian seat {peer} has an invalid guardian-fee account"
            ),
            Self::DuplicateGuardianFeeAccount(peer) => write!(
                f,
                "The binding for guardian seat {peer} repeats another seat's guardian-fee account"
            ),
            Self::UnboundPeerId(peer) => write!(f, "Guardian seat {peer} has no binding"),
            Self::ConfigWithoutGuardianKeys => f.write_str(
                "The federation config has no guardian broadcast keys to check bindings against",
            ),
        }
    }
}

impl std::error::Error for SeatBindingsError {}

/// One binding after the shape checks, before its signature and claims are
fn peer_of(binding: &Value) -> Option<u16> {
    binding
        .pointer("/attestation/peer_id")?
        .as_str()?
        .parse()
        .ok()
}

/// Parses `value` and checks the shape Manifold's `parse_canonical`
/// enforces: size, exact fields, canonical bytes, peer-id order and
/// uniqueness. Returns the bindings in order.
pub fn parse_canonical(value: &str) -> Result<Vec<Value>, SeatBindingsError> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(SeatBindingsError::ValueTooLarge);
    }
    let parsed: Value =
        serde_json::from_str(value).map_err(|_| SeatBindingsError::MalformedJson)?;
    let top = exact_object(&parsed, &["seat_bindings", "version"])?;
    expect_version(&top["version"])?;
    let bindings = top["seat_bindings"]
        .as_array()
        .ok_or(SeatBindingsError::MalformedJson)?;
    if bindings.is_empty() {
        return Err(SeatBindingsError::Empty);
    }
    if bindings.len() > MAX_SEAT_BINDINGS {
        return Err(SeatBindingsError::TooManySeatBindings);
    }

    let mut peers = Vec::with_capacity(bindings.len());
    for binding in bindings {
        check_binding_shape(binding)?;
        let spelled = binding["attestation"]["peer_id"]
            .as_str()
            .ok_or(SeatBindingsError::MalformedJson)?;
        let peer: u16 = spelled
            .parse()
            .map_err(|_| SeatBindingsError::NonCanonicalPeerId)?;
        if peer.to_string() != spelled {
            return Err(SeatBindingsError::NonCanonicalPeerId);
        }
        peers.push(peer);
    }
    let mut sorted = peers.clone();
    sorted.sort_unstable();
    if let Some(pair) = sorted.windows(2).find(|pair| pair[0] == pair[1]) {
        return Err(SeatBindingsError::DuplicatePeerId(pair[0]));
    }
    // Manifold sorts by numeric peer id before canonicalizing, so any other
    // order is not the canonical value
    if sorted != peers || canonical_json(&parsed)? != value {
        return Err(SeatBindingsError::NonCanonical);
    }
    Ok(bindings.clone())
}

/// Checks every binding against the federation, in Manifold's order of
/// checks, and that every seat is bound
pub fn verify_for_federation(
    value: &str,
    seats: &FederationSeats,
) -> Result<Vec<VerifiedSeatBinding>, SeatBindingsError> {
    let bindings = parse_canonical(value)?;
    let secp = Secp256k1::verification_only();
    let mut fee_accounts = BTreeSet::new();
    let mut verified = Vec::with_capacity(bindings.len());

    for binding in &bindings {
        let peer = peer_of(binding).ok_or(SeatBindingsError::MalformedJson)?;
        let attestation = &binding["attestation"];
        let text = |key: &str| attestation[key].as_str().unwrap_or_default();

        // The signature, under the key the binding names
        let fman_pubkey = text("fman_pubkey");
        let public_key =
            canonical_xonly_key(fman_pubkey).ok_or(SeatBindingsError::InvalidSignature(peer))?;
        let signature = binding["proof"]["signature"]
            .as_str()
            .and_then(decode_signature)
            .ok_or(SeatBindingsError::InvalidSignature(peer))?;
        let digest = attestation_digest(attestation)?;
        secp.verify_schnorr(&signature, &Message::from_digest(digest), &public_key)
            .map_err(|_| SeatBindingsError::InvalidSignature(peer))?;

        // Its claims, against the config
        if text("federation_id") != seats.federation_id {
            return Err(SeatBindingsError::FederationMismatch(peer));
        }
        if hash_bytes(&attestation["federation_config_hash"])? != seats.config_hash {
            return Err(SeatBindingsError::ConfigHashMismatch(peer));
        }
        let Some(seat_identity) = seats.seats.get(&peer) else {
            return Err(SeatBindingsError::UnknownPeerId(peer));
        };
        if text("guardian_identity") != seat_identity {
            return Err(SeatBindingsError::GuardianIdentityMismatch(peer));
        }
        let fee_pubkey = single_btc_depositor(&attestation["guardian_fee_account"])
            .ok_or(SeatBindingsError::InvalidGuardianFeeAccount(peer))?;
        // A single-signature account is identified by its one key
        if !fee_accounts.insert(fee_pubkey.clone()) {
            return Err(SeatBindingsError::DuplicateGuardianFeeAccount(peer));
        }

        verified.push(VerifiedSeatBinding {
            peer_id: peer,
            fman_pubkey: fman_pubkey.to_owned(),
            guardian_identity: seat_identity.clone(),
            guardian_fee_pubkey: fee_pubkey,
            issued_at: attestation["issued_at"]
                .as_u64()
                .ok_or(SeatBindingsError::MalformedJson)?,
        });
    }

    if let Some(unbound) = seats
        .seats
        .keys()
        .find(|peer| !verified.iter().any(|binding| binding.peer_id == **peer))
    {
        return Err(SeatBindingsError::UnboundPeerId(*unbound));
    }
    Ok(verified)
}

/// SHA-256 over the domain separator and the JCS form of the type-tagged
/// statement, the message the MG's Schnorr signature covers
fn attestation_digest(attestation: &Value) -> Result<[u8; 32], SeatBindingsError> {
    let payload = serde_json::json!({
        "type": ATTESTATION_TYPE,
        "version": 1,
        "attestation": attestation,
    });
    let mut engine = sha256::Hash::engine();
    engine.input(ATTESTATION_DOMAIN);
    engine.input(canonical_json(&payload)?.as_bytes());
    Ok(sha256::Hash::from_engine(engine).to_byte_array())
}

fn check_binding_shape(binding: &Value) -> Result<(), SeatBindingsError> {
    let object = exact_object(binding, &["attestation", "proof", "version"])?;
    expect_version(&object["version"])?;
    let proof = exact_object(&object["proof"], &["signature"])?;
    proof["signature"]
        .as_str()
        .ok_or(SeatBindingsError::MalformedJson)?;
    let attestation = exact_object(
        &object["attestation"],
        &[
            "federation_config_hash",
            "federation_id",
            "fman_pubkey",
            "guardian_fee_account",
            "guardian_identity",
            "issued_at",
            "peer_id",
        ],
    )?;
    for key in [
        "federation_id",
        "fman_pubkey",
        "guardian_identity",
        "peer_id",
    ] {
        attestation[key]
            .as_str()
            .ok_or(SeatBindingsError::MalformedJson)?;
    }
    attestation["issued_at"]
        .as_u64()
        .ok_or(SeatBindingsError::MalformedJson)?;
    // Any length decodes; a wrong one fails as a config hash mismatch
    attestation["federation_config_hash"]
        .as_array()
        .ok_or(SeatBindingsError::MalformedJson)?
        .iter()
        .try_for_each(|byte| match byte.as_u64() {
            Some(byte) if byte <= 255 => Ok(()),
            _ => Err(SeatBindingsError::MalformedJson),
        })?;
    let account = exact_object(
        &attestation["guardian_fee_account"],
        &["acc_type", "pub_keys", "threshold"],
    )?;
    account["acc_type"]
        .as_str()
        .ok_or(SeatBindingsError::MalformedJson)?;
    account["threshold"]
        .as_u64()
        .ok_or(SeatBindingsError::MalformedJson)?;
    account["pub_keys"]
        .as_array()
        .ok_or(SeatBindingsError::MalformedJson)?
        .iter()
        .try_for_each(|key| {
            key.as_str()
                .map(drop)
                .ok_or(SeatBindingsError::MalformedJson)
        })
}

/// An object with exactly these keys
fn exact_object<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a Map<String, Value>, SeatBindingsError> {
    let object = value.as_object().ok_or(SeatBindingsError::MalformedJson)?;
    if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
        return Err(SeatBindingsError::MalformedJson);
    }
    Ok(object)
}

fn expect_version(value: &Value) -> Result<(), SeatBindingsError> {
    (value.as_u64() == Some(1))
        .then_some(())
        .ok_or(SeatBindingsError::MalformedJson)
}

fn hash_bytes(value: &Value) -> Result<[u8; 32], SeatBindingsError> {
    let bytes: Vec<u8> = value
        .as_array()
        .ok_or(SeatBindingsError::MalformedJson)?
        .iter()
        .map(|byte| byte.as_u64().and_then(|byte| u8::try_from(byte).ok()))
        .collect::<Option<_>>()
        .ok_or(SeatBindingsError::MalformedJson)?;
    // A hash of another length can't be this config's
    Ok(bytes.try_into().unwrap_or([0; 32]))
}

/// The lowercase x-only hex Manifold requires; npub and uppercase are not it
fn canonical_xonly_key(hex_key: &str) -> Option<XOnlyPublicKey> {
    let is_lower_hex = hex_key.len() == 64
        && hex_key
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    is_lower_hex
        .then(|| hex::decode(hex_key).ok())
        .flatten()
        .and_then(|bytes| XOnlyPublicKey::from_slice(&bytes).ok())
}

/// base64url without padding, as Manifold's proofs encode the 64 bytes
pub(crate) fn decode_signature(encoded: &str) -> Option<schnorr::Signature> {
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    // Reject alternative spellings of the same bytes
    if URL_SAFE_NO_PAD.encode(&bytes) != encoded {
        return None;
    }
    schnorr::Signature::from_slice(&bytes).ok()
}

/// The key of a single-signature BTC depositor account, in canonical hex
fn single_btc_depositor(account: &Value) -> Option<String> {
    if account["acc_type"].as_str()? != FEE_ACCOUNT_TYPE || account["threshold"].as_u64()? != 1 {
        return None;
    }
    let [key] = account["pub_keys"].as_array()?.as_slice() else {
        return None;
    };
    let key = key.as_str()?;
    let parsed = PublicKey::from_slice(&hex::decode(key).ok()?).ok()?;
    (parsed.to_string() == key).then(|| key.to_owned())
}

/// RFC 8785 (JCS) serialization of the values a directory can hold: objects
/// with keys in UTF-16 order, strings escaped as ECMAScript does (which
/// `serde_json` matches), and integers. Fractions, negatives and integers
/// past 2^53 have no place in a directory and are rejected.
pub fn canonical_json(value: &Value) -> Result<String, SeatBindingsError> {
    let mut out = String::new();
    write_canonical(value, &mut out)?;
    Ok(out)
}

fn write_canonical(value: &Value, out: &mut String) -> Result<(), SeatBindingsError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => {
            let integer = number
                .as_u64()
                .filter(|integer| *integer <= MAX_SAFE_INTEGER)
                .ok_or(SeatBindingsError::NonCanonical)?;
            out.push_str(&integer.to_string());
        }
        Value::String(text) => {
            out.push_str(&serde_json::to_string(text).map_err(|_| SeatBindingsError::NonCanonical)?)
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out)?;
            }
            out.push(']');
        }
        Value::Object(object) => {
            let mut entries: Vec<_> = object.iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (index, (key, item)) in entries.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(
                    &serde_json::to_string(key).map_err(|_| SeatBindingsError::NonCanonical)?,
                );
                out.push(':');
                write_canonical(item, out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
