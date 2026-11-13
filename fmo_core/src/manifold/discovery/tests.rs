use fmo_api_types::{AdFreshness, TrustCredential};
use nostr_sdk::{EventBuilder, JsonUtil, Keys, Kind, Tag};
use serde_json::json;

use super::*;
use crate::manifold::peerbadge::{revocation_locations, IssuerRevocationCheck, RevocationState};

/// Real kind 37701, 37709 and 37702 events captured from Manifold's
/// production relays, signed by their publishers
const RELAY_EVENTS: &str = include_str!("../fixtures/relay-events.json");

fn real_events() -> Vec<Event> {
    let fixture: Value = serde_json::from_str(RELAY_EVENTS).unwrap();
    fixture["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| Event::from_json(event.to_string()).unwrap())
        .collect()
}

fn tags(kind: u16) -> Vec<Tag> {
    let (d, t) = kind_tags(kind).unwrap();
    vec![Tag::identifier(d), Tag::hashtag(t)]
}

fn signed(keys: &Keys, kind: u16, content: String, tags: Vec<Tag>) -> Event {
    EventBuilder::new(Kind::Custom(kind), content, tags)
        .to_event(keys)
        .unwrap()
}

/// A v2 envelope over `payload`, signed by `signer`
fn envelope(signer: &Keys, payload: &str) -> String {
    let secp = Secp256k1::new();
    let keypair = bitcoin::secp256k1::Keypair::from_seckey_slice(
        &secp,
        &signer.secret_key().unwrap().secret_bytes(),
    )
    .unwrap();
    let signature =
        secp.sign_schnorr_no_aux_rand(&digest(MG_V2_PURPOSE, payload.as_bytes()), &keypair);
    json!({
        "signer": signer.public_key().to_hex(),
        "payload": payload,
        "signature": hex::encode(signature.as_ref()),
    })
    .to_string()
}

#[test]
fn real_manifold_events_verify() {
    let events = real_events();
    let kinds: Vec<u16> = events.iter().map(|e| e.kind().as_u16()).collect();
    assert_eq!(kinds, vec![MG_LEGACY_KIND, MG_V2_KIND, FLIP_KIND]);
    for event in &events {
        let ad = verify_event(event)
            .unwrap_or_else(|error| panic!("kind {}: {error:?}", event.kind().as_u16()));
        assert_eq!(ad.author, event.author().to_hex());
        assert!(ad.expires_at >= ad.issued_at);
        assert!(ad.payload.is_object());
    }
    // The same guardian published both MG formats from one snapshot
    let legacy = verify_event(&events[0]).unwrap();
    let v2 = verify_event(&events[1]).unwrap();
    assert_eq!(legacy.author, v2.author);
    assert_eq!(
        legacy.payload["service_pubkey"],
        v2.payload["service_pubkey"]
    );
    assert!(v2.payload.pointer("/version/mg_version").is_some());
}

#[test]
fn a_changed_payload_no_longer_verifies() {
    // Re-signing the outer event keeps it valid, so only the inner proof can catch
    // this
    let keys = Keys::generate();
    let legacy: Value = serde_json::from_str(real_events()[0].content()).unwrap();
    let mut changed = legacy.clone();
    changed["payload"]["fman_id_pubkey"] = json!(keys.public_key().to_hex());
    changed["payload"]["plans"][0]["InfiniteBestEffort"]["price_msats"] = json!(1);
    assert_eq!(
        verify_event(&signed(
            &keys,
            MG_LEGACY_KIND,
            changed.to_string(),
            tags(MG_LEGACY_KIND)
        )),
        Err(DiscoveryError::InvalidSignature)
    );
    // Republishing someone else's signed document under another author
    assert_eq!(
        verify_event(&signed(
            &keys,
            MG_LEGACY_KIND,
            legacy.to_string(),
            tags(MG_LEGACY_KIND)
        )),
        Err(DiscoveryError::AuthorMismatch)
    );
}

#[test]
fn v2_envelopes_bind_the_exact_payload_bytes_to_the_author() {
    let keys = Keys::generate();
    let payload = r#"{"service_pubkey":"aa","issued_at":10,"expires_at":20,"api_endpoints":[],"availability":{"fedimintd_version":"0.12.0+fedi","federation_sizes":[4]},"plans":[],"holder_authorizations":[]}"#;
    let ad = verify_event(&signed(
        &keys,
        MG_V2_KIND,
        envelope(&keys, payload),
        tags(MG_V2_KIND),
    ))
    .unwrap();
    assert_eq!((ad.issued_at, ad.expires_at), (10, 20));

    // Whitespace changes the signed bytes
    let mut reformatted: Value = serde_json::from_str(&envelope(&keys, payload)).unwrap();
    reformatted["payload"] = json!(payload.replace(",\"", ", \""));
    assert_eq!(
        verify_event(&signed(
            &keys,
            MG_V2_KIND,
            reformatted.to_string(),
            tags(MG_V2_KIND)
        )),
        Err(DiscoveryError::InvalidSignature)
    );

    // Signed by one key, published by another
    let other = Keys::generate();
    assert_eq!(
        verify_event(&signed(
            &other,
            MG_V2_KIND,
            envelope(&keys, payload),
            tags(MG_V2_KIND)
        )),
        Err(DiscoveryError::AuthorMismatch)
    );

    // An extra envelope field is not the specified shape
    let mut extra: Value = serde_json::from_str(&envelope(&keys, payload)).unwrap();
    extra["note"] = json!("x");
    assert_eq!(
        verify_event(&signed(
            &keys,
            MG_V2_KIND,
            extra.to_string(),
            tags(MG_V2_KIND)
        )),
        Err(DiscoveryError::Malformed)
    );
}

/// A correctly signed v2 advertisement whose payload is `payload`
fn signed_v2(keys: &Keys, payload: &str) -> Event {
    signed(keys, MG_V2_KIND, envelope(keys, payload), tags(MG_V2_KIND))
}

#[test]
fn signed_but_unstorable_advertisements_are_rejected() {
    let keys = Keys::generate();
    let with = |issued_at: &str, name: &str| {
        format!(
            r#"{{"service_pubkey":"aa","issued_at":{issued_at},"expires_at":20,"display_name":"{name}","plans":[]}}"#
        )
    };

    // One past i64::MAX: cast to BIGINT it became i64::MIN, and the presence
    // query's `now - issued_at` overflowed and rolled back the whole sync
    assert_eq!(
        verify_event(&signed_v2(&keys, &with("9223372036854775808", "a"))),
        Err(DiscoveryError::Unstorable)
    );
    assert_eq!(
        verify_event(&signed_v2(
            &keys,
            &with(&(MAX_TIMESTAMP + 1).to_string(), "a")
        )),
        Err(DiscoveryError::Unstorable)
    );
    // The last storable second is still a valid advertisement
    let ad = verify_event(&signed_v2(&keys, &with(&MAX_TIMESTAMP.to_string(), "a"))).unwrap();
    assert_eq!(ad.issued_at, MAX_TIMESTAMP);

    // JSONB cannot hold U+0000, in a value or in a key
    assert_eq!(
        verify_event(&signed_v2(&keys, &with("10", "a\\u0000b"))),
        Err(DiscoveryError::Unstorable)
    );
    let nul_key =
        r#"{"service_pubkey":"aa","issued_at":10,"expires_at":20,"plans":[{"k\u0000":1}]}"#;
    assert_eq!(
        verify_event(&signed_v2(&keys, nul_key)),
        Err(DiscoveryError::Unstorable)
    );
    // An escaped backslash before "u0000" is ordinary text, not a NUL
    assert!(verify_event(&signed_v2(&keys, &with("10", "a\\\\u0000b"))).is_ok());
}

#[test]
fn only_manifold_tagged_events_are_advertisements() {
    let keys = Keys::generate();
    // Other applications publish credentials and probes under kind 37702
    let probe = signed(
        &keys,
        FLIP_KIND,
        "{}".into(),
        vec![Tag::identifier("probe:1")],
    );
    assert_eq!(verify_event(&probe), Err(DiscoveryError::MissingTags));
    let wrong_hashtag = signed(
        &keys,
        MG_V2_KIND,
        "{}".into(),
        vec![Tag::identifier("mg-ad"), Tag::hashtag("other")],
    );
    assert_eq!(
        verify_event(&wrong_hashtag),
        Err(DiscoveryError::MissingTags)
    );
    let other_kind = signed(&keys, 1, "hi".into(), vec![]);
    assert_eq!(verify_event(&other_kind), Err(DiscoveryError::UnknownKind));
}

#[test]
fn freshness_follows_the_fa_rule() {
    let now = 1_800_000_000;
    assert_eq!(freshness(now - 60, now + 3600, now), AdFreshness::Fresh);
    assert_eq!(freshness(now - 7200, now + 1, now), AdFreshness::Fresh);
    assert_eq!(freshness(now - 7201, now + 1, now), AdFreshness::Stale);
    assert_eq!(freshness(now - 10, now, now), AdFreshness::Expired);
    // A withdrawal republishes with expires_at == issued_at
    assert_eq!(freshness(now - 10, now - 10, now), AdFreshness::Expired);
    assert_eq!(
        freshness(now + 5, now + 3600, now),
        AdFreshness::NotYetValid
    );
}

fn default_issuers() -> Vec<String> {
    crate::manifold::collector::parse_trusted_issuers(None).unwrap()
}

/// Trust as of `now`: `issuers`, and revocations read completely just now
fn trust_at(now: u64, issuers: Vec<String>) -> TrustContext {
    let checked_at = chrono::DateTime::from_timestamp(now as i64, 0).unwrap();
    let checks = issuers
        .iter()
        .filter_map(|issuer| {
            let authority = crate::manifold::peerbadge::pinned_authorities().get(issuer)?;
            Some((
                issuer.clone(),
                IssuerRevocationCheck {
                    checked_at,
                    locations: revocation_locations(authority).unwrap(),
                },
            ))
        })
        .collect();
    TrustContext {
        issuers,
        revocations: RevocationState {
            checks,
            entries: Default::default(),
        },
    }
}

fn credential_issuer(ad: &VerifiedAdvertisement) -> String {
    ad.payload
        .pointer("/holder_authorizations/0/signed_credential/credential/issuer_id_pubkey")
        .and_then(Value::as_str)
        .unwrap()
        .to_lowercase()
}

fn result(credential: &TrustCredential, id: &str) -> Option<bool> {
    credential
        .checks
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("no {id} check"))
        .passed
}

