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
fn test_accept_valid_announcement_and_rotation() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (p1, _, _k1, pk1) = generate_test_guardian(1);

    let url0_initial = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let url1_initial = SafeUrl::from_str("ws://guardian1.example.com:8173").unwrap();

    let (fed_id, config) = sample_global_config(&[
        (p0, pk0, url0_initial.clone()),
        (p1, pk1, url1_initial.clone()),
    ]);

    // Baseline: fall back to config URLs when no announcements
    let initial_peers = active_api_peers(&config, None);
    assert_eq!(initial_peers.get(&p0), Some(&url0_initial));
    assert_eq!(initial_peers.get(&p1), Some(&url1_initial));

    let existing = BTreeMap::new();
    let url0_migrated = SafeUrl::from_str("wss://guardian0-new.example.com:8173").unwrap();

    let signed0 = ApiAnnouncement::new(url0_migrated.clone(), 10).sign(SECP256K1, &k0);

    let mut incoming = BTreeMap::new();
    incoming.insert(p0, signed0.clone());

    let (updated, changed) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &existing,
        &incoming,
    );

    assert!(changed);
    assert_eq!(updated.len(), 1);
    assert_eq!(updated.get(&p0).unwrap().url, url0_migrated);
    assert_eq!(updated.get(&p0).unwrap().nonce, 10);

    // Active peers should reflect updated endpoint for p0 and fallback to
    // config for p1
    let active = active_api_peers(&config, Some(&updated));
    assert_eq!(active.get(&p0), Some(&url0_migrated));
    assert_eq!(active.get(&p1), Some(&url1_initial));

    // Monotonic nonce rotation: newer announcement with nonce 20 is accepted
    let url0_rotated_again = SafeUrl::from_str("wss://guardian0-v2.example.com:8173").unwrap();
    let signed0_v2 = ApiAnnouncement::new(url0_rotated_again.clone(), 20).sign(SECP256K1, &k0);

    let mut incoming_v2 = BTreeMap::new();
    incoming_v2.insert(p0, signed0_v2);

    let (updated_v2, changed_v2) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &updated,
        &incoming_v2,
    );

    assert!(changed_v2);
    assert_eq!(updated_v2.get(&p0).unwrap().url, url0_rotated_again);
    assert_eq!(updated_v2.get(&p0).unwrap().nonce, 20);
}

#[test]
fn test_reject_forged_signature() {
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
    let (updated, changed) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &existing,
        &incoming,
    );

    assert!(!changed);
    assert!(updated.is_empty());
}

#[test]
fn test_reject_stale_or_equal_nonce() {
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

    let (existing, _) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &BTreeMap::new(),
        &incoming,
    );

    // 1. Equal nonce (10) rejected
    let signed_equal = ApiAnnouncement::new(
        SafeUrl::from_str("ws://equal.example.com:8173").unwrap(),
        10,
    )
    .sign(SECP256K1, &k0);
    let mut incoming_equal = BTreeMap::new();
    incoming_equal.insert(p0, signed_equal);

    let (updated_equal, changed_equal) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &existing,
        &incoming_equal,
    );
    assert!(!changed_equal);
    assert_eq!(
        updated_equal.get(&p0).unwrap().url.as_str(),
        "ws://migrated.example.com:8173/"
    );

    // 2. Lower nonce (5) rejected
    let signed_lower =
        ApiAnnouncement::new(SafeUrl::from_str("ws://lower.example.com:8173").unwrap(), 5)
            .sign(SECP256K1, &k0);
    let mut incoming_lower = BTreeMap::new();
    incoming_lower.insert(p0, signed_lower);

    let (updated_lower, changed_lower) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &existing,
        &incoming_lower,
    );
    assert!(!changed_lower);
    assert_eq!(
        updated_lower.get(&p0).unwrap().url.as_str(),
        "ws://migrated.example.com:8173/"
    );
}

#[test]
fn test_reject_unsupported_url_schemes() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let url0 = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let (fed_id, config) = sample_global_config(&[(p0, pk0, url0)]);

    for unsupported_url in [
        "ftp://guardian0.example.com:8173",
        "http://guardian0.example.com:8173",
        "https://guardian0.example.com:8173",
    ] {
        let invalid_url = SafeUrl::from_str(unsupported_url).unwrap();
        let signed = ApiAnnouncement::new(invalid_url, 10).sign(SECP256K1, &k0);

        let mut incoming = BTreeMap::new();
        incoming.insert(p0, signed);

        let (updated, changed) = verify_and_merge_announcements(
            fed_id,
            config.broadcast_public_keys.as_ref(),
            &BTreeMap::new(),
            &incoming,
        );

        assert!(!changed);
        assert!(updated.is_empty());
    }
}

