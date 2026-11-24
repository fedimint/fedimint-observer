//! Verifies PeerBadge credentials against pinned issuers and revocations.
//! Missing or stale evidence fails closed.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use anyhow::{ensure, Context};
use chrono::{DateTime, TimeDelta, Utc};
use peerbadge_protocol::{CredentialDigest, IssuerAuthority, SignedRevocation};

use super::collector::MANIFOLD_PRODUCTION_ISSUERS;

pub const REVOCATION_KIND: u16 = 37704;
const REVOCATION_D_TAG_PREFIX: &str = "credential-revocation";
const NOSTR_REVOCATION_PROTOCOL: &str = "nostr";
const MAX_REVOCATION_LOCATIONS: usize = 4;
pub const REVOCATION_MAX_AGE: TimeDelta = TimeDelta::minutes(15);

const AUTHORITY_DOCUMENTS: &[&str] = &[
    include_str!("fixtures/issuer-authority-ac9647df.json"),
    include_str!("fixtures/issuer-authority-db37f7c6.json"),
    include_str!("fixtures/issuer-authority-8cdf45bc.json"),
    include_str!("fixtures/issuer-authority-f6c7bc12.json"),
    include_str!("fixtures/issuer-authority-96eb3609.json"),
    include_str!("fixtures/issuer-authority-ef245cb4.json"),
    include_str!("fixtures/issuer-authority-c3cfb837.json"),
    include_str!("fixtures/issuer-authority-5546e9ca.json"),
];

/// Parses and verifies authority documents, keyed by issuer (lowercase hex)
fn load_authorities(documents: &[&str]) -> anyhow::Result<BTreeMap<String, IssuerAuthority>> {
    let mut authorities = BTreeMap::new();
    for document in documents {
        let authority: IssuerAuthority =
            serde_json::from_str(document).context("parse an issuer authority")?;
        authority
            .verify()
            .map_err(|error| anyhow::anyhow!("issuer authority does not verify: {error}"))?;
        ensure!(
            authority.issuer.revocation.len() <= MAX_REVOCATION_LOCATIONS,
            "issuer authority lists more than {MAX_REVOCATION_LOCATIONS} revocation locations"
        );
        let issuer = authority.issuer.issuer_id_pubkey.0.to_hex();
        ensure!(
            authorities.insert(issuer.clone(), authority).is_none(),
            "duplicate issuer authority for {issuer}"
        );
    }
    Ok(authorities)
}

/// Manifold's production issuer authorities, verified on first use
pub fn pinned_authorities() -> &'static BTreeMap<String, IssuerAuthority> {
    static PINNED: OnceLock<BTreeMap<String, IssuerAuthority>> = OnceLock::new();
    PINNED.get_or_init(|| {
        load_authorities(AUTHORITY_DOCUMENTS).expect("the committed issuer authorities verify")
    })
}

pub fn pinned_authorities_fingerprint() -> &'static str {
    static FINGERPRINT: OnceLock<String> = OnceLock::new();
    FINGERPRINT.get_or_init(|| {
        use bitcoin::hashes::{sha256, Hash, HashEngine};
        let mut engine = sha256::Hash::engine();
        for document in AUTHORITY_DOCUMENTS {
            engine.input(&(document.len() as u64).to_be_bytes());
            engine.input(document.as_bytes());
        }
        hex::encode(&sha256::Hash::from_engine(engine).to_byte_array()[..16])
    })
}

/// Checked at startup.
pub fn check_pinned_authorities() -> anyhow::Result<()> {
    let authorities = load_authorities(AUTHORITY_DOCUMENTS)?;
    ensure!(
        authorities.len() == MANIFOLD_PRODUCTION_ISSUERS.len()
            && MANIFOLD_PRODUCTION_ISSUERS
                .iter()
                .all(|issuer| authorities.contains_key(*issuer)),
        "the committed issuer authorities do not match MANIFOLD_PRODUCTION_ISSUERS"
    );
    Ok(())
}

/// "Manifold production issuer 3 of 8" for a roster issuer
pub fn issuer_label(issuer: &str) -> Option<String> {
    let issuer = issuer.to_lowercase();
    MANIFOLD_PRODUCTION_ISSUERS
        .iter()
        .position(|key| *key == issuer)
        .map(|index| {
            format!(
                "Manifold production issuer {} of {}",
                index + 1,
                MANIFOLD_PRODUCTION_ISSUERS.len()
            )
        })
}

