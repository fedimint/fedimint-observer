use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;

use fedimint_core::config::{ClientConfig, FederationId, GlobalClientConfig, PeerUrl};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::CoreConsensusVersion;
use fedimint_core::net::api_announcement::ApiAnnouncement;
use fedimint_core::secp256k1::{Keypair, PublicKey, SecretKey, SECP256K1};
use fedimint_core::task::TaskGroup;
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;

use super::{load_guardian_endpoint_inputs, uses_superseded_routes, FederationObserver};
use crate::federation::endpoints::GUARDIAN_ENDPOINTS_OBSERVATION_KIND;

async fn endpoint_test_observer(
    pool: deadpool_postgres::Pool,
) -> anyhow::Result<FederationObserver> {
    Ok(FederationObserver {
        connection_pool: pool.clone(),
        worker_pool: pool,
        worker_lease: None,
        worker_epoch: Some(1),
        work_budget: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        external_io: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        work_notifications: Default::default(),
        health_retention_days: 0,
        admin_auth: String::new(),
        esplora: esplora_client::Builder::new("http://127.0.0.1:1").build_async()?,
        task_group: TaskGroup::default(),
        connectors: fedimint_connectors::ConnectorRegistry::build_from_testing_defaults()
            .bind()
            .await?,
        federation_config_cache: Default::default(),
        api_clients: Default::default(),
        guardian_endpoints: Default::default(),
        guardian_endpoint_revision: Default::default(),
        modules: crate::module::ModuleRegistry::new(Vec::new())?,
        projector_wakeups: Default::default(),
        nostr_pool: Default::default(),
        spawned_federations: Default::default(),
    })
}

fn independent_endpoint_state(template: &FederationObserver) -> FederationObserver {
    FederationObserver {
        task_group: TaskGroup::default(),
        api_clients: Default::default(),
        guardian_endpoints: Default::default(),
        guardian_endpoint_revision: Default::default(),
        spawned_federations: Default::default(),
        ..template.clone()
    }
}

fn endpoint_test_config() -> (FederationId, ClientConfig, PeerId, Keypair, PublicKey) {
    let peer_id = PeerId::from(0);
    let secret_key = SecretKey::from_slice(&[1; 32]).expect("valid test secret key");
    let keypair = Keypair::from_secret_key(SECP256K1, &secret_key);
    let public_key = PublicKey::from_keypair(&keypair);
    let global = GlobalClientConfig {
        api_endpoints: BTreeMap::from([(
            peer_id,
            PeerUrl {
                url: SafeUrl::from_str("wss://canonical.example.com:8173").unwrap(),
                name: "guardian-0".to_owned(),
            },
        )]),
        broadcast_public_keys: Some(BTreeMap::from([(peer_id, public_key)])),
        consensus_version: CoreConsensusVersion::new(0, 0),
        meta: BTreeMap::new(),
    };
    let federation_id = global.calculate_federation_id();
    (
        federation_id,
        ClientConfig {
            global,
            modules: BTreeMap::new(),
        },
        peer_id,
        keypair,
        public_key,
    )
}

fn cached_api_peers(label: &str) -> super::ApiPeers {
    BTreeMap::from([(
        PeerId::from(0),
        SafeUrl::from_str(&format!("wss://{label}.example.com")).unwrap(),
    )])
}

#[tokio::test]
async fn canonical_recovery_eviction_never_consumes_active_capacity() -> anyhow::Result<()> {
    let pool = crate::runtime::pool("postgresql://localhost/unused", 1, false)?;
    let observer = endpoint_test_observer(pool).await?;
    observer.federation_api_for_peers(cached_api_peers("active-0"))?;

    for index in 0..=super::MAX_RECOVERY_API_CLIENTS {
        observer.recovery_api_for_peers(cached_api_peers(&format!("recovery-{index}")))?;
    }
    {
        let clients = observer
            .api_clients
            .lock()
            .expect("API client cache lock poisoned");
        assert_eq!(clients.observed.len(), 1);
        assert_eq!(clients.recovery.len(), super::MAX_RECOVERY_API_CLIENTS);
    }

    observer.federation_api_for_peers(cached_api_peers("active-1"))?;
    let clients = observer
        .api_clients
        .lock()
        .expect("API client cache lock poisoned");
    assert_eq!(
        clients.observed.len(),
        2,
        "evictable canonical clients must not deny a new active client"
    );
    assert_eq!(clients.recovery.len(), super::MAX_RECOVERY_API_CLIENTS);
    Ok(())
}

