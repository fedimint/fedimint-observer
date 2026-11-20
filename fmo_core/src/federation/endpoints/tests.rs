use std::collections::BTreeMap;
use std::str::FromStr;

use fedimint_core::config::{FederationId, GlobalClientConfig, PeerUrl};
use fedimint_core::module::CoreConsensusVersion;
use fedimint_core::net::api_announcement::ApiAnnouncement;
use fedimint_core::secp256k1::{Keypair, PublicKey, SecretKey, SECP256K1};
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;

use super::*;

fn generate_test_guardian(seed: u8) -> (PeerId, SecretKey, Keypair, PublicKey) {
    let mut key_bytes = [1u8; 32];
    key_bytes[0] = seed.wrapping_add(1);
    let sk = SecretKey::from_slice(&key_bytes).expect("valid secret key");
    let keypair = Keypair::from_secret_key(SECP256K1, &sk);
    let pk = PublicKey::from_keypair(&keypair);
    (PeerId::from(seed as u16), sk, keypair, pk)
}

fn sample_global_config(
    peers: &[(PeerId, PublicKey, SafeUrl)],
) -> (FederationId, GlobalClientConfig) {
    let mut api_endpoints = BTreeMap::new();
    let mut broadcast_public_keys = BTreeMap::new();

    for (peer_id, pk, url) in peers {
        api_endpoints.insert(
            *peer_id,
            PeerUrl {
                url: url.clone(),
                name: format!("guardian_{peer_id}"),
            },
        );
        broadcast_public_keys.insert(*peer_id, *pk);
    }

    let global = GlobalClientConfig {
        api_endpoints,
        broadcast_public_keys: Some(broadcast_public_keys),
        consensus_version: CoreConsensusVersion::new(0, 0),
        meta: BTreeMap::new(),
    };

    let fed_id = global.calculate_federation_id();
    (fed_id, global)
}

#[test]
fn signed_rotation_overrides_only_the_target_guardian() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (p1, _, _k1, pk1) = generate_test_guardian(1);

    let url0_initial = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let url1_initial = SafeUrl::from_str("ws://guardian1.example.com:8173").unwrap();

    let (fed_id, config) = sample_global_config(&[
        (p0, pk0, url0_initial.clone()),
        (p1, pk1, url1_initial.clone()),
    ]);

    let url0_migrated = SafeUrl::from_str("wss://guardian0-new.example.com:8173").unwrap();
    let signed0 = ApiAnnouncement::new(url0_migrated.clone(), 10).sign(SECP256K1, &k0);
    let mut incoming = BTreeMap::new();
    incoming.insert(p0, signed0);

    let (updated, changed) =
        verify_and_merge_announcements(fed_id, &config, &BTreeMap::new(), &incoming);

    assert!(changed, "the valid signed rotation must be accepted");
    let active = active_api_peers(&config, Some(&updated));
    assert_eq!(
        active.get(&p0),
        Some(&url0_migrated),
        "the rotated guardian must be dialed at its announced route"
    );
    assert_eq!(
        active.get(&p1),
        Some(&url1_initial),
        "an unannounced guardian must keep its canonical route"
    );
}

#[test]
fn higher_nonce_replaces_the_previously_accepted_route() {
    let (peer, _, keypair, public_key) = generate_test_guardian(0);
    let canonical_url = SafeUrl::from_str("ws://guardian.example.com:8173").unwrap();
    let (federation_id, config) = sample_global_config(&[(peer, public_key, canonical_url)]);

    let first_url = SafeUrl::from_str("wss://guardian-v1.example.com:8173").unwrap();
    let first = ApiAnnouncement::new(first_url, 10).sign(SECP256K1, &keypair);
    let (current, first_changed) = verify_and_merge_announcements(
        federation_id,
        &config,
        &BTreeMap::new(),
        &[(peer, first)].into_iter().collect(),
    );

    let newest_url = SafeUrl::from_str("wss://guardian-v2.example.com:8173").unwrap();
    let newest = ApiAnnouncement::new(newest_url.clone(), 20).sign(SECP256K1, &keypair);
    let (updated, changed) = verify_and_merge_announcements(
        federation_id,
        &config,
        &current,
        &[(peer, newest)].into_iter().collect(),
    );

    assert!(
        first_changed,
        "the first valid announcement must be accepted"
    );
    assert!(changed, "a strictly newer announcement must replace it");
    let route = updated
        .get(&peer)
        .expect("the guardian must have an accepted route");
    assert_eq!(route.url(), &newest_url);
    assert_eq!(route.nonce(), 20);
}

