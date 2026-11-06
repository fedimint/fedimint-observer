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
    let fed = std::env::var("FMO_LNV2_LIVE_FEDERATION")?.parse()?;
    let endpoint = std::env::var("FMO_LNV2_LIVE_GATEWAY")?.parse()?;
    let info = bounded_iroh::routing_info(&endpoint, &fed).await?;
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
    let oversized = format!("https://large.example/{}", "x".repeat(8192));
    let (bounded, complete) =
        gateways::bounded_registrations(vec![(0, vec![url.to_owned(), oversized])]);
    assert_eq!(complete, 0);
    for endpoint in bounded.keys() {
        tx.execute("INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians) VALUES($1,0,$2,'[1,2]'::jsonb)",&[&fed,&endpoint]).await?;
    }
    let info = serde_json::json!({"lightning_alias":"Test node","module_public_key":"key-one"});
    probes::save(
        &tx,
        &fed,
        0,
        url,
        probes::Observation::Reachable {
            latency: 40,
            routing: Some(info.clone()),
        },
    )
    .await?;
    probes::save(&tx, &fed, 0, url, probes::Observation::Unreachable).await?;
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
        probes::Observation::Reachable {
            latency: 60,
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
        probes::Observation::Reachable {
            latency: 1,
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

#[test]
fn oversized_gateway_identity_keeps_valid_entries_and_incomplete_coverage() {
    let oversized = format!("https://gateway.example/{}", "abcdef0123456789".repeat(400));
    let _: fedimint_core::util::SafeUrl = oversized.parse().expect("valid URL");
    let normal = "https://normal.example/".to_owned();
    let (registrations, complete) = gateways::bounded_registrations(vec![
        (0, vec![oversized, normal.clone()]),
        (1, vec![normal.clone()]),
    ]);
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[&normal], vec![0, 1]);
    assert_eq!(complete, 1);
    assert!(!gateways::complete_registry(complete, 2));
}

// Use real local QUIC streams, rather than checking a constant after allocating
// a complete malicious response in the client.
#[tokio::test]
async fn iroh_read_limit_and_distinct_target_cleanup() -> anyhow::Result<()> {
    use std::time::Duration;

    use fedimint_core::module::FEDIMINT_GATEWAY_ALPN;
    for oversized in [false, true] {
        for _ in 0..4 {
            let server = iroh::Endpoint::builder()
                .relay_mode(iroh::RelayMode::Disabled)
                .alpns(vec![FEDIMINT_GATEWAY_ALPN.to_vec()])
                .bind()
                .await?;
            let client = iroh::Endpoint::builder()
                .relay_mode(iroh::RelayMode::Disabled)
                .bind()
                .await?;
            let target = server.node_addr().await?;
            let server_task = tokio::spawn({
                let server = server.clone();
                async move {
                    let connection = server.accept().await.context("missing connection")?.await?;
                    let (mut send, mut recv) = connection.accept_bi().await?;
                    recv.read_to_end(4096).await?;
                    let response = if oversized {
                        vec![b'x'; bounded_iroh::MAX_RESPONSE_BYTES + 1]
                    } else {
                        br#"{"status":200,"body":null}"#.to_vec()
                    };
                    // A capped reader can stop the sender before all bytes arrive.
                    let _ = send.write_all(&response).await;
                    let _ = send.finish();
                    tokio::time::timeout(Duration::from_secs(5), connection.closed()).await?;
                    anyhow::Ok(())
                }
            });
            let response = tokio::time::timeout(
                Duration::from_secs(5),
                bounded_iroh::request_stable(&client, target, b"{}"),
            )
            .await?;
            if oversized {
                assert!(response.is_err());
                assert!(format!("{:#}", response.unwrap_err())
                    .to_lowercase()
                    .contains("long"));
            } else {
                assert!(response?.is_none());
            }
            // Each target has a distinct identity. No pool or live endpoint
            // survives the request, including oversized responses.
            server_task.await??;
            client.close().await;
            assert!(client.is_closed());
            server.close().await;
        }
    }
    Ok(())
}

#[tokio::test]
async fn iroh_next_read_limit_closes_connection() -> anyhow::Result<()> {
    use std::time::Duration;

    use fedimint_core::module::FEDIMINT_GATEWAY_ALPN;
    use iroh_next::endpoint::presets::Minimal;
    let server = iroh_next::Endpoint::builder(Minimal)
        .relay_mode(iroh_next::RelayMode::Disabled)
        .alpns(vec![FEDIMINT_GATEWAY_ALPN.to_vec()])
        .bind()
        .await?;
    let client = iroh_next::Endpoint::builder(Minimal)
        .relay_mode(iroh_next::RelayMode::Disabled)
        .bind()
        .await?;
    let target = server.addr();
    let server_task = tokio::spawn({
        let server = server.clone();
        async move {
            let connection = server.accept().await.context("missing connection")?.await?;
            let (mut send, mut recv) = connection.accept_bi().await?;
            recv.read_to_end(4096).await?;
            let _ = send
                .write_all(&vec![b'x'; bounded_iroh::MAX_RESPONSE_BYTES + 1])
                .await;
            let _ = send.finish();
            tokio::time::timeout(Duration::from_secs(5), connection.closed()).await?;
            anyhow::Ok(())
        }
    });
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        bounded_iroh::request_next(&client, target, b"{}"),
    )
    .await?;
    assert!(response.is_err());
    assert!(format!("{:#}", response.unwrap_err())
        .to_lowercase()
        .contains("long"));
    server_task.await??;
    client.close().await;
    assert!(client.is_closed());
    server.close().await;
    Ok(())
}
