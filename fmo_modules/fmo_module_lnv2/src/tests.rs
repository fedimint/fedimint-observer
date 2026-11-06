use fedimint_core::core::IntoDynInstance;

use super::*;

#[tokio::test]
async fn early_http_close_is_unreachable() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        drop(socket);
    });
    let error = reqwest::Client::new()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(
        public_http::classify_send_error(error),
        probes::ProbeError::Unreachable(_)
    ));
    server.await.unwrap();
}

#[tokio::test]
async fn malformed_http_response_is_bad_response() {
    use tokio::io::AsyncWriteExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.write_all(b"HTTP/1.1 xyz\r\n\r\n").await.unwrap();
    });
    let error = reqwest::Client::new()
        .get(format!("http://{address}/"))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(
        public_http::classify_send_error(error),
        probes::ProbeError::BadResponse(_)
    ));
    server.await.unwrap();
}

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
    let (info, _latency) = bounded_iroh::routing_info(&endpoint, &fed).await?;
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
    // Every window covers exactly its length in whole, hour-aligned buckets.
    for (window, hours) in [
        ("24h", 24),
        ("7d", 7 * 24),
        ("30d", 30 * 24),
        ("90d", 90 * 24),
    ] {
        let (count, width) = gateways::window_buckets(window).expect("known window");
        assert_eq!(i64::from(count) * width, hours, "{window}");
    }
    assert_eq!(gateways::window_buckets("all"), None);
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-06T10:30:15Z")
        .unwrap()
        .to_utc();
    let start = gateways::window_start(now, 24, 1);
    assert_eq!(start.to_rfc3339(), "2026-11-05T11:00:00+00:00");
    // The last bucket holds the current hour.
    assert_eq!(
        start + chrono::Duration::hours(23),
        now - chrono::Duration::seconds(30 * 60 + 15)
    );
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
    tx.batch_execute(include_str!("../schema/v2.sql")).await?;
    assert_eq!(
        tx.query_one("SELECT next_session FROM module_cursors", &[])
            .await?
            .get::<_, i32>(0),
        0
    );
    let fed = vec![4u8; 32];
    let url = "iroh://public-gateway";
    // No registered gateways or health rows: the wait stays NULL, so the probe
    // worker awaits a notification instead of polling.
    let next_probe = tx
        .query_one(probes::NEXT_PROBE_SQL, &[&fed, &0i32])
        .await?
        .get::<_, Option<f64>>(0);
    assert_eq!(next_probe, None);
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
    // An unprobed registered gateway is due now.
    let next_probe = tx
        .query_one(probes::NEXT_PROBE_SQL, &[&fed, &0i32])
        .await?
        .get::<_, Option<f64>>(0)
        .expect("a gateway is registered");
    assert_eq!(
        fmo_core::module::Services::wait_for_deadline(next_probe),
        std::time::Duration::ZERO
    );
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
    let since = gateways::window_start(chrono::Utc::now(), 28, 6);
    let rows = tx
        .query(
            gateways::REGISTRY_SQL,
            &[
                &fed,
                &0i32,
                &None::<String>,
                &100i64,
                &since,
                &(6.0 * 3600.0),
                &28i32,
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
    assert_eq!(rows[0].get::<_, i64>(13), 2);
    assert_eq!(rows[0].get::<_, i64>(14), 0);

    // Keys are self-reported: another registered endpoint claiming key-one
    // contests it, and its contracts are credited to neither endpoint.
    let other = "https://other.example/";
    tx.execute("INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians) VALUES($1,0,$2,'[0]'::jsonb)",&[&fed,&other]).await?;
    probes::save(
        &tx,
        &fed,
        0,
        other,
        probes::Observation::Reachable {
            latency: 5,
            routing: Some(serde_json::json!({"module_public_key":"key-one"})),
        },
    )
    .await?;
    async fn overview(
        tx: &deadpool_postgres::Transaction<'_>,
        fed: &[u8],
        since: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<tokio_postgres::Row>, tokio_postgres::Error> {
        tx.query(
            gateways::REGISTRY_SQL,
            &[
                &fed,
                &0i32,
                &None::<String>,
                &100i64,
                &since,
                &(6.0 * 3600.0),
                &28i32,
            ],
        )
        .await
    }
    let rows = overview(&tx, &fed, since).await?;
    for row in &rows {
        let activity: serde_json::Value = row.get(12);
        assert_eq!(
            activity["incoming_contracts"],
            0,
            "{}",
            row.get::<_, String>(0)
        );
        assert_eq!(activity["outgoing_contracts"], 0);
        assert_eq!(row.get::<_, i64>(14), 1);
        assert_eq!(gateways::attribution(row.get(13), row.get(14)), "contested");
    }
    // A retired endpoint no longer contests the key.
    tx.execute(
        "UPDATE fmo_lnv2.gateways SET registered=false WHERE api_endpoint=$1",
        &[&other],
    )
    .await?;
    let rows = overview(&tx, &fed, since).await?;
    let row = rows
        .iter()
        .find(|row| row.get::<_, String>(0) == url)
        .expect("gateway");
    let activity: serde_json::Value = row.get(12);
    assert_eq!(activity["incoming_contracts"], 1);
    assert_eq!(
        gateways::attribution(row.get(13), row.get(14)),
        "self_reported"
    );

    // An endpoint keeps only its most recently claimed keys; key-one, the
    // least recently claimed, drops off and its contracts are no longer credited.
    for index in 0..probes::MAX_KEYS_PER_ENDPOINT {
        probes::save(
            &tx,
            &fed,
            0,
            url,
            probes::Observation::Reachable {
                latency: 5,
                routing: Some(serde_json::json!({"module_public_key":format!("rotated-{index}")})),
            },
        )
        .await?;
    }
    let keys: Vec<String> = tx
        .query("SELECT module_public_key FROM fmo_lnv2.gateway_keys WHERE api_endpoint=$1 ORDER BY module_public_key",&[&url])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(keys.len() as i64, probes::MAX_KEYS_PER_ENDPOINT);
    assert!(keys.iter().all(|key| key.starts_with("rotated-")));
    tx.execute(
        "DELETE FROM fmo_lnv2.gateways WHERE api_endpoint=$1",
        &[&other],
    )
    .await?;
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
        3 + probes::MAX_KEYS_PER_ENDPOINT
    );

    // Admission: at capacity, a new endpoint announced by more guardians
    // displaces one announced by fewer that is absent from this poll; one
    // announced by no more guardians than any registered endpoint is skipped.
    tx.execute("DELETE FROM fmo_lnv2.gateways", &[]).await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians)
         SELECT $1,0,'https://junk'||n||'.example/','[0]'::jsonb FROM generate_series(1,$2::bigint) n",
        &[&fed, &gateways::MAX_ACTIVE_GATEWAYS],
    )
    .await?;
    let agreed = "https://agreed.example/".to_owned();
    let kept = "https://junk1.example/".to_owned();
    let registrations =
        std::collections::BTreeMap::from([(agreed.clone(), vec![0, 1]), (kept.clone(), vec![0])]);
    let announced = vec![agreed.clone(), kept.clone()];
    assert_eq!(
        gateways::admit(&tx, &fed, 0, &announced, registrations).await?,
        0
    );
    async fn active(tx: &deadpool_postgres::Transaction<'_>) -> Result<i64, tokio_postgres::Error> {
        tx.query_one(
            "SELECT count(*) FROM fmo_lnv2.gateways WHERE registered",
            &[],
        )
        .await
        .map(|row| row.get(0))
    }
    async fn registered(
        tx: &deadpool_postgres::Transaction<'_>,
        endpoint: &str,
    ) -> Result<bool, tokio_postgres::Error> {
        tx.query_one(
            "SELECT registered FROM fmo_lnv2.gateways WHERE api_endpoint=$1",
            &[&endpoint],
        )
        .await
        .map(|row| row.get(0))
    }
    assert_eq!(active(&tx).await?, gateways::MAX_ACTIVE_GATEWAYS);
    assert!(registered(&tx, &agreed).await?);
    assert!(registered(&tx, &kept).await?);
    let flood = "https://flood.example/".to_owned();
    let registrations = std::collections::BTreeMap::from([(flood.clone(), vec![0])]);
    assert_eq!(
        gateways::admit(&tx, &fed, 0, std::slice::from_ref(&flood), registrations).await?,
        1
    );
    assert_eq!(active(&tx).await?, gateways::MAX_ACTIVE_GATEWAYS);
    assert_eq!(
        tx.query_one(
            "SELECT count(*) FROM fmo_lnv2.gateways WHERE api_endpoint=$1",
            &[&flood]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    // Retired history is capped by count as well as age.
    assert_eq!(
        tx.execute(gateways::PRUNE_RETIRED_SQL, &[&fed, &0i32, &0i64])
            .await?,
        1
    );

    // A valid routing reply can contain NUL. Reproduce PostgreSQL's rejection,
    // then exercise the production sanitizer, probe write and API read query.
    let key = "0242fb0fe09fcc78a9d25de65837145da2d7828d8789b14f5f53f685c19ae5e0ef";
    let fee = serde_json::json!({"base":1000,"parts_per_million":100});
    let info: fedimint_lnv2_common::gateway_api::RoutingInfo =
        serde_json::from_value(serde_json::json!({
            "lightning_public_key":key,"module_public_key":key,
            "lightning_alias":format!("node\u{0}\n{}", "é".repeat(256)),
            "send_fee_minimum":fee,"send_fee_default":fee,"receive_fee":fee,
            "expiration_delta_minimum":10,"expiration_delta_default":20
        }))?;
    tx.batch_execute("SAVEPOINT unsanitized_alias").await?;
    let error = tx
        .execute(
            "UPDATE fmo_lnv2.gateways SET routing_info=$3 WHERE federation_id=$1 AND api_endpoint=$2",
            &[&fed, &agreed, &serde_json::to_value(&info)?],
        )
        .await
        .expect_err("PostgreSQL must reject a NUL in JSONB");
    assert_eq!(
        error.as_db_error().expect("database rejection").code(),
        &tokio_postgres::error::SqlState::UNTRANSLATABLE_CHARACTER
    );
    tx.batch_execute(
        "ROLLBACK TO SAVEPOINT unsanitized_alias; RELEASE SAVEPOINT unsanitized_alias",
    )
    .await?;
    probes::save(
        &tx,
        &fed,
        0,
        &agreed,
        probes::Observation::Reachable {
            latency: 5,
            routing: Some(probes::bounded_routing(info)?),
        },
    )
    .await?;
    let rows = overview(&tx, &fed, since).await?;
    let gateway = rows
        .iter()
        .find(|row| row.get::<_, String>(0) == agreed)
        .expect("sanitized gateway is returned by the API query");
    let stored: serde_json::Value = gateway.get(5);
    assert_eq!(
        stored["lightning_alias"],
        format!("node{}", "é".repeat(probes::MAX_ALIAS_CHARS - 4))
    );
    assert_eq!(stored["module_public_key"], key);
    assert!(serde_json::to_vec(&stored)?.len() < 2048);
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires an empty isolated FMO_LNV2_TEST_DATABASE"]
async fn postgres_v2_migration_and_admission_trim_existing_records() -> anyhow::Result<()> {
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
        CREATE TABLE session_times(federation_id BYTEA,session_index INTEGER,estimated_session_timestamp TIMESTAMP);").await?;
    tx.batch_execute(include_str!("../schema/v0.sql")).await?;
    tx.batch_execute(include_str!("../schema/v1.sql")).await?;
    let fed = vec![6u8; 32];
    tx.execute("INSERT INTO federations VALUES($1)", &[&fed])
        .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.gateway_polls VALUES($1,0,now(),2,2)",
        &[&fed],
    )
    .await?;
    let cap = gateways::MAX_ACTIVE_GATEWAYS;
    // Records stored before the limits: an overfull active registry (cap
    // endpoints both guardians announce, six only one does), overfull
    // retired history, and an endpoint with twelve keys.
    tx.execute(
        "INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians)
         SELECT $1,0,'https://strong'||n||'.example/','[0,1]'::jsonb FROM generate_series(1,$2::bigint) n",
        &[&fed, &cap],
    )
    .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians)
         SELECT $1,0,'https://weak'||n||'.example/','[0]'::jsonb FROM generate_series(1,6) n",
        &[&fed],
    )
    .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians,registered,last_seen)
         SELECT $1,0,'https://old'||n||'.example/','[0]'::jsonb,false,now()-n*interval '1 hour'
         FROM generate_series(1,$2::bigint) n",
        &[&fed, &gateways::MAX_RETIRED_GATEWAYS],
    )
    .await?;
    let keyed = "https://strong1.example/";
    tx.execute(
        "UPDATE fmo_lnv2.gateways SET routing_info=jsonb_build_object('module_public_key','zz-current',
             'lightning_alias','a'||chr(10)||repeat('b',5000)) WHERE api_endpoint=$1",
        &[&keyed],
    )
    .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.gateway_keys SELECT $1,0,$2,'k'||lpad(n::text,2,'0') FROM generate_series(0,10) n",
        &[&fed, &keyed],
    )
    .await?;
    tx.execute(
        "INSERT INTO fmo_lnv2.gateway_keys VALUES($1,0,$2,'zz-current')",
        &[&fed, &keyed],
    )
    .await?;

    tx.batch_execute(include_str!("../schema/v2.sql")).await?;

    // An alias stored before the limit is cut and loses control characters.
    let alias: String = tx
        .query_one(
            "SELECT routing_info->>'lightning_alias' FROM fmo_lnv2.gateways WHERE api_endpoint=$1",
            &[&keyed],
        )
        .await?
        .get(0);
    assert_eq!(
        alias,
        format!("a{}", "b".repeat(probes::MAX_ALIAS_CHARS - 1))
    );
    // Keys: the current key is kept with the first others by key, as the
    // runtime prune orders ties.
    let keys: Vec<String> = tx
        .query(
            "SELECT module_public_key FROM fmo_lnv2.gateway_keys WHERE api_endpoint=$1 ORDER BY module_public_key",
            &[&keyed],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(keys.len() as i64, probes::MAX_KEYS_PER_ENDPOINT);
    assert_eq!(keys.last().map(String::as_str), Some("zz-current"));
    assert_eq!(keys[0], "k00");
    async fn count(
        tx: &deadpool_postgres::Transaction<'_>,
        sql: &str,
    ) -> Result<i64, tokio_postgres::Error> {
        tx.query_one(sql, &[]).await.map(|row| row.get(0))
    }
    // Active registry: the weakly agreed endpoints are retired.
    assert_eq!(
        count(
            &tx,
            "SELECT count(*) FROM fmo_lnv2.gateways WHERE registered"
        )
        .await?,
        cap
    );
    assert_eq!(
        count(&tx, "SELECT count(*) FROM fmo_lnv2.gateways WHERE registered AND api_endpoint LIKE 'https://weak%'").await?,
        0
    );
    // Retired history keeps the most recent, including those just retired.
    assert_eq!(
        count(
            &tx,
            "SELECT count(*) FROM fmo_lnv2.gateways WHERE NOT registered"
        )
        .await?,
        gateways::MAX_RETIRED_GATEWAYS
    );
    assert_eq!(
        count(
            &tx,
            "SELECT count(*) FROM fmo_lnv2.gateways WHERE api_endpoint LIKE 'https://weak%'"
        )
        .await?,
        6
    );

    // At runtime, admission also shrinks a registry that is already overfull.
    tx.execute(
        "UPDATE fmo_lnv2.gateways SET registered=true WHERE api_endpoint LIKE 'https://weak%'",
        &[],
    )
    .await?;
    let skipped = gateways::admit(&tx, &fed, 0, &[], std::collections::BTreeMap::new()).await?;
    assert_eq!(skipped, 0);
    assert_eq!(
        count(
            &tx,
            "SELECT count(*) FROM fmo_lnv2.gateways WHERE registered"
        )
        .await?,
        cap
    );
    assert_eq!(
        count(&tx, "SELECT count(*) FROM fmo_lnv2.gateways WHERE registered AND api_endpoint LIKE 'https://weak%'").await?,
        0
    );
    tx.rollback().await?;
    Ok(())
}