#[test]
fn canonical_recovery_accepts_second_rotation_without_stale_downgrade() {
    let (peer, _, keypair, public_key) = generate_test_guardian(0);
    let canonical_url = SafeUrl::from_str("wss://canonical.example.com:8173").unwrap();
    let (federation_id, config) =
        sample_global_config(&[(peer, public_key, canonical_url.clone())]);
    let first_url = SafeUrl::from_str("wss://first-rotation.example.com:8173").unwrap();
    let first = ApiAnnouncement::new(first_url, 1).sign(SECP256K1, &keypair);
    let (first_routes, changed) = verify_and_merge_announcements(
        federation_id,
        &config,
        &BTreeMap::new(),
        &[(peer, first.clone())].into_iter().collect(),
    );
    assert!(changed);

    // A canonical guardian may return an old and a newer route.
    let second_url = SafeUrl::from_str("wss://second-rotation.example.com:8173").unwrap();
    let second = ApiAnnouncement::new(second_url.clone(), 2).sign(SECP256K1, &keypair);
    let (after_stale, stale_changed) = verify_and_merge_announcements(
        federation_id,
        &config,
        &first_routes,
        &[(peer, first)].into_iter().collect(),
    );
    let (recovered, recovered_changed) = verify_and_merge_announcements(
        federation_id,
        &config,
        &after_stale,
        &[(peer, second)].into_iter().collect(),
    );

    assert!(!stale_changed, "the stale route must not be restored");
    assert!(
        recovered_changed,
        "the newer recovery route must be accepted"
    );
    assert_eq!(
        recovered.get(&peer).map(VerifiedRoute::url),
        Some(&second_url)
    );
    assert_eq!(
        config
            .api_endpoints
            .get(&peer)
            .map(|endpoint| &endpoint.url),
        Some(&canonical_url),
        "recovery routing must not mutate federation identity"
    );
}

#[test]
fn announcement_signed_by_another_guardian_is_ignored() {
    let (p0, _, _, pk0) = generate_test_guardian(0);
    let (_, _, k1_attacker, _) = generate_test_guardian(1);

    let url0 = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let (fed_id, config) = sample_global_config(&[(p0, pk0, url0)]);

    let malicious_url = SafeUrl::from_str("ws://malicious.example.com:8173").unwrap();
    // Signed with guardian 1 keypair instead of guardian 0
    let forged = ApiAnnouncement::new(malicious_url, 10).sign(SECP256K1, &k1_attacker);

    let mut incoming = BTreeMap::new();
    incoming.insert(p0, forged);

    let existing = BTreeMap::new();
    let (updated, changed) = verify_and_merge_announcements(fed_id, &config, &existing, &incoming);

    assert!(!changed, "a signature from the wrong guardian must fail");
    assert!(
        updated.is_empty(),
        "a forged announcement must not enter the routing cache"
    );
}

