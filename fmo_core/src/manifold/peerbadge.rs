//! PeerBadge trust credentials, verified the way the Fedi app's Manifold
//! verifier does (`crates/peer-badge-verifier` in fedibtc/manifold), with
//! Fedi's own PeerBadge SDK:
//!
//! 1. The issuer is a trusted root, with the identity-signed authority pinned
//!    from Manifold's production profile. Authorities are never fetched from
//!    relays, so a relay cannot rotate an issuer's keys.
//! 2. The SDK verifies the credential's issuance proof (a PBRSA blind
//!    signature), the holder authorization, the holder binding and the
//!    credential digest.
//! 3. The badge is a `fedi-trust-score-v1.0` badge at the required trust level,
//!    authorizing this advertiser.
//! 4. The issuer has published no revocation of it at its revocation locations,
//!    by the collector's latest complete lookup, which must be recent.
//!
//! Whatever cannot complete leaves a credential unverified, never verified.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use anyhow::{ensure, Context};
use chrono::{DateTime, TimeDelta, Utc};
use peerbadge_protocol::{CredentialDigest, IssuerAuthority, SignedRevocation};

use super::collector::MANIFOLD_PRODUCTION_ISSUERS;

/// Manifold's credential revocation kind (`CREDENTIAL_REVOCATION_EVENT_KIND`)
pub const REVOCATION_KIND: u16 = 37704;
/// A revocation's `d` tag is this prefix, a colon and the credential digest
const REVOCATION_D_TAG_PREFIX: &str = "credential-revocation";
const NOSTR_REVOCATION_PROTOCOL: &str = "nostr";
/// Manifold's `MAX_REVOCATION_LOCATIONS`
const MAX_REVOCATION_LOCATIONS: usize = 4;
/// Revocation state older than this no longer verifies anything: Manifold
/// reads it afresh for every check, and the collector reads it every sync
pub const REVOCATION_MAX_AGE: TimeDelta = TimeDelta::minutes(15);

/// The identity-signed authorities of [`MANIFOLD_PRODUCTION_ISSUERS`], in the
/// same order, copied with them from `crates/manifold-environment/fixtures`
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

/// Checked at startup, so a bad committed authority stops the observer there
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

/// The relays to read revocations from, each with the trusted issuers whose
/// pinned authority names it. Trusted issuers without a pinned authority have
/// none: their credentials cannot verify anyway.
pub fn revocation_targets(trusted: &[String]) -> anyhow::Result<BTreeMap<String, Vec<String>>> {
    let mut targets: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for issuer in trusted {
        let Some(authority) = pinned_authorities().get(issuer) else {
            continue;
        };
        for location in &authority.issuer.revocation {
            if location.protocol == NOSTR_REVOCATION_PROTOCOL {
                let relay = nostr_sdk::Url::parse(&location.location)
                    .with_context(|| format!("revocation location {:?}", location.location))?;
                targets
                    .entry(relay.to_string())
                    .or_default()
                    .push(issuer.clone());
            }
        }
    }
    Ok(targets)
}

/// What an issuer published about one credential
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevocationKind {
    /// A revocation whose issuer signature verifies
    Revoked,
    /// Only revocation events that do not verify: Manifold then fails the
    /// check closed rather than call the credential unrevoked
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
    /// Lowercase hex
    pub issuer: String,
    /// The digest's wire form (unpadded base64url), as in the `d` tag
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

/// What a relay event says about a credential, admitted the way Manifold's
/// verifier does: a correctly signed kind-37704 event by a trusted issuer, for
/// the credential its `d` tag names. A revocation whose issuer proof fails or
/// names another credential is recorded as invalid, not ignored.
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

/// The revocations the collector has read, as of its latest complete lookup
#[derive(Debug, Clone, Default)]
pub struct RevocationState {
    /// When every revocation location last answered completely; None before
    pub checked_at: Option<DateTime<Utc>>,
    /// By (issuer, credential digest)
    pub entries: HashMap<(String, String), RevocationKind>,
}

impl RevocationState {
    pub fn is_current(&self, now: DateTime<Utc>) -> bool {
        self.checked_at
            .is_some_and(|checked| now - checked <= REVOCATION_MAX_AGE)
    }
}

/// What credentials are checked against
#[derive(Debug, Clone, Default)]
pub struct TrustContext {
    /// Trusted issuers, lowercase hex
    pub issuers: Vec<String>,
    pub revocations: RevocationState,
}

#[cfg(test)]
mod tests;