#[tokio::test]
async fn full_lookup_cache_evicts_instead_of_refusing() -> anyhow::Result<()> {
    let pool = crate::runtime::pool("postgresql://localhost/unused", 1, false)?;
    let observer = endpoint_test_observer(pool).await?;
    let key = |label: &str| (cached_api_peers(label), Some("secret".to_owned()));
    for index in 0..super::MAX_LOOKUP_API_CLIENTS {
        let (peers, secret) = key(&format!("lookup-{index}"));
        observer.lookup_api_in(
            &mut observer.api_clients(),
            peers,
            secret,
            crate::guardian_api::ConnectionClass::Lookup,
        )?;
    }

    // A literal fallback after a failed announced attempt still gets a client.
    let (peers, secret) = key("literal-fallback");
    observer.lookup_api_in(
        &mut observer.api_clients(),
        peers,
        secret,
        crate::guardian_api::ConnectionClass::LookupFallback,
    )?;
    let clients = observer.api_clients();
    assert_eq!(clients.lookups.len(), super::MAX_LOOKUP_API_CLIENTS);
    assert!(clients.lookups.contains_key(&key("literal-fallback")));
    assert!(
        !clients.lookups.contains_key(&key("lookup-0")),
        "least recent evicted"
    );
    Ok(())
}

#[test]
fn rotation_retires_full_and_single_peer_clients_using_the_old_route() {
    let peer0 = PeerId::from(0);
    let peer1 = PeerId::from(1);
    let old_url = SafeUrl::from_str("wss://old.example.com").unwrap();
    let new_url = SafeUrl::from_str("wss://new.example.com").unwrap();
    let stable_url = SafeUrl::from_str("wss://stable.example.com").unwrap();
    let old = BTreeMap::from([(peer0, old_url.clone()), (peer1, stable_url.clone())]);
    let new = BTreeMap::from([(peer0, new_url.clone()), (peer1, stable_url.clone())]);

    assert!(uses_superseded_routes(&old, &old, &new));
    assert!(uses_superseded_routes(
        &BTreeMap::from([(peer0, old_url)]),
        &old,
        &new
    ));
    assert!(!uses_superseded_routes(
        &BTreeMap::from([(peer0, new_url)]),
        &old,
        &new
    ));
    assert!(!uses_superseded_routes(
        &BTreeMap::from([(peer1, stable_url)]),
        &old,
        &new
    ));
}

#[tokio::test]
#[ignore = "requires an isolated FMO_WORK_TEST_DATABASE"]
async fn postgres_endpoint_restore_completes_with_one_database_connection() -> anyhow::Result<()> {
    let database = std::env::var("FMO_WORK_TEST_DATABASE")?;
    let pool = crate::runtime::pool(&database, 1, false)?;
    let connection = pool.get().await?;
    connection
        .batch_execute(
            "CREATE TEMP TABLE federation_observations(
                 federation_id BYTEA NOT NULL,
                 kind TEXT NOT NULL,
                 data JSONB NOT NULL,
                 revision BIGINT NOT NULL
             );
             CREATE TEMP TABLE federations(
                 federation_id BYTEA NOT NULL,
                 config BYTEA NOT NULL
             );
             INSERT INTO federations
                 SELECT int4send(i), '\\x00'::bytea FROM generate_series(1, 129) AS i;
             INSERT INTO federation_observations
                 SELECT int4send(i), 'guardian_endpoints', '{}'::jsonb, i
                 FROM generate_series(1, 129) AS i;",
        )
        .await?;
    drop(connection);

    let observations = tokio::time::timeout(
        Duration::from_secs(2),
        load_guardian_endpoint_inputs(&pool, 0),
    )
    .await
    .expect("endpoint restore must not wait for a second pool checkout")?;

    assert_eq!(
        observations.len(),
        128,
        "one refresh has a fixed row budget"
    );
    assert_eq!(
        observations.first().map(|row| row.get::<_, i64>(2)),
        Some(1)
    );
    assert_eq!(
        observations.last().map(|row| row.get::<_, i64>(2)),
        Some(128)
    );

    let remainder = tokio::time::timeout(
        Duration::from_secs(2),
        load_guardian_endpoint_inputs(&pool, 128),
    )
    .await
    .expect("the next cursor page must reuse the one-connection pool")?;
    assert_eq!(remainder.len(), 1);
    assert_eq!(remainder[0].get::<_, i64>(2), 129);
    Ok(())
}

