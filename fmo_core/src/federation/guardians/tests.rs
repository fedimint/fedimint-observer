use super::*;

fn discovery_keypair() -> fedimint_core::secp256k1::Keypair {
    use fedimint_core::secp256k1::{Keypair, SecretKey, SECP256K1};

    let secret = SecretKey::from_slice(&[1; 32]).expect("valid test key");
    Keypair::from_secret_key(SECP256K1, &secret)
}

fn discovery_config(num_peers: u16) -> fedimint_core::config::GlobalClientConfig {
    use fedimint_core::config::PeerUrl;
    use fedimint_core::module::CoreConsensusVersion;
    use fedimint_core::secp256k1::PublicKey;

    let public_key = PublicKey::from_keypair(&discovery_keypair());

    fedimint_core::config::GlobalClientConfig {
        api_endpoints: (0..num_peers)
            .map(|index| {
                let peer_id = PeerId::from(index);
                let url = SafeUrl::parse(&format!("wss://guardian-{index}.example.com"))
                    .expect("test URL is valid");
                (
                    peer_id,
                    PeerUrl {
                        url,
                        name: format!("guardian-{index}"),
                    },
                )
            })
            .collect(),
        broadcast_public_keys: Some(
            (0..num_peers)
                .map(|index| (PeerId::from(index), public_key))
                .collect(),
        ),
        consensus_version: CoreConsensusVersion::new(0, 0),
        meta: BTreeMap::new(),
    }
}

fn discovery_announcement(peer_id: PeerId) -> SignedApiAnnouncement {
    use fedimint_core::net::api_announcement::ApiAnnouncement;
    use fedimint_core::secp256k1::SECP256K1;

    ApiAnnouncement::new(
        SafeUrl::parse(&format!("wss://announced-{peer_id}.example.com")).unwrap(),
        1,
    )
    .sign(SECP256K1, &discovery_keypair())
}

#[test]
fn rotated_guardian_discovery_keeps_canonical_recovery_route() {
    let config = discovery_config(2);
    let peer_ids = config.api_endpoints.keys().copied().collect::<Vec<_>>();
    let mut active = config
        .api_endpoints
        .iter()
        .map(|(&peer_id, endpoint)| (peer_id, endpoint.url.clone()))
        .collect::<BTreeMap<_, _>>();
    active.insert(
        PeerId::from(0),
        SafeUrl::parse("wss://rotated.example.com").unwrap(),
    );

    let window = discovery_targets(&peer_ids, &active, &config, 0, 64);
    assert!(window.targets.contains(&DiscoveryTarget {
        peer_id: PeerId::from(0),
        route: DiscoveryRoute::Active,
    }));
    assert!(window.targets.contains(&DiscoveryTarget {
        peer_id: PeerId::from(0),
        route: DiscoveryRoute::Canonical,
    }));
    assert_eq!(
        window
            .targets
            .iter()
            .filter(|target| target.peer_id == PeerId::from(1))
            .count(),
        1,
        "an unchanged route must not be queried twice"
    );
}

