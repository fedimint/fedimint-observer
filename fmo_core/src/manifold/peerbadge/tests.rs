use base64::Engine as _;
use bitcoin::secp256k1::{Keypair, Message, Secp256k1};
use nostr_sdk::{EventBuilder, Keys, Kind, Tag};
use peerbadge_protocol::Revocation;
use sha2::Digest as _;

use super::*;

#[test]
fn pinned_authorities_are_manifolds_production_roster() {
    check_pinned_authorities().unwrap();
    let pinned: Vec<&String> = pinned_authorities().keys().collect();
    let mut roster: Vec<&str> = MANIFOLD_PRODUCTION_ISSUERS.to_vec();
    roster.sort_unstable();
    assert_eq!(pinned, roster);
    // A changed document no longer verifies
    let tampered = AUTHORITY_DOCUMENTS[0].replace("relay.dev.fedibtc.com", "relay.example.com");
    assert!(load_authorities(&[&tampered]).is_err());
    assert!(load_authorities(&[AUTHORITY_DOCUMENTS[0], AUTHORITY_DOCUMENTS[0]]).is_err());
}

#[test]
fn roster_issuers_are_labelled_by_position() {
    assert_eq!(
        issuer_label(&MANIFOLD_PRODUCTION_ISSUERS[2].to_uppercase()).as_deref(),
        Some("Manifold production issuer 3 of 8")
    );
    assert_eq!(issuer_label(&"00".repeat(32)), None);
}

#[test]
fn revocations_are_read_where_the_pinned_authorities_say() {
    let roster: Vec<String> = MANIFOLD_PRODUCTION_ISSUERS
        .iter()
        .map(|key| (*key).to_owned())
        .collect();
    let targets = revocation_targets(&roster).unwrap();
    assert_eq!(
        targets.keys().collect::<Vec<_>>(),
        ["wss://relay.dev.fedibtc.com/"]
    );
    assert_eq!(targets.values().next().unwrap().len(), roster.len());
    // Trusted issuers without a pinned authority have nowhere to read from
    assert!(revocation_targets(&["00".repeat(32)]).unwrap().is_empty());
}

/// A credential digest, as revocations carry it
fn digest_of(text: &str) -> CredentialDigest {
    CredentialDigest(sha2::Sha256::digest(text.as_bytes()))
}

/// The issuer's signed revocation of `digest`, as JSON
fn signed_revocation(issuer: &Keys, digest: &CredentialDigest) -> String {
    let revocation = Revocation {
        credential_digest: digest.clone(),
    };
    let message = Message::from_digest(revocation.digest().unwrap().into());
    let secp = Secp256k1::new();
    let keypair =
        Keypair::from_seckey_slice(&secp, &issuer.secret_key().unwrap().secret_bytes()).unwrap();
    let signature = secp.sign_schnorr_no_aux_rand(&message, &keypair);
    serde_json::json!({
        "version": 1,
        "revocation": { "credential_digest": digest_wire(digest).unwrap() },
        "proof": {
            "issuer_id_pubkey": issuer.public_key().to_hex(),
            "signature": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.as_ref()),
        },
    })
    .to_string()
}

fn revocation_event(author: &Keys, d_tags: &[String], content: String) -> nostr_sdk::Event {
    let tags = d_tags
        .iter()
        .map(|d| Tag::identifier(d.clone()))
        .collect::<Vec<_>>();
    EventBuilder::new(Kind::Custom(REVOCATION_KIND), content, tags)
        .to_event(author)
        .unwrap()
}

#[test]
fn revocations_are_admitted_as_manifold_admits_them() {
    let issuer = Keys::generate();
    let trusted = vec![issuer.public_key().to_hex()];
    let digest = digest_of("credential");
    let wire = digest_wire(&digest).unwrap();
    let d_tag = format!("credential-revocation:{wire}");
    // The SDK accepts the hand-made revocation as the issuer's
    assert!(
        serde_json::from_str::<SignedRevocation>(&signed_revocation(&issuer, &digest))
            .unwrap()
            .verify()
            .is_ok()
    );

    let valid = revocation_event(
        &issuer,
        std::slice::from_ref(&d_tag),
        signed_revocation(&issuer, &digest),
    );
    assert_eq!(
        admit_revocation(&valid, &trusted),
        Some(RevocationRecord {
            issuer: trusted[0].clone(),
            credential_digest: wire.clone(),
            kind: RevocationKind::Revoked,
            event_id: valid.id().to_hex(),
        })
    );

    // Issuer events for this credential that do not revoke it are invalid,
    // which keeps the credential unverified rather than unrevoked
    let other_digest = revocation_event(
        &issuer,
        std::slice::from_ref(&d_tag),
        signed_revocation(&issuer, &digest_of("another credential")),
    );
    let unsigned_content = revocation_event(&issuer, std::slice::from_ref(&d_tag), "{}".into());
    let someone_else = Keys::generate();
    let signed_by_another = revocation_event(
        &issuer,
        std::slice::from_ref(&d_tag),
        signed_revocation(&someone_else, &digest),
    );
    for event in [&other_digest, &unsigned_content, &signed_by_another] {
        assert_eq!(
            admit_revocation(event, &trusted).map(|record| record.kind),
            Some(RevocationKind::Invalid)
        );
    }

    // Not about a credential, or not from a trusted issuer: ignored
    let untrusted = revocation_event(
        &someone_else,
        std::slice::from_ref(&d_tag),
        signed_revocation(&someone_else, &digest),
    );
    let no_d_tag = revocation_event(&issuer, &[], signed_revocation(&issuer, &digest));
    let two_d_tags = revocation_event(
        &issuer,
        &[d_tag.clone(), d_tag.clone()],
        signed_revocation(&issuer, &digest),
    );
    let other_prefix = revocation_event(
        &issuer,
        &[format!("credential-revoked:{wire}")],
        signed_revocation(&issuer, &digest),
    );
    for event in [&untrusted, &no_d_tag, &two_d_tags, &other_prefix] {
        assert_eq!(admit_revocation(event, &trusted), None);
    }
}

#[test]
fn an_issuers_revocation_state_counts_only_for_it_its_locations_and_fifteen_minutes() {
    let now = Utc::now();
    let issuer = MANIFOLD_PRODUCTION_ISSUERS[0];
    let locations = revocation_locations(&pinned_authorities()[issuer]).unwrap();
    assert_eq!(locations, ["wss://relay.dev.fedibtc.com/"]);
    let state = |checked_at: DateTime<Utc>, locations: Vec<String>| RevocationState {
        checks: HashMap::from([(
            issuer.to_owned(),
            IssuerRevocationCheck {
                checked_at,
                locations,
            },
        )]),
        entries: HashMap::new(),
    };
    let fresh = state(now, locations.clone());
    assert_eq!(fresh.current_check(issuer, &locations, now), Ok(now));
    assert_eq!(
        fresh.current_check(MANIFOLD_PRODUCTION_ISSUERS[1], &locations, now),
        Err(RevocationGap::NeverRead)
    );
    assert_eq!(
        fresh.current_check(issuer, &["wss://relay.example.com/".to_owned()], now),
        Err(RevocationGap::OtherLocations)
    );
    let edge = now - REVOCATION_MAX_AGE;
    assert_eq!(
        state(edge, locations.clone()).current_check(issuer, &locations, now),
        Ok(edge)
    );
    assert_eq!(
        state(edge - TimeDelta::seconds(1), locations.clone())
            .current_check(issuer, &locations, now),
        Err(RevocationGap::Stale)
    );
}
