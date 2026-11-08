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