#[test]
fn discovery_budget_rotates_instead_of_disabling_large_target_sets() {
    let config = discovery_config(40);
    let peer_ids = config.api_endpoints.keys().copied().collect::<Vec<_>>();
    let active = peer_ids
        .iter()
        .map(|&peer_id| {
            (
                peer_id,
                SafeUrl::parse(&format!("wss://rotated-{peer_id}.example.com")).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();

    let first = discovery_targets(&peer_ids, &active, &config, 0, 64);
    // Only four finished, so the next round starts after them.
    let second = discovery_targets(&peer_ids, &active, &config, first.next_start(4), 64);
    let late_target = DiscoveryTarget {
        peer_id: PeerId::from(33),
        route: DiscoveryRoute::Active,
    };
    assert!(!first.targets.contains(&late_target));
    assert!(
        second.targets.contains(&late_target),
        "the next round must reach targets beyond the first round budget"
    );
}

#[test]
fn announcement_admission_size_is_checked_without_a_copy() {
    assert!(json_fits_admission_limit(
        &serde_json::json!({"route": "ok"}),
        32
    ));
    assert!(!json_fits_admission_limit(
        &serde_json::json!({"padding": "x".repeat(128)}),
        64
    ));
}

#[test]
fn unknown_announcements_cannot_suppress_a_known_rotation() {
    let config = discovery_config(1);
    let known_peer = PeerId::from(0);
    let mut announcements = BTreeMap::from([(known_peer, discovery_announcement(known_peer))]);
    for index in 1..=80 {
        let peer_id = PeerId::from(index);
        announcements.insert(peer_id, discovery_announcement(peer_id));
    }

    let admitted =
        admit_announcements(serde_json::to_value(announcements).unwrap(), &config).unwrap();
    assert_eq!(
        admitted.rejected, 0,
        "unknown peers are ignored, not counted"
    );
    assert_eq!(
        admitted.announcements.keys().copied().collect::<Vec<_>>(),
        vec![known_peer]
    );
}

#[test]
fn one_oversized_announcement_cannot_block_other_routes() {
    use fedimint_core::net::api_announcement::ApiAnnouncement;
    use fedimint_core::secp256k1::SECP256K1;

    let config = discovery_config(3);
    let oversized = ApiAnnouncement::new(
        SafeUrl::parse(&format!("wss://g1.example.com/{}", "a".repeat(8 * 1024))).unwrap(),
        2,
    )
    .sign(SECP256K1, &discovery_keypair());
    let reply = serde_json::json!({
        "0": discovery_announcement(PeerId::from(0)),
        "1": oversized,
        "2": {"not": "an announcement"},
    });

    let admitted = admit_announcements(reply, &config).unwrap();
    assert_eq!(admitted.rejected, 2);
    assert_eq!(
        admitted.announcements.keys().copied().collect::<Vec<_>>(),
        vec![PeerId::from(0)],
        "the valid member's rotation must still be admitted"
    );
    assert!(admit_announcements(serde_json::json!([1, 2]), &config).is_none());
}

#[test]
fn large_federation_rotates_bounded_announcement_windows() {
    let announcements = (0..70)
        .map(|index| {
            let peer_id = PeerId::from(index);
            (peer_id, discovery_announcement(peer_id))
        })
        .collect::<BTreeMap<_, _>>();

    let first = bounded_announcement_window(announcements.clone(), 0, 64);
    let second = bounded_announcement_window(announcements, first.next_start, 64);
    assert_eq!(first.announcements.len(), 64);
    assert_eq!(second.announcements.len(), 64);
    let covered = first
        .announcements
        .keys()
        .chain(second.announcements.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(covered.len(), 70, "both windows must cover every member");
}

#[test]
fn health_polls_back_off_only_while_offline() {
    let minutes = |interval: Duration| interval.as_secs() / 60;

    assert_eq!(minutes(health_poll_interval(None)), 1);
    // Online again within the current hour, see `health_offline_since`
    assert_eq!(
        minutes(health_poll_interval(Some(TimeDelta::minutes(-20)))),
        1
    );
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::hours(23)))), 1);
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::days(1)))), 10);
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::days(6)))), 10);
    assert_eq!(minutes(health_poll_interval(Some(TimeDelta::days(7)))), 30);
    assert_eq!(
        minutes(health_poll_interval(Some(TimeDelta::days(200)))),
        30
    );
}

#[test]
fn block_heights_from_either_wallet() {
    let v1 = BlockHeightSource::WalletLocal(0);
    assert_eq!(v1.block_height(serde_json::json!(850_001)), Some(850_000));
    assert_eq!(v1.block_height(serde_json::json!(0)), Some(0));
    assert_eq!(v1.block_height(serde_json::Value::Null), None);
    assert_eq!(v1.block_height(serde_json::json!("garbage")), None);

    // Wallet v2 reports the consensus block count, 6 blocks behind the tip
    let v2 = BlockHeightSource::WalletV2Consensus(1);
    assert_eq!(v2.block_height(serde_json::json!(849_995)), Some(850_000));
    assert_eq!(v2.block_height(serde_json::json!(0)), None);
    assert_eq!(v2.block_height(serde_json::json!(u64::MAX)), None);
    assert_eq!(v2.endpoint(), "consensus_block_count");
    assert_eq!(v2.instance(), 1);
}