/// Isolated v12 tables for running the real v13 migration
fn v12_route_schema(schema: &str) -> String {
    format!(
        "DROP SCHEMA IF EXISTS {schema} CASCADE;
         CREATE SCHEMA {schema};
         SET search_path TO {schema};
         CREATE TABLE schema_version(version INTEGER);
         INSERT INTO schema_version VALUES (12);
         CREATE TABLE federations(
             federation_id BYTEA PRIMARY KEY,
             config BYTEA NOT NULL
         );
         CREATE TABLE federation_observations(
             federation_id BYTEA NOT NULL,
             kind TEXT NOT NULL,
             data JSONB NOT NULL,
             observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
             PRIMARY KEY(federation_id, kind)
         );"
    )
}

/// The statements of the production route commit, without the merge.
async fn fenced_route_write(
    transaction: &tokio_postgres::Transaction<'_>,
    federation_id: &[u8],
) -> anyhow::Result<i64> {
    let state = transaction
        .query_one(
            "SELECT worker_epoch, last_revision
             FROM guardian_endpoint_write_state
             WHERE singleton = TRUE
             FOR UPDATE",
            &[],
        )
        .await?;
    let epoch: i64 = state.get(0);
    let revision = state.get::<_, i64>(1) + 1;
    transaction
        .query_one(
            "SELECT set_config('fmo.guardian_endpoint_worker_epoch', $1, TRUE)",
            &[&epoch.to_string()],
        )
        .await?;
    transaction
        .execute(
            "UPDATE guardian_endpoint_write_state SET last_revision = $1",
            &[&revision],
        )
        .await?;
    transaction
        .execute(
            "INSERT INTO federation_observations(federation_id, kind, data, revision)
             VALUES ($1, 'guardian_endpoints', '{}', $2)",
            &[&federation_id, &revision],
        )
        .await?;
    Ok(revision)
}