#[test]
fn equal_or_lower_nonce_cannot_roll_back_an_accepted_route() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let url0 = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let (fed_id, config) = sample_global_config(&[(p0, pk0, url0)]);

    let signed_v10 = ApiAnnouncement::new(
        SafeUrl::from_str("ws://migrated.example.com:8173").unwrap(),
        10,
    )
    .sign(SECP256K1, &k0);

    let mut incoming = BTreeMap::new();
    incoming.insert(p0, signed_v10);

    let (existing, _) =
        verify_and_merge_announcements(fed_id, &config, &BTreeMap::new(), &incoming);

    // 1. Equal nonce (10) rejected
    let signed_equal = ApiAnnouncement::new(
        SafeUrl::from_str("ws://equal.example.com:8173").unwrap(),
        10,
    )
    .sign(SECP256K1, &k0);
    let mut incoming_equal = BTreeMap::new();
    incoming_equal.insert(p0, signed_equal);

    let (updated_equal, changed_equal) =
        verify_and_merge_announcements(fed_id, &config, &existing, &incoming_equal);
    assert!(
        !changed_equal,
        "an equal nonce must not replace the accepted route"
    );
    assert_eq!(
        updated_equal
            .get(&p0)
            .expect("the previously accepted route must remain")
            .url()
            .as_str(),
        "ws://migrated.example.com:8173/",
        "an equal nonce must not change the destination"
    );

    // 2. Lower nonce (5) rejected
    let signed_lower =
        ApiAnnouncement::new(SafeUrl::from_str("ws://lower.example.com:8173").unwrap(), 5)
            .sign(SECP256K1, &k0);
    let mut incoming_lower = BTreeMap::new();
    incoming_lower.insert(p0, signed_lower);

    let (updated_lower, changed_lower) =
        verify_and_merge_announcements(fed_id, &config, &existing, &incoming_lower);
    assert!(!changed_lower, "a lower nonce must not roll the route back");
    assert_eq!(
        updated_lower
            .get(&p0)
            .expect("the previously accepted route must remain")
            .url()
            .as_str(),
        "ws://migrated.example.com:8173/",
        "a lower nonce must not change the destination"
    );
}

#[test]
fn unsupported_transport_is_never_added_to_active_routes() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let canonical_url = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let (fed_id, config) = sample_global_config(&[(p0, pk0, canonical_url.clone())]);

    let invalid_url = SafeUrl::from_str("ftp://guardian0.example.com:8173").unwrap();
    let signed = ApiAnnouncement::new(invalid_url, 10).sign(SECP256K1, &k0);

    let mut incoming = BTreeMap::new();
    incoming.insert(p0, signed);

    let (updated, changed) =
        verify_and_merge_announcements(fed_id, &config, &BTreeMap::new(), &incoming);

    assert!(!changed, "an unsupported transport must be rejected");
    assert_eq!(
        active_api_peers(&config, Some(&updated)).get(&p0),
        Some(&canonical_url),
        "rejecting the announcement must leave the canonical route active"
    );
}

#[test]
fn gateway_only_http_transport_is_rejected_for_guardian_routes() {
    let (peer, _, keypair, public_key) = generate_test_guardian(0);
    let canonical_url = SafeUrl::from_str("wss://guardian.example.com:8173").unwrap();
    let (federation_id, config) =
        sample_global_config(&[(peer, public_key, canonical_url.clone())]);

    let http_url = SafeUrl::from_str("https://guardian.example.com/api").unwrap();
    let signed = ApiAnnouncement::new(http_url, 10).sign(SECP256K1, &keypair);
    let (updated, changed) = verify_and_merge_announcements(
        federation_id,
        &config,
        &BTreeMap::new(),
        &[(peer, signed)].into_iter().collect(),
    );

    assert!(
        !changed,
        "the HTTP connector serves gateways but cannot connect to guardians"
    );
    assert_eq!(
        active_api_peers(&config, Some(&updated)).get(&peer),
        Some(&canonical_url),
        "rejecting a gateway-only transport must preserve the working guardian route"
    );
}

#[test]
fn federation_without_broadcast_keys_keeps_canonical_routes() {
    let (p0, _, k0, _) = generate_test_guardian(0);

    let url0 = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let global = GlobalClientConfig {
        api_endpoints: [(
            p0,
            PeerUrl {
                url: url0.clone(),
                name: "g0".into(),
            },
        )]
        .into_iter()
        .collect(),
        broadcast_public_keys: None,
        consensus_version: CoreConsensusVersion::new(0, 0),
        meta: BTreeMap::new(),
    };
    let fed_id = global.calculate_federation_id();

    let signed = ApiAnnouncement::new(SafeUrl::from_str("ws://new.example.com:8173").unwrap(), 10)
        .sign(SECP256K1, &k0);

    let mut incoming = BTreeMap::new();
    incoming.insert(p0, signed);

    let (updated, changed) =
        verify_and_merge_announcements(fed_id, &global, &BTreeMap::new(), &incoming);

    assert!(
        !changed,
        "an announcement cannot be trusted without canonical broadcast keys"
    );
    assert_eq!(
        active_api_peers(&global, Some(&updated)).get(&p0),
        Some(&url0),
        "the canonical route must remain active when verification is impossible"
    );
}