/// The real advertisements with a credential, and the credential's digest
fn real_credentials() -> Vec<(VerifiedAdvertisement, String)> {
    real_events()
        .iter()
        .map(|event| verify_event(event).unwrap())
        .filter_map(|ad| {
            let signed: SignedCredential = serde_json::from_value(
                ad.payload
                    .pointer("/holder_authorizations/0/signed_credential")?
                    .clone(),
            )
            .ok()?;
            let digest = digest_wire(&CredentialDigest(signed.credential.digest().ok()?))?;
            Some((ad, digest))
        })
        .collect()
}

/// Badges production guardians publish verify with Fedi's PeerBadge SDK,
/// against the pinned authorities of Manifold's production issuers
#[test]
fn real_production_credentials_verify() {
    let credentials = real_credentials();
    assert!(!credentials.is_empty(), "the fixtures carry credentials");
    for (ad, _) in &credentials {
        let now = ad.issued_at;
        let verified = credential(
            &ad.payload,
            &ad.author,
            now,
            &trust_at(now, default_issuers()),
        );
        assert_eq!(
            verified.state,
            CredentialState::Verified,
            "{:#?}",
            verified.checks
        );
        assert!(verified.checks.iter().all(|c| c.passed == Some(true)));
        assert!(verified
            .issuer_label
            .as_deref()
            .is_some_and(|label| label.starts_with("Manifold production issuer ")));
        assert!(verified.checked_at.is_some());
        assert_eq!(verified.trust_level, Some(9));
    }
}