/// The Nostr revocation locations an authority names.
pub fn revocation_locations(authority: &IssuerAuthority) -> anyhow::Result<Vec<String>> {
    let mut locations = authority
        .issuer
        .revocation
        .iter()
        .filter(|location| location.protocol == NOSTR_REVOCATION_PROTOCOL)
        .map(|location| {
            nostr_sdk::Url::parse(&location.location)
                .map(|relay| relay.to_string())
                .with_context(|| format!("revocation location {:?}", location.location))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    locations.sort();
    locations.dedup();
    Ok(locations)
}

pub fn revocation_targets(trusted: &[String]) -> anyhow::Result<BTreeMap<String, Vec<String>>> {
    let mut targets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for issuer in trusted {
        let Some(authority) = pinned_authorities().get(issuer) else {
            continue;
        };
        for relay in revocation_locations(authority)? {
            targets.entry(relay).or_default().push(issuer.clone());
        }
    }
    Ok(targets)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationKind {
    Revoked,
    Invalid,
}

impl RevocationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RevocationKind::Revoked => "revoked",
            RevocationKind::Invalid => "invalid",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "revoked" => Some(RevocationKind::Revoked),
            "invalid" => Some(RevocationKind::Invalid),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevocationRecord {
    pub issuer: String,
    pub credential_digest: String,
    pub kind: RevocationKind,
    pub event_id: String,
}

/// The digest's wire form, as revocations and their `d` tags carry it
pub fn digest_wire(digest: &CredentialDigest) -> Option<String> {
    match serde_json::to_value(digest).ok()? {
        serde_json::Value::String(wire) => Some(wire),
        _ => None,
    }
}

/// The one `d` tag's value, as Manifold requires: exactly one, of two parts
fn exact_d_tag(event: &nostr_sdk::Event) -> Option<String> {
    let mut d_tags = event
        .tags()
        .iter()
        .map(|tag| tag.as_vec())
        .filter(|parts| parts.first().map(String::as_str) == Some("d"));
    let tag = d_tags.next()?;
    (tag.len() == 2 && d_tags.next().is_none()).then(|| tag[1].clone())
}

pub fn admit_revocation(event: &nostr_sdk::Event, trusted: &[String]) -> Option<RevocationRecord> {
    let issuer = event.author().to_hex();
    if event.kind().as_u16() != REVOCATION_KIND
        || !trusted.contains(&issuer)
        || event.verify().is_err()
    {
        return None;
    }
    let digest = exact_d_tag(event)?
        .strip_prefix(REVOCATION_D_TAG_PREFIX)?
        .strip_prefix(':')?
        .to_owned();
    if digest.is_empty() || digest.len() > 64 {
        return None;
    }
    let revokes = serde_json::from_str::<SignedRevocation>(event.content())
        .ok()
        .filter(|revocation| revocation.proof.issuer_id_pubkey.0.to_hex() == issuer)
        .and_then(|revocation| revocation.verify().ok())
        .and_then(|revocation| digest_wire(&revocation.credential_digest))
        .is_some_and(|revoked| revoked == digest);
    Some(RevocationRecord {
        issuer,
        credential_digest: digest,
        kind: if revokes {
            RevocationKind::Revoked
        } else {
            RevocationKind::Invalid
        },
        event_id: event.id().to_hex(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuerRevocationCheck {
    pub checked_at: DateTime<Utc>,
    pub locations: Vec<String>,
}

/// Why an issuer's revocation state cannot be relied on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationGap {
    NeverRead,
    OtherLocations,
    Stale,
}

#[derive(Debug, Clone, Default)]
pub struct RevocationState {
    pub checks: HashMap<String, IssuerRevocationCheck>,
    pub entries: HashMap<(String, String), RevocationKind>,
}

impl RevocationState {
    pub fn current_check(
        &self,
        issuer: &str,
        locations: &[String],
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, RevocationGap> {
        let check = self.checks.get(issuer).ok_or(RevocationGap::NeverRead)?;
        if check.locations != locations {
            return Err(RevocationGap::OtherLocations);
        }
        if now - check.checked_at > REVOCATION_MAX_AGE {
            return Err(RevocationGap::Stale);
        }
        Ok(check.checked_at)
    }
}

#[derive(Debug, Clone, Default)]
pub struct TrustContext {
    pub issuers: Vec<String>,
    pub revocations: RevocationState,
}

#[cfg(test)]
mod tests;