#[test]
fn replica_restore_reverifies_signatures_and_preserves_newest_nonce() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (p1, _, _, pk1) = generate_test_guardian(1);
    let (_, _, attacker, _) = generate_test_guardian(2);

    let url0 = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let url1 = SafeUrl::from_str("ws://guardian1.example.com:8173").unwrap();
    let (fed_id, config) = sample_global_config(&[(p0, pk0, url0), (p1, pk1, url1)]);

    let valid_signed =
        ApiAnnouncement::new(SafeUrl::from_str("ws://valid.example.com:8173").unwrap(), 5)
            .sign(SECP256K1, &k0);
    let forged_signed = ApiAnnouncement::new(
        SafeUrl::from_str("ws://forged.example.com:8173").unwrap(),
        6,
    )
    .sign(SECP256K1, &attacker);

    let mut stored = BTreeMap::new();
    stored.insert(p0, valid_signed);
    stored.insert(p1, forged_signed);

    let (verified, changed) =
        verify_and_merge_announcements(fed_id, &config, &BTreeMap::new(), &stored);

    assert!(changed, "the valid persisted route must be restored");
    assert_eq!(
        verified.len(),
        1,
        "the forged persisted route must be discarded"
    );
    assert_eq!(verified.get(&p0).map(VerifiedRoute::nonce), Some(5));

    let stale = ApiAnnouncement::new(SafeUrl::from_str("ws://stale.example.com:8173").unwrap(), 4)
        .sign(SECP256K1, &k0);
    let (restored, changed) = verify_and_merge_announcements(
        fed_id,
        &config,
        &verified,
        &[(p0, stale)].into_iter().collect(),
    );
    assert!(
        !changed,
        "a stale database snapshot must not downgrade a replica"
    );
    assert_eq!(restored, verified, "the newest verified route must survive");
}

#[test]
fn route_rotation_does_not_change_the_canonical_federation_id() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (p1, _, _, pk1) = generate_test_guardian(1);

    let url0_initial = SafeUrl::from_str("ws://g0.example.com:8173").unwrap();
    let url1_initial = SafeUrl::from_str("ws://g1.example.com:8173").unwrap();

    let (fed_id, config) = sample_global_config(&[
        (p0, pk0, url0_initial.clone()),
        (p1, pk1, url1_initial.clone()),
    ]);

    // Baseline federation ID before rotation
    let baseline_fed_id = config.calculate_federation_id();
    assert_eq!(fed_id, baseline_fed_id);

    // Rotate p0 endpoint
    let url0_rotated = SafeUrl::from_str("ws://g0-new.example.com:8173").unwrap();
    let signed_rotated = ApiAnnouncement::new(url0_rotated.clone(), 1).sign(SECP256K1, &k0);

    let mut incoming = BTreeMap::new();
    incoming.insert(p0, signed_rotated);

    let (announced, changed) =
        verify_and_merge_announcements(fed_id, &config, &BTreeMap::new(), &incoming);
    assert!(changed, "the signed rotation must affect active dialing");

    // Dialing peers uses the rotated URL
    let active_peers = active_api_peers(&config, Some(&announced));
    assert_eq!(
        active_peers.get(&p0),
        Some(&url0_rotated),
        "active dialing must use the rotated route"
    );

    // The canonical config and federation ID are unchanged
    assert_eq!(
        config.calculate_federation_id(),
        baseline_fed_id,
        "routing changes must not mutate federation identity"
    );
    assert_eq!(
        config
            .api_endpoints
            .get(&p0)
            .expect("peer zero is in the canonical config")
            .url,
        url0_initial,
        "the exported canonical endpoint must remain unchanged"
    );
}

