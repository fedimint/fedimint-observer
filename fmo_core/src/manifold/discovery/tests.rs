use fmo_api_types::AdFreshness;
use nostr_sdk::{EventBuilder, JsonUtil, Keys, Kind, Tag};
use serde_json::json;

use super::*;

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

#[test]
fn credentials_are_never_reported_verified() {
    let events = real_events();
    let v2 = verify_event(&events[1]).unwrap();
    let now = v2.issued_at;

    let real = credential(&v2.payload, &v2.author, now, &[]);
    assert_eq!(real.state, CredentialState::Unavailable);
    assert_eq!(real.trust_level, Some(9));
    assert_eq!(real.schema.as_deref(), Some("fedi-trust-score-v1.0"));
    let result = |id: &str| real.checks.iter().find(|c| c.id == id).unwrap().passed;
    // No trust roots configured: nothing to compare with, so not run
    assert_eq!(result("issuer_root"), None);
    assert_eq!(result("subject"), Some(true));
    assert_eq!(result("blind_msg"), Some(true));
    assert_eq!(result("trust_level"), Some(true));
    assert_eq!(result("issuance_proof"), None);
    assert_eq!(result("revocation"), None);

    // Even a pinned issuer stays unverified without the PeerBadge proofs
    let pinned = credential(
        &v2.payload,
        &v2.author,
        now,
        &[real.issuer_pubkey.clone().unwrap()],
    );
    assert_eq!(pinned.state, CredentialState::Unavailable);
    let pinned_result = pinned
        .checks
        .iter()
        .find(|c| c.id == "issuer_root")
        .unwrap()
        .passed;
    assert_eq!(pinned_result, Some(true));

    // A configured root that is not this issuer is a real, checked mismatch
    let other = credential(&v2.payload, &v2.author, now, &["00".repeat(32)]);
    let other_result = other
        .checks
        .iter()
        .find(|c| c.id == "issuer_root")
        .unwrap()
        .passed;
    assert_eq!(other_result, Some(false));
    assert_eq!(other.state, CredentialState::Unavailable);

    // Claims the observer can check do fail a credential
    let mut low = v2.payload.clone();
    low["holder_authorizations"][0]["signed_credential"]["credential"]["info"]["trust_level"] =
        json!(7);
    assert_eq!(
        credential(&low, &v2.author, now, &[]).state,
        CredentialState::Failed
    );
    assert_eq!(
        credential(&v2.payload, &"00".repeat(32), now, &[]).state,
        CredentialState::Failed
    );

    let none = credential(
        &json!({ "holder_authorizations": [] }),
        &v2.author,
        now,
        &[],
    );
    assert_eq!(none.state, CredentialState::None);
}