#[test]
fn test_missing_broadcast_public_keys() {
    let (p0, _, k0, _) = generate_test_guardian(0);

    let url0 = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let global = GlobalClientConfig {
        api_endpoints: [(
            p0,
            PeerUrl {
                url: url0,
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

    let (updated, changed) = verify_and_merge_announcements(
        fed_id,
        global.broadcast_public_keys.as_ref(),
        &BTreeMap::new(),
        &incoming,
    );

    assert!(!changed);
    assert!(updated.is_empty());
}

#[test]
fn test_verify_restored_routes_preserves_code_invariant() {
    let (p0, _, k0, pk0) = generate_test_guardian(0);
    let (_p1, _, k1_attacker, _) = generate_test_guardian(1);

    let url0 = SafeUrl::from_str("ws://guardian0.example.com:8173").unwrap();
    let (fed_id, config) = sample_global_config(&[(p0, pk0, url0)]);

    // Valid route
    let valid_signed =
        ApiAnnouncement::new(SafeUrl::from_str("ws://valid.example.com:8173").unwrap(), 5)
            .sign(SECP256K1, &k0);
    let valid_route = VerifiedRoute::verify(valid_signed, &pk0).unwrap();

    // Forged route that somehow got into stored data
    let forged_signed = ApiAnnouncement::new(
        SafeUrl::from_str("ws://forged.example.com:8173").unwrap(),
        6,
    )
    .sign(SECP256K1, &k1_attacker);
    let forged_route = VerifiedRoute {
        url: SafeUrl::from_str("ws://forged.example.com:8173").unwrap(),
        nonce: 6,
        announcement: forged_signed,
    };

    let mut stored = BTreeMap::new();
    stored.insert(p0, valid_route.clone());
    stored.insert(PeerId::from(99), forged_route);

    let verified = verify_restored_routes(fed_id, config.broadcast_public_keys.as_ref(), &stored);

    // Only p0 with valid signature survives restoration
    assert_eq!(verified.len(), 1);
    assert_eq!(verified.get(&p0), Some(&valid_route));
}

#[test]
fn test_federation_id_stability_across_rotations() {
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

    let (announced, changed) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &BTreeMap::new(),
        &incoming,
    );
    assert!(changed);

    // Dialing peers uses the rotated URL
    let active_peers = active_api_peers(&config, Some(&announced));
    assert_eq!(active_peers.get(&p0), Some(&url0_rotated));

    // Invariant: Canonical config is untouched; FederationId is identical
    assert_eq!(config.calculate_federation_id(), baseline_fed_id);
    assert_eq!(config.api_endpoints.get(&p0).unwrap().url, url0_initial);
}

#[test]
fn test_concurrent_multi_peer_announcement_merge_keeps_highest_nonce() {
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

    // Guardian 0 announces rotation with nonce 1
    let url0_v1 = SafeUrl::from_str("ws://g0-v1.example.com:8173").unwrap();
    let ann0_v1 = ApiAnnouncement::new(url0_v1, 1).sign(SECP256K1, &k0);

    // Later, Guardian 0 rotates to v2 with nonce 2
    let url0_v2 = SafeUrl::from_str("ws://g0-v2.example.com:8173").unwrap();
    let ann0_v2 = ApiAnnouncement::new(url0_v2.clone(), 2).sign(SECP256K1, &k0);

    // Peer 1 has up-to-date announcement for Guardian 1 with nonce 5
    let url1_v5 = SafeUrl::from_str("ws://g1-v5.example.com:8173").unwrap();
    let ann1_v5 = ApiAnnouncement::new(url1_v5.clone(), 5).sign(SECP256K1, &k1);

    // Peer A response: has stale v1 for Peer 0
    let mut peer_a_response = BTreeMap::new();
    peer_a_response.insert(p0, ann0_v1);

    // Peer B response: has fresh v2 for Peer 0 and fresh v5 for Peer 1
    let mut peer_b_response = BTreeMap::new();
    peer_b_response.insert(p0, ann0_v2);
    peer_b_response.insert(p1, ann1_v5);

    // Simulated multi-peer merge (same logic as in guardians.rs)
    let peer_responses = vec![peer_a_response, peer_b_response];
    let mut merged_incoming = BTreeMap::new();
    for announcements in peer_responses {
        for (target_peer, announcement) in announcements {
            merged_incoming
                .entry(target_peer)
                .and_modify(|existing: &mut SignedApiAnnouncement| {
                    if announcement.api_announcement.nonce > existing.api_announcement.nonce {
                        *existing = announcement.clone();
                    }
                })
                .or_insert(announcement);
        }
    }

    let (updated, changed) = verify_and_merge_announcements(
        fed_id,
        config.broadcast_public_keys.as_ref(),
        &BTreeMap::new(),
        &merged_incoming,
    );

    assert!(changed);
    assert_eq!(updated.get(&p0).unwrap().url, url0_v2);
    assert_eq!(updated.get(&p0).unwrap().nonce, 2);
    assert_eq!(updated.get(&p1).unwrap().url, url1_v5);
    assert_eq!(updated.get(&p1).unwrap().nonce, 5);
}