#[tokio::test]
#[ignore = "requires an isolated FMO_WORK_TEST_DATABASE"]
async fn postgres_guardian_route_migration_fences_revisions() -> anyhow::Result<()> {
    const SCHEMA: &str = "fmo_test_guardian_route_fence";
    let database = std::env::var("FMO_WORK_TEST_DATABASE")?;
    let (mut first, first_driver) =
        tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    let first_driver = tokio::spawn(first_driver);
    first.batch_execute(&v12_route_schema(SCHEMA)).await?;
    first
        .batch_execute(
            "INSERT INTO federation_observations(federation_id, kind, data) VALUES
                 ('\\x01', 'guardian_endpoints', '{\"0\": {
                     \"url\": \"wss://legacy.example.com/\",
                     \"nonce\": 1,
                     \"announcement\": {\"signature\": \"aa\", \"api_announcement\": {
                         \"nonce\": 1, \"api_url\": \"wss://legacy.example.com/\"}}
                 }}'),
                 ('\\x02', 'meta', '{\"url\": \"kept\", \"announcement\": \"kept\"}');",
        )
        .await?;
    first
        .batch_execute(include_str!("../../schema/v13.sql"))
        .await?;

    let backfilled = first
        .query(
            "SELECT kind, revision FROM federation_observations ORDER BY kind",
            &[],
        )
        .await?
        .into_iter()
        .map(|row| (row.get::<_, String>(0), row.get::<_, Option<i64>>(1)))
        .collect::<Vec<_>>();
    assert_eq!(
        backfilled,
        vec![
            ("guardian_endpoints".to_owned(), Some(1)),
            ("meta".to_owned(), None)
        ],
        "only existing route rows receive a revision"
    );
    let migrated = first
        .query(
            "SELECT kind, data FROM federation_observations ORDER BY kind",
            &[],
        )
        .await?
        .into_iter()
        .map(|row| row.get::<_, serde_json::Value>(1))
        .collect::<Vec<_>>();
    assert_eq!(
        migrated,
        vec![
            serde_json::json!({"0": {
                "signature": "aa",
                "api_announcement": {"nonce": 1, "api_url": "wss://legacy.example.com/"}
            }}),
            serde_json::json!({"url": "kept", "announcement": "kept"}),
        ],
        "legacy route rows keep only the signed announcement; other kinds are untouched"
    );
    let state = first
        .query_one(
            "SELECT worker_epoch, last_revision FROM guardian_endpoint_write_state",
            &[],
        )
        .await?;
    assert_eq!((state.get::<_, i64>(0), state.get::<_, i64>(1)), (0, 1));
    assert_eq!(
        first
            .query_one("SELECT max(version) FROM schema_version", &[])
            .await?
            .get::<_, i32>(0),
        13
    );
    // Other observation kinds keep their unfenced write path.
    first
        .execute(
            "INSERT INTO federation_observations(federation_id, kind, data)
             VALUES ('\\x02', 'meta', '{\"updated\":true}')
             ON CONFLICT(federation_id, kind) DO UPDATE SET data = EXCLUDED.data",
            &[],
        )
        .await?;
    first
        .execute(
            "UPDATE guardian_endpoint_write_state SET worker_epoch = 1",
            &[],
        )
        .await?;

    // The second commit waits and gets the next revision.
    let first_transaction = first.transaction().await?;
    let first_revision = fenced_route_write(&first_transaction, &[2]).await?;

    let (mut second, second_driver) =
        tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    let second_driver = tokio::spawn(second_driver);
    second
        .batch_execute(&format!("SET search_path TO {SCHEMA}"))
        .await?;
    let second_write = tokio::spawn(async move {
        let transaction = second.transaction().await?;
        let revision = fenced_route_write(&transaction, &[3]).await?;
        transaction.commit().await?;
        anyhow::Ok(revision)
    });

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !second_write.is_finished(),
        "the next revision must wait for the earlier transaction to commit"
    );
    first_transaction.commit().await?;
    let second_revision = second_write.await??;

    let revisions = first
        .query(
            "SELECT revision FROM federation_observations
             WHERE kind = 'guardian_endpoints'
             ORDER BY revision",
            &[],
        )
        .await?
        .into_iter()
        .map(|row| row.get::<_, i64>(0))
        .collect::<Vec<_>>();
    assert_eq!(revisions, vec![1, 2, 3]);
    assert_eq!((first_revision, second_revision), (2, 3));

    for (setting, case) in [("", "missing"), ("0", "stale")] {
        let transaction = first.transaction().await?;
        transaction
            .query_one(
                "SELECT set_config('fmo.guardian_endpoint_worker_epoch', $1, TRUE)",
                &[&setting],
            )
            .await?;
        let rejected = transaction
            .execute(
                "INSERT INTO federation_observations(federation_id, kind, data, revision)
                 VALUES ('\\x04', 'guardian_endpoints', '{}', 4)",
                &[],
            )
            .await
            .expect_err("a route write without the current worker epoch must fail");
        assert!(
            rejected
                .as_db_error()
                .is_some_and(|error| error.message().contains("stale or missing worker epoch")),
            "{case} epoch"
        );
        transaction.rollback().await?;
    }

    first
        .batch_execute(&format!("DROP SCHEMA {SCHEMA} CASCADE"))
        .await?;
    first_driver.abort();
    second_driver.abort();
    Ok(())
}

/// A session holding the worker lock, like the worker's lease
async fn lease_session(database: &str, schema: &str) -> anyhow::Result<tokio_postgres::Client> {
    let (client, connection) = tokio_postgres::connect(database, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::debug!(%error, "Test lease session ended");
        }
    });
    client
        .batch_execute(&format!("SET search_path TO {schema}"))
        .await?;
    let (class_id, object_id) = crate::runtime::WORKER_LOCK;
    // Waits for any other holder, such as a parallel database test.
    client
        .execute("SELECT pg_advisory_lock($1, $2)", &[&class_id, &object_id])
        .await?;
    Ok(client)
}

async fn stored_route_revision(
    pool: &deadpool_postgres::Pool,
    federation_id: FederationId,
) -> anyhow::Result<i64> {
    Ok(pool
        .get()
        .await?
        .query_one(
            "SELECT revision FROM federation_observations
             WHERE federation_id = $1 AND kind = $2",
            &[
                &federation_id.consensus_encode_to_vec(),
                &GUARDIAN_ENDPOINTS_OBSERVATION_KIND,
            ],
        )
        .await?
        .get(0))
}