#[test]
fn a_revoked_credential_fails_and_an_unclear_revocation_does_not_verify() {
    let (ad, digest) = real_credentials().remove(0);
    let now = ad.issued_at;
    let issuer = ad
        .payload
        .pointer("/holder_authorizations/0/signed_credential/credential/issuer_id_pubkey")
        .and_then(Value::as_str)
        .unwrap()
        .to_lowercase();

    let mut trust = trust_at(now, default_issuers());
    trust
        .revocations
        .entries
        .insert((issuer.clone(), digest.clone()), RevocationKind::Revoked);
    let revoked = credential(&ad.payload, &ad.author, now, &trust);
    assert_eq!(revoked.state, CredentialState::Failed);
    assert_eq!(result(&revoked, "revocation"), Some(false));

    // Only events that do not verify: unclear, so not verified, not revoked
    trust
        .revocations
        .entries
        .insert((issuer, digest), RevocationKind::Invalid);
    let unclear = credential(&ad.payload, &ad.author, now, &trust);
    assert_eq!(unclear.state, CredentialState::Unavailable);
    assert_eq!(result(&unclear, "revocation"), None);
}

#[test]
fn without_current_revocation_state_nothing_verifies() {
    let (ad, _) = real_credentials().remove(0);
    let now = ad.issued_at;

    let mut never_read = trust_at(now, default_issuers());
    never_read.revocations.checks.clear();
    let unread = credential(&ad.payload, &ad.author, now, &never_read);
    assert_eq!(unread.state, CredentialState::Unavailable);
    assert_eq!(result(&unread, "revocation"), None);
    // Everything else did verify
    assert_eq!(result(&unread, "issuance_proof"), Some(true));
    assert!(unread
        .reason
        .unwrap()
        .contains("revocations have not been read in full"));

    let stale = trust_at(now - 16 * 60, default_issuers());
    let old = credential(&ad.payload, &ad.author, now, &stale);
    assert_eq!(old.state, CredentialState::Unavailable);
    assert_eq!(result(&old, "revocation"), None);
    // Fifteen minutes old still counts
    let recent = trust_at(now - 15 * 60, default_issuers());
    assert_eq!(
        credential(&ad.payload, &ad.author, now, &recent).state,
        CredentialState::Verified
    );
}

