use fedimint_core::core::IntoDynInstance;

use super::*;

#[test]
fn lightning_time_vote_keeps_its_session_timestamp_behavior() {
    let module = LnV2Observer;
    let time = LightningConsensusItem::UnixTimeVote(1_700_000_000).into_dyn(0);
    let height = LightningConsensusItem::BlockCountVote(850_000).into_dyn(0);
    assert_eq!(module.unix_time_vote(&time), Some(1_700_000_000));
    assert_eq!(module.unix_time_vote(&height), None);
    assert!(module.summarize_ci(&time).is_none());
    assert!(module.summarize_ci(&height).is_none());
}

#[test]
fn registries_merge_urls_without_inventing_peer_agreement() {
    let registrations = gateways::merge_registrations(vec![
        (
            2,
            vec!["https://a.example/".into(), "https://a.example/".into()],
        ),
        (
            0,
            vec!["https://a.example/".into(), "https://b.example/".into()],
        ),
    ]);
    assert_eq!(registrations["https://a.example/"], vec![0, 2]);
    assert_eq!(registrations["https://b.example/"], vec![0]);
    assert_eq!(gateways::merge_registrations(vec![]).len(), 0);
}

#[test]
fn partial_registry_polls_do_not_remove_existing_gateways() {
    assert!(!gateways::complete_registry(7, 10));
    assert!(!gateways::complete_registry(0, 0));
    assert!(gateways::complete_registry(10, 10));
}