#[tokio::test]
#[ignore = "requires an isolated FMO_WORK_TEST_DATABASE"]
async fn postgres_worker_and_replica_share_rotations_without_downgrade() -> anyhow::Result<()> {
    let database = std::env::var("FMO_WORK_TEST_DATABASE")?;
    let pool = crate::runtime::pool(&database, 1, false)?;
    let (federation_id, config, peer_id, keypair, public_key) = endpoint_test_config();
    const SCHEMA: &str = "fmo_test_guardian_route_rotation";
    // One pooled connection, so the search path applies to every checkout.
    let connection = pool.get().await?;
    connection.batch_execute(&v12_route_schema(SCHEMA)).await?;
    connection
        .batch_execute(include_str!("../../schema/v13.sql"))
        .await?;
    connection
        .execute(
            "INSERT INTO federations(federation_id, config) VALUES ($1, $2)",
            &[
                &federation_id.consensus_encode_to_vec(),
                &config.consensus_encode_to_vec(),
            ],
        )
        .await?;
    drop(connection);

    let first_lease = lease_session(&database, SCHEMA).await?;
    assert_eq!(super::claim_worker_epoch(&first_lease).await?, 1);
    let worker = endpoint_test_observer(pool.clone()).await?;
    let replica = independent_endpoint_state(&worker);

    let first_url = SafeUrl::from_str("wss://first-rotation.example.com:8173")?;
    let first_announcement = ApiAnnouncement::new(first_url, 1).sign(SECP256K1, &keypair);
    let first_routes = BTreeMap::from([(
        peer_id,
        crate::federation::endpoints::VerifiedRoute::verify(
            first_announcement.clone(),
            &public_key,
        )
        .expect("the first route is signed by its guardian"),
    )]);
    worker
        .update_guardian_endpoints(federation_id, &first_routes)
        .await?;
    replica.load_guardian_endpoint_batch().await?;

    let second_url = SafeUrl::from_str("wss://second-rotation.example.com:8173")?;
    let second_announcement = ApiAnnouncement::new(second_url.clone(), 2).sign(SECP256K1, &keypair);
    let second_routes = BTreeMap::from([(
        peer_id,
        crate::federation::endpoints::VerifiedRoute::verify(second_announcement, &public_key)
            .expect("the second route is signed by its guardian"),
    )]);
    worker
        .update_guardian_endpoints(federation_id, &second_routes)
        .await?;
    replica.load_guardian_endpoint_batch().await?;

    assert_eq!(
        replica
            .active_api_peers(federation_id, &config.global)
            .get(&peer_id),
        Some(&second_url),
        "the independent replica must adopt the worker's second rotation"
    );

    let revision_before_stale = stored_route_revision(&pool, federation_id).await?;

    // A worker without the newer row proposes the older route.
    let stale_worker = independent_endpoint_state(&worker);
    stale_worker
        .update_guardian_endpoints(federation_id, &first_routes)
        .await?;
    let revision_after_stale = stored_route_revision(&pool, federation_id).await?;
    assert_eq!(
        revision_after_stale, revision_before_stale,
        "a stale proposal must not rewrite the durable row"
    );

    let fresh_replica = independent_endpoint_state(&worker);
    fresh_replica.load_guardian_endpoint_batch().await?;

    assert_eq!(
        fresh_replica
            .active_api_peers(federation_id, &config.global)
            .get(&peer_id),
        Some(&second_url),
        "a fresh replica must see the newer durable nonce after a stale worker write"
    );

    // Only the session holding the worker lock can claim an epoch.
    let unleased = pool.get().await?;
    assert!(super::claim_worker_epoch(&unleased).await.is_err());
    drop(unleased);
    drop(first_lease);
    let second_lease = lease_session(&database, SCHEMA).await?;
    assert_eq!(super::claim_worker_epoch(&second_lease).await?, 2);
    let third_announcement = ApiAnnouncement::new(
        SafeUrl::from_str("wss://third-rotation.example.com:8173")?,
        3,
    )
    .sign(SECP256K1, &keypair);
    let third_routes = BTreeMap::from([(
        peer_id,
        crate::federation::endpoints::VerifiedRoute::verify(third_announcement, &public_key)
            .expect("the third route is signed by its guardian"),
    )]);
    let fenced = stale_worker
        .update_guardian_endpoints(federation_id, &third_routes)
        .await
        .expect_err("the old worker epoch must not publish after handoff");
    assert!(format!("{fenced:#}").contains("worker epoch changed"));
    let revision_after_handoff = stored_route_revision(&pool, federation_id).await?;
    assert_eq!(revision_after_handoff, revision_before_stale);
    assert_eq!(
        config.calculate_federation_id(),
        federation_id,
        "dynamic routes must not mutate canonical federation identity"
    );
    pool.get()
        .await?
        .batch_execute(&format!("DROP SCHEMA {SCHEMA} CASCADE"))
        .await?;
    Ok(())
}