#[test]
fn valid_rotation_from_another_peer_survives_a_forged_high_nonce() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (p1, _, k1, pk1) = generate_test_guardian(1);

    let (fed_id, config) = sample_global_config(&[
        (
            p0,
            pk0,
            SafeUrl::from_str("ws://g0.example.com:8173").unwrap(),
        ),
        (
            p1,
            pk1,
            SafeUrl::from_str("ws://g1.example.com:8173").unwrap(),
        ),
    ]);

    let url0_v2 = SafeUrl::from_str("ws://g0-v2.example.com:8173").unwrap();
    let ann0_v2 = ApiAnnouncement::new(url0_v2.clone(), 2).sign(SECP256K1, &k0);
    let url1_v5 = SafeUrl::from_str("ws://g1-v5.example.com:8173").unwrap();
    let ann1_v5 = ApiAnnouncement::new(url1_v5.clone(), 5).sign(SECP256K1, &k1);
    let forged_high_nonce = ApiAnnouncement::new(
        SafeUrl::from_str("ws://forged.example.com:8173").unwrap(),
        100,
    )
    .sign(SECP256K1, &k1);

    let mut peer_a_response = BTreeMap::new();
    peer_a_response.insert(p0, forged_high_nonce);
    let mut peer_b_response = BTreeMap::new();
    peer_b_response.insert(p0, ann0_v2);
    peer_b_response.insert(p1, ann1_v5);

    // Discovery verifies each response on its own, as it arrives.
    let mut updated = GuardianEndpointsMap::new();
    let mut changed = false;
    for response in [peer_a_response, peer_b_response] {
        let (next, response_changed) =
            verify_and_merge_announcements(fed_id, &config, &updated, &response);
        updated = next;
        changed |= response_changed;
    }

    assert!(
        changed,
        "valid responses must still update routes despite the forged response"
    );
    let peer0_route = updated
        .get(&p0)
        .expect("peer zero's valid rotation must be retained");
    assert_eq!(peer0_route.url(), &url0_v2);
    assert_eq!(peer0_route.nonce(), 2);
    let peer1_route = updated
        .get(&p1)
        .expect("peer one's valid rotation must be retained");
    assert_eq!(peer1_route.url(), &url1_v5);
    assert_eq!(peer1_route.nonce(), 5);
}

#[test]
fn public_guardian_cannot_announce_a_non_public_address() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (fed_id, config) = sample_global_config(&[(
        p0,
        pk0,
        SafeUrl::from_str("wss://guardian0.example.com/").unwrap(),
    )]);
    let announce = |url: &str| {
        let signed = ApiAnnouncement::new(SafeUrl::from_str(url).unwrap(), 1).sign(SECP256K1, &k0);
        verify_and_merge_announcements(
            fed_id,
            &config,
            &BTreeMap::new(),
            &BTreeMap::from([(p0, signed)]),
        )
        .1
    };

    for url in [
        "ws://127.0.0.1:8173/",
        "ws://2130706433:8173/",
        "wss://10.0.0.5/",
        "ws://169.254.169.254/latest/meta-data/",
        "ws://[::1]:8173/",
        "ws://[::ffff:192.168.1.1]/",
        "ws://localhost:8173/",
        "ws://admin.LOCALHOST./",
    ] {
        assert!(!announce(url), "{url} must be rejected");
    }
    for url in [
        "wss://guardian0-new.example.com/",
        "ws://1.1.1.1:8173/",
        "iroh://4ezhsc3hwr4q6n6r4h4yt4uf7pdazkbl2yoyevvwz5dcp7un6wka",
    ] {
        assert!(announce(url), "{url} must be accepted");
    }
}

#[test]
fn private_guardian_may_announce_another_private_address() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (fed_id, config) =
        sample_global_config(&[(p0, pk0, SafeUrl::from_str("ws://127.0.0.1:8173/").unwrap())]);
    let signed = ApiAnnouncement::new(SafeUrl::from_str("ws://127.0.0.1:9173/").unwrap(), 1)
        .sign(SECP256K1, &k0);
    let (_, changed) = verify_and_merge_announcements(
        fed_id,
        &config,
        &BTreeMap::new(),
        &BTreeMap::from([(p0, signed)]),
    );
    assert!(
        changed,
        "a LAN or local federation keeps rotating within its network"
    );
}