#[tokio::test]
#[ignore = "read-only smoke test requires explicit public federation and gateway environment variables"]
async fn public_gateway_routing_info() -> anyhow::Result<()> {
    use fedimint_lnv2_common::gateway_api::{GatewayConnection, RealGatewayConnection};
    let fed = std::env::var("FMO_LNV2_LIVE_FEDERATION")?.parse()?;
    let endpoint = std::env::var("FMO_LNV2_LIVE_GATEWAY")?.parse()?;
    let connectors = fedimint_connectors::ConnectorRegistry::build_from_client_env()?
        .bind()
        .await?;
    let connection = RealGatewayConnection {
        api: fedimint_ln_common::client::GatewayApi::new(None, connectors),
    };
    let info = tokio::time::timeout(
        std::time::Duration::from_secs(40),
        connection.routing_info(endpoint, &fed),
    )
    .await?
    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    anyhow::ensure!(
        info.is_some(),
        "Gateway does not report routing support for this federation"
    );
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

#[test]
fn gateway_status_uses_fresh_probes_not_registration_alone() {
    let now = chrono::Utc::now();
    assert_eq!(probes::status(true, None, None, None), "unknown");
    assert_eq!(
        probes::status(true, Some(now), Some(true), Some(true)),
        "online"
    );
    assert_eq!(
        probes::status(true, Some(now), Some(true), Some(false)),
        "degraded"
    );
    assert_eq!(
        probes::status(true, Some(now), Some(false), None),
        "offline"
    );
    assert_eq!(
        probes::status(
            true,
            Some(now - chrono::Duration::minutes(16)),
            Some(true),
            Some(true)
        ),
        "unknown"
    );
    assert_eq!(
        probes::status(false, Some(now), Some(true), Some(true)),
        "removed"
    );
}

#[test]
fn gateway_windows_and_outgoing_outcomes_are_explicit() {
    use fedimint_lnv2_common::OutgoingWitness;
    assert_eq!(gateways::window_seconds("24h"), Some(86400));
    assert_eq!(gateways::window_seconds("90d"), Some(90 * 86400));
    assert_eq!(gateways::window_seconds("all"), None);
    assert_eq!(
        activity::outgoing_outcome(&OutgoingWitness::Claim([0; 32])),
        "claimed"
    );
    assert_eq!(
        activity::outgoing_outcome(&OutgoingWitness::Refund),
        "refunded"
    );
}

#[tokio::test]
#[ignore = "requires an empty isolated FMO_LNV2_TEST_DATABASE"]
async fn postgres_probe_history_and_activity() -> anyhow::Result<()> {
    let database = std::env::var("FMO_LNV2_TEST_DATABASE")?;
    let pool = deadpool_postgres::Config {
        url: Some(database),
        ..Default::default()
    }
    .create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?;
    let mut connection = pool.get().await?;
    let tx = connection.transaction().await?;
    tx.batch_execute("CREATE SCHEMA fmo_lnv2;
        CREATE TABLE federations(federation_id BYTEA PRIMARY KEY);
        CREATE TABLE module_cursors(module_kind TEXT PRIMARY KEY,next_session INTEGER);
        INSERT INTO module_cursors VALUES('lnv2',10);
        CREATE TABLE session_times(federation_id BYTEA,session_index INTEGER,estimated_session_timestamp TIMESTAMP);").await?;
    tx.batch_execute(include_str!("../schema/v0.sql")).await?;
    tx.batch_execute(include_str!("../schema/v1.sql")).await?;
    assert_eq!(
        tx.query_one("SELECT next_session FROM module_cursors", &[])
            .await?
            .get::<_, i32>(0),
        0
    );
    let fed = vec![4u8; 32];
    let url = "iroh://public-gateway";
    tx.execute("INSERT INTO federations VALUES($1)", &[&fed])
        .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.gateway_polls VALUES($1,0,now(),9,10)",
        &[&fed],
    )
    .await?;
    tx.execute("INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians) VALUES($1,0,$2,'[1,2]'::jsonb)",&[&fed,&url]).await?;
    let info = serde_json::json!({"lightning_alias":"Test node","module_public_key":"key-one"});
    probes::save(
        &tx,
        &fed,
        0,
        url,
        probes::Observation {
            reachable: true,
            supported: Some(true),
            latency: Some(40),
            routing: Some(info.clone()),
        },
    )
    .await?;
    probes::save(
        &tx,
        &fed,
        0,
        url,
        probes::Observation {
            reachable: false,
            supported: None,
            latency: None,
            routing: None,
        },
    )
    .await?;
    let state = tx
        .query_one("SELECT routing_info,reachable FROM fmo_lnv2.gateways", &[])
        .await?;
    assert_eq!(state.get::<_, serde_json::Value>(0), info);
    assert!(!state.get::<_, bool>(1));
    let health=tx.query_one("SELECT samples,online_samples,latency_sum_ms,latency_samples FROM fmo_lnv2.gateway_health_hourly",&[]).await?;
    assert_eq!(
        (
            health.get::<_, i64>(0),
            health.get::<_, i64>(1),
            health.get::<_, i64>(2),
            health.get::<_, i64>(3)
        ),
        (2, 1, 40, 1)
    );
    tx.execute(
        "INSERT INTO session_times VALUES($1,7,now() AT TIME ZONE 'UTC')",
        &[&fed],
    )
    .await?;
    let funding = vec![5u8; 32];
    tx.execute(
        "INSERT INTO fmo_lnv2.contract_activity VALUES($1,0,$2,0,7,'key-one','outgoing',100000)",
        &[&fed, &funding],
    )
    .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.contract_activity VALUES($1,0,$2,1,7,'key-one','incoming',200000)",
        &[&fed, &funding],
    )
    .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.contract_resolutions VALUES($1,0,$2,0,8,'claimed')",
        &[&fed, &funding],
    )
    .await?;
    // Key rotation must not lose activity attributable to an earlier gateway key.
    probes::save(
        &tx,
        &fed,
        0,
        url,
        probes::Observation {
            reachable: true,
            supported: Some(true),
            latency: Some(60),
            routing: Some(serde_json::json!({"module_public_key":"key-two"})),
        },
    )
    .await?;
    let since = chrono::Utc::now() - chrono::Duration::days(7);
    let rows = tx
        .query(
            gateways::REGISTRY_SQL,
            &[
                &fed,
                &0i32,
                &None::<String>,
                &100i64,
                &since,
                &(7.0 * 86400.0 / 30.0),
            ],
        )
        .await?;
    assert_eq!(rows.len(), 1);
    let buckets: serde_json::Value = rows[0].get(11);
    assert_eq!(buckets[0]["samples"], 3);
    let activity: serde_json::Value = rows[0].get(12);
    assert_eq!(activity["incoming_contracts"], 1);
    assert_eq!(activity["outgoing_contracts"], 1);
    assert_eq!(activity["outgoing_claimed"], 1);
    assert_eq!(activity["incoming_msat"], 200000);
    tx.execute("UPDATE fmo_lnv2.gateways SET registered=false", &[])
        .await?;
    probes::save(
        &tx,
        &fed,
        0,
        url,
        probes::Observation {
            reachable: true,
            supported: Some(true),
            latency: Some(1),
            routing: None,
        },
    )
    .await?;
    assert_eq!(
        tx.query_one("SELECT samples FROM fmo_lnv2.gateway_health_hourly", &[])
            .await?
            .get::<_, i64>(0),
        3
    );
    tx.rollback().await?;
    Ok(())
}

#[test]
fn public_https_probes_reject_private_and_special_use_addresses() {
    for address in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "100.64.0.1",
        "192.168.0.1",
        "0.0.0.0",
        "255.255.255.255",
        "::1",
        "::ffff:127.0.0.1",
        "fd00::1",
        "fe80::1",
        "2001:db8::1",
    ] {
        assert!(
            !public_http::public_ip(address.parse().unwrap()),
            "{address}"
        );
    }
    assert!(public_http::public_ip("1.1.1.1".parse().unwrap()));
    assert!(public_http::public_ip(
        "2606:4700:4700::1111".parse().unwrap()
    ));
}