/// Completeness is per issuer: a complete read of other issuers' revocations,
/// however recent, says nothing about an issuer added to the trusted set since
#[test]
fn another_issuers_revocation_read_does_not_vouch_for_this_one() {
    let (ad, _) = real_credentials().remove(0);
    let now = ad.issued_at;
    let issuer = credential_issuer(&ad);

    // Read in full for every production issuer except this credential's, as
    // when the collector ran with a subset and the trusted set grew since
    let mut subset = trust_at(now, default_issuers());
    subset.revocations.checks.remove(&issuer);
    assert!(!subset.revocations.checks.is_empty());
    let expanded = credential(&ad.payload, &ad.author, now, &subset);
    assert_eq!(expanded.state, CredentialState::Unavailable);
    assert_eq!(result(&expanded, "revocation"), None);
    assert_eq!(result(&expanded, "issuer_root"), Some(true));

    // Read in full, but from other locations than the pinned authority names
    let mut elsewhere = trust_at(now, default_issuers());
    elsewhere
        .revocations
        .checks
        .get_mut(&issuer)
        .unwrap()
        .locations = vec!["wss://relay.example.com/".to_owned()];
    let moved = credential(&ad.payload, &ad.author, now, &elsewhere);
    assert_eq!(moved.state, CredentialState::Unavailable);
    assert_eq!(result(&moved, "revocation"), None);
    assert!(moved.reason.unwrap().contains("other locations"));
}

#[test]
fn only_trusted_issuers_with_a_pinned_authority_verify() {
    let (ad, _) = real_credentials().remove(0);
    let now = ad.issued_at;

    // No trust roots: nothing to compare with, so not run
    let none = credential(&ad.payload, &ad.author, now, &trust_at(now, Vec::new()));
    assert_eq!(none.state, CredentialState::Unavailable);
    assert_eq!(result(&none, "issuer_root"), None);
    assert_eq!(result(&none, "issuance_proof"), None);

    // Another root: a checked mismatch, still not a failed credential
    let other = credential(
        &ad.payload,
        &ad.author,
        now,
        &trust_at(now, vec!["00".repeat(32)]),
    );
    assert_eq!(other.state, CredentialState::Unavailable);
    assert_eq!(result(&other, "issuer_root"), Some(false));

    // A trusted issuer outside Manifold's roster has no pinned authority
    let mut unrooted = ad.payload.clone();
    unrooted["holder_authorizations"][0]["signed_credential"]["credential"]["issuer_id_pubkey"] =
        json!("11".repeat(32));
    let unpinned = credential(
        &unrooted,
        &ad.author,
        now,
        &trust_at(now, vec!["11".repeat(32)]),
    );
    assert_eq!(result(&unpinned, "issuer_root"), Some(true));
    assert_eq!(result(&unpinned, "issuer_pinned"), None);
    assert_ne!(unpinned.state, CredentialState::Verified);
}

#[test]
fn tampered_credentials_fail() {
    let (ad, _) = real_credentials().remove(0);
    let now = ad.issued_at;
    let trust = trust_at(now, default_issuers());
    let tampered = |pointer: &str, value: Value| {
        let mut payload = ad.payload.clone();
        *payload.pointer_mut(pointer).unwrap() = value;
        credential(&payload, &ad.author, now, &trust)
    };

    // The issuer signed the trust level: raising it breaks the issuance proof
    let level = "/holder_authorizations/0/signed_credential/credential/info/trust_level";
    let raised = tampered(level, json!(12));
    assert_eq!(raised.state, CredentialState::Failed);
    assert_eq!(result(&raised, "issuance_proof"), Some(false));
    let lowered = tampered(level, json!(7));
    assert_eq!(lowered.state, CredentialState::Failed);
    assert_eq!(result(&lowered, "trust_level"), Some(false));

    // The holder signed the subject: another subject breaks the authorization
    let resubjected = tampered(
        "/holder_authorizations/0/holder_authorization/authorization/subject_pubkey",
        json!("22".repeat(32)),
    );
    assert_eq!(resubjected.state, CredentialState::Failed);
    assert_eq!(result(&resubjected, "holder_authorization"), Some(false));

    // A badge presented by another advertiser
    let borrowed = credential(&ad.payload, &"00".repeat(32), now, &trust);
    assert_eq!(borrowed.state, CredentialState::Failed);
    assert_eq!(result(&borrowed, "subject"), Some(false));

    let none = credential(
        &json!({ "holder_authorizations": [] }),
        &ad.author,
        now,
        &trust,
    );
    assert_eq!(none.state, CredentialState::None);
}