#[test]
fn reported_session_counts_are_clamped() {
    assert_eq!(clamp_to_u32(5), 5);
    assert_eq!(clamp_to_u32(-1), 0);
    assert_eq!(clamp_to_u32(i64::from(i32::MAX) + 1), 2_147_483_648);
    assert_eq!(clamp_to_u32(i64::MAX), u32::MAX);
}

#[test]
fn test_migrated_guardian_endpoint_resolution() {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use fedimint_core::config::{GlobalClientConfig, PeerUrl};
    use fedimint_core::net::api_announcement::ApiAnnouncement;
    use fedimint_core::secp256k1::{Keypair, PublicKey, SecretKey, SECP256K1};
    use fedimint_core::util::SafeUrl;
    use fedimint_core::PeerId;

    use crate::federation::endpoints::{
        active_api_peers, verify_and_merge_announcements, GuardianEndpointsMap,
    };

    let peer1 = PeerId::from(1);
    let mut key_bytes = [0u8; 32];
    key_bytes[0] = 1;
    let sk = SecretKey::from_slice(&key_bytes).unwrap();
    let keypair = Keypair::from_secret_key(SECP256K1, &sk);
    let pk = PublicKey::from_keypair(&keypair);

    let old_url = SafeUrl::from_str("ws://api.bitcoinprinciples.xyz:8173").unwrap();
    let new_url = SafeUrl::from_str("ws://api.fmo.org:8173").unwrap();

    let mut api_endpoints = BTreeMap::new();
    api_endpoints.insert(
        peer1,
        PeerUrl {
            url: old_url.clone(),
            name: "peer1".into(),
        },
    );
    let mut broadcast_public_keys = BTreeMap::new();
    broadcast_public_keys.insert(peer1, pk);

    let global = GlobalClientConfig {
        api_endpoints,
        broadcast_public_keys: Some(broadcast_public_keys),
        consensus_version: fedimint_core::module::CoreConsensusVersion::new(0, 0),
        meta: BTreeMap::new(),
    };
    let fed_id = global.calculate_federation_id();

    // 1. Initial health check uses config URL
    let peers_initial = active_api_peers(&global, None);
    assert_eq!(peers_initial.get(&peer1).unwrap(), &old_url);

    // 2. Peer 1 announces ws://api.fmo.org with nonce 1
    let announcement = ApiAnnouncement::new(new_url.clone(), 1).sign(SECP256K1, &keypair);
    let mut incoming = BTreeMap::new();
    incoming.insert(peer1, announcement);

    let existing = GuardianEndpointsMap::new();
    let (updated, changed) = verify_and_merge_announcements(fed_id, &global, &existing, &incoming);
    assert!(changed);

    // 3. Health check now resolves to new URL
    let peers_after_migration = active_api_peers(&global, Some(&updated));
    assert_eq!(peers_after_migration.get(&peer1).unwrap(), &new_url);

    // 4. Stale nonce 1 is rejected
    let stale_announcement = ApiAnnouncement::new(old_url.clone(), 1).sign(SECP256K1, &keypair);
    let mut incoming_stale = BTreeMap::new();
    incoming_stale.insert(peer1, stale_announcement);
    let (updated_after_stale, changed_stale) =
        verify_and_merge_announcements(fed_id, &global, &updated, &incoming_stale);
    assert!(!changed_stale);
    assert_eq!(updated_after_stale.get(&peer1).unwrap().url(), &new_url);

    // 5. Forged signature is rejected
    let mut fake_key_bytes = [0u8; 32];
    fake_key_bytes[0] = 99;
    let fake_sk = SecretKey::from_slice(&fake_key_bytes).unwrap();
    let fake_keypair = Keypair::from_secret_key(SECP256K1, &fake_sk);
    let forged_announcement = ApiAnnouncement::new(old_url, 2).sign(SECP256K1, &fake_keypair);
    let mut incoming_forged = BTreeMap::new();
    incoming_forged.insert(peer1, forged_announcement);
    let (updated_after_forge, changed_forge) =
        verify_and_merge_announcements(fed_id, &global, &updated, &incoming_forged);
    assert!(!changed_forge);
    assert_eq!(updated_after_forge.get(&peer1).unwrap().url(), &new_url);
}