#[test]
fn stored_gateway_alias_is_bounded_and_storable() {
    let key = "02e7c0a1e3a7b7b2b0e5d2a9c1f0b3a4d5e6f708192a3b4c5d6e7f8091a2b3c4d5";
    let fee = serde_json::json!({"base":1000,"parts_per_million":100});
    let alias = format!("node\u{0}\n{}", "é".repeat(1024 * 1024));
    let info: fedimint_lnv2_common::gateway_api::RoutingInfo =
        serde_json::from_value(serde_json::json!({
            "lightning_public_key":key,"lightning_alias":alias,"module_public_key":key,
            "send_fee_minimum":fee,"send_fee_default":fee,"receive_fee":fee,
            "expiration_delta_minimum":10,"expiration_delta_default":20
        }))
        .expect("routing info");
    let stored = probes::bounded_routing(info).unwrap();
    let alias = stored["lightning_alias"].as_str().unwrap();
    assert_eq!(alias.chars().count(), probes::MAX_ALIAS_CHARS);
    assert!(alias.starts_with("nodeé") && !alias.chars().any(char::is_control));
    assert!(serde_json::to_vec(&stored).unwrap().len() < 2048);
    assert_eq!(stored["module_public_key"], key);
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
        // IANA special-purpose blocks, including ones that embed or tunnel IPv4
        "192.0.0.8",
        "192.0.2.1",
        "192.88.99.1",
        "198.19.255.255",
        "198.51.100.7",
        "203.0.113.7",
        "224.0.0.1",
        "240.0.0.1",
        "172.31.255.255",
        "::",
        "::ffff:10.0.0.1",
        "64:ff9b::a00:1",
        "64:ff9b:1::1",
        "100::1",
        "2001::1",
        "2001:0:4136:e378::1",
        "2001:2::1",
        "2001:20::1",
        "2001:1ff:ffff::1",
        "2002:a00:1::1",
        "3fff::1",
        "3fff:fff:ffff::1",
        "5f00::1",
        "ff02::1",
    ] {
        assert!(
            !public_http::public_ip(address.parse().unwrap()),
            "{address}"
        );
    }
    // Neighbours of the blocks stay public, so the checks are not over-broad.
    for address in [
        "1.1.1.1",
        "192.0.1.1",
        "192.0.3.1",
        "192.88.98.1",
        "198.17.255.255",
        "198.20.0.1",
        "172.32.0.1",
        "100.128.0.1",
        "223.255.255.254",
        "::ffff:1.1.1.1",
        "2606:4700:4700::1111",
        "2001:200::1",
        "2003::1",
        "3ffe:ffff::1",
        "3fff:1000::1",
    ] {
        assert!(
            public_http::public_ip(address.parse().unwrap()),
            "{address}"
        );
    }
}

#[test]
fn guardian_announcements_are_capped_and_ordered_by_agreement() {
    let many: Vec<String> = (0..gateways::MAX_URLS_PER_GUARDIAN + 10)
        .rev()
        .map(|index| format!("https://gw{index:04}.example/"))
        .collect();
    let shared = "https://gw0000.example/".to_owned();
    let mut duplicated = vec![shared.clone(); 3];
    duplicated.push("https://zz.example/".to_owned());
    let (registrations, complete) =
        gateways::bounded_registrations(vec![(0, many), (1, duplicated)]);
    // The flooding guardian keeps a deterministic, capped subset and is incomplete;
    // duplicates don't count against the other guardian's limit.
    assert_eq!(complete, 1);
    assert_eq!(registrations.len(), gateways::MAX_URLS_PER_GUARDIAN + 1);
    assert!(registrations.contains_key("https://gw0255.example/"));
    assert!(!registrations.contains_key("https://gw0256.example/"));
    let ordered = gateways::admission_order(registrations);
    assert_eq!(ordered[0], (shared, vec![0, 1]));
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
