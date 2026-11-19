use super::*;

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
    let (updated, changed) = verify_and_merge_announcements(
        fed_id,
        global.broadcast_public_keys.as_ref(),
        &existing,
        &incoming,
    );
    assert!(changed);

    // 3. Health check now resolves to new URL
    let peers_after_migration = active_api_peers(&global, Some(&updated));
    assert_eq!(peers_after_migration.get(&peer1).unwrap(), &new_url);

    // 4. Stale nonce 1 is rejected
    let stale_announcement = ApiAnnouncement::new(old_url.clone(), 1).sign(SECP256K1, &keypair);
    let mut incoming_stale = BTreeMap::new();
    incoming_stale.insert(peer1, stale_announcement);
    let (updated_after_stale, changed_stale) = verify_and_merge_announcements(
        fed_id,
        global.broadcast_public_keys.as_ref(),
        &updated,
        &incoming_stale,
    );
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
    let (updated_after_forge, changed_forge) = verify_and_merge_announcements(
        fed_id,
        global.broadcast_public_keys.as_ref(),
        &updated,
        &incoming_forged,
    );
    assert!(!changed_forge);
    assert_eq!(updated_after_forge.get(&peer1).unwrap().url(), &new_url);
}
