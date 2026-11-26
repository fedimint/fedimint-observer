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
fn short_replies_cannot_reset_another_sources_progress() {
    let full = (0..70)
        .map(|index| {
            let peer_id = PeerId::from(index);
            (peer_id, discovery_announcement(peer_id))
        })
        .collect::<BTreeMap<_, _>>();
    let short = full.iter().take(3).map(|(&k, v)| (k, v.clone())).collect();
    let source = |peer| DiscoveryTarget {
        peer_id: PeerId::from(peer),
        route: DiscoveryRoute::Canonical,
    };

    let mut cursors = AnnouncementCursors::default();
    let first = cursors.window(source(0), full.clone());
    cursors.window(source(1), short);
    let second = cursors.window(source(0), full);
    let covered = first
        .announcements
        .keys()
        .chain(second.announcements.keys())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        covered.len(),
        70,
        "the full source must reach peers past its first window"
    );
}
