use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::extract::{Path, State};
use axum::Json;
use chrono::Utc;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use serde_json::json;

use super::api::{authenticate, federation_observations};
use super::config::parse_push_sources;
use super::sink::{prune, upsert, validate};
use super::{
    ObservationSource, PushSource, PushToken, Recorded, SourceKind, SourceRegistry, MAX_JSON_DEPTH,
    MAX_OBSERVATION_BYTES,
};
use crate::error::ClientError;
use crate::federation::observer::FederationObserver;
use crate::AppState;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// A pulling source, with no push token
struct Pulling(&'static str, Vec<SourceKind>);

impl ObservationSource for Pulling {
    fn id(&self) -> &str {
        self.0
    }
    fn kinds(&self) -> Vec<SourceKind> {
        self.1.clone()
    }
}

fn pushing(id: &str, kinds: Vec<SourceKind>) -> Arc<dyn ObservationSource> {
    Arc::new(PushSource::new(
        id,
        kinds,
        PushToken::new(TOKEN).expect("valid"),
    ))
}

fn registry(sources: Vec<Arc<dyn ObservationSource>>) -> anyhow::Result<SourceRegistry> {
    SourceRegistry::new(sources)
}

#[test]
fn registry_accepts_valid_sources() {
    let sources = registry(vec![
        pushing(
            "uptime_probe",
            vec![SourceKind::new("guardian.latency-ms").public()],
        ),
        Arc::new(Pulling("directory", vec![SourceKind::new("listing")])),
    ])
    .expect("valid");
    assert_eq!(
        sources.ids().collect::<Vec<_>>(),
        ["directory", "uptime_probe"]
    );
    assert!(sources
        .kind("uptime_probe", "guardian.latency-ms")
        .is_some());
    assert!(sources.kind("uptime_probe", "listing").is_none());
}

#[test]
fn registry_rejects_invalid_declarations() {
    let kind = || vec![SourceKind::new("k")];
    for id in ["", "Upper", "1st", "has-dash", "has space", &"a".repeat(33)] {
        assert!(registry(vec![pushing(id, kind())]).is_err(), "id {id:?}");
    }
    for name in ["", "Upper", "_lead", "slash/y", &"a".repeat(65)] {
        assert!(
            registry(vec![pushing("s", vec![SourceKind::new(name)])]).is_err(),
            "kind {name:?}"
        );
    }
    // No kinds, a kind twice, a source twice
    assert!(registry(vec![pushing("s", vec![])]).is_err());
    assert!(registry(vec![pushing(
        "s",
        vec![SourceKind::new("k"), SourceKind::new("k")]
    )])
    .is_err());
    assert!(registry(vec![pushing("s", kind()), pushing("s", kind())]).is_err());
    // Limits of sizes and retention
    for bad in [
        SourceKind::new("k").max_bytes(0),
        SourceKind::new("k").max_bytes(MAX_OBSERVATION_BYTES + 1),
        SourceKind::new("k").retention(Duration::from_secs(60)),
        SourceKind::new("k").retention(Duration::from_secs(401 * 24 * 60 * 60)),
    ] {
        assert!(
            registry(vec![pushing("s", vec![bad.clone()])]).is_err(),
            "{bad:?}"
        );
    }
    // Too many kinds or sources
    let many = (0..65).map(|i| SourceKind::new(format!("k{i}"))).collect();
    assert!(registry(vec![pushing("s", many)]).is_err());
    let sources = (0..65).map(|i| pushing(&format!("s{i}"), kind())).collect();
    assert!(registry(sources).is_err());
}

fn status(result: Result<(), ClientError>) -> Option<u16> {
    result.err().map(|error| error.status().as_u16())
}

#[test]
fn observations_are_validated_against_their_kind() {
    let kind = SourceKind::new("k").max_bytes(64);
    let now = Utc::now();
    let ok = json!({ "p50_ms": 120 });
    assert_eq!(status(validate(Some(&kind), "k", now, &ok, now)), None);
    // Undeclared kinds, non-objects, oversized data
    assert_eq!(status(validate(None, "k", now, &ok, now)), Some(400));
    for data in [json!([1]), json!("text"), json!(null), json!(1)] {
        assert_eq!(
            status(validate(Some(&kind), "k", now, &data, now)),
            Some(400)
        );
    }
    let big = json!({ "x": "y".repeat(64) });
    assert_eq!(
        status(validate(Some(&kind), "k", now, &big, now)),
        Some(400)
    );
    // A little clock skew is fine, the future is not, nor the past retention
    let soon = now + chrono::Duration::minutes(4);
    assert_eq!(status(validate(Some(&kind), "k", soon, &ok, now)), None);
    let later = now + chrono::Duration::minutes(6);
    assert_eq!(
        status(validate(Some(&kind), "k", later, &ok, now)),
        Some(400)
    );
    let old = now - chrono::Duration::days(8);
    assert_eq!(status(validate(Some(&kind), "k", old, &ok, now)), Some(400));
}

#[test]
fn deeply_nested_observations_are_rejected() {
    let kind = SourceKind::new("k").max_bytes(MAX_OBSERVATION_BYTES);
    let now = Utc::now();
    let nested = |depth: usize| (1..depth).fold(json!({}), |inner, _| json!({ "a": inner }));
    assert_eq!(
        status(validate(
            Some(&kind),
            "k",
            now,
            &nested(MAX_JSON_DEPTH),
            now
        )),
        None
    );
    assert_eq!(
        status(validate(
            Some(&kind),
            "k",
            now,
            &nested(MAX_JSON_DEPTH + 1),
            now
        )),
        Some(400)
    );
}

#[test]
fn observations_with_nul_characters_are_rejected() {
    // PostgreSQL's jsonb can't store U+0000, in a value or a key, at any depth
    let kind = SourceKind::new("k");
    let now = Utc::now();
    for data in [
        json!({ "name": "a\u{0}b" }),
        json!({ "a\u{0}b": 1 }),
        json!({ "list": [1, { "deep": ["\u{0}"] }] }),
        json!({ "outer": { "\u{0}": null } }),
    ] {
        assert_eq!(
            status(validate(Some(&kind), "k", now, &data, now)),
            Some(400),
            "{data}"
        );
    }
    // As the push endpoint parses it from an escape
    let pushed: serde_json::Value = serde_json::from_str(r#"{"x": "\u0000"}"#).expect("valid");
    assert_eq!(
        status(validate(Some(&kind), "k", now, &pushed, now)),
        Some(400)
    );
}

#[test]
fn push_tokens_verify_and_stay_secret() {
    let token = PushToken::new(TOKEN).expect("valid");
    assert!(token.verify(TOKEN));
    assert!(!token.verify(&TOKEN[1..]));
    assert!(!token.verify(""));
    assert!(!format!("{token:?}").contains(TOKEN));
    assert!(PushToken::new("short").is_err());
    assert!(PushToken::new(&"a".repeat(513)).is_err());
}

#[test]
fn only_a_push_sources_own_token_authenticates() {
    let sources = registry(vec![
        pushing("a", vec![SourceKind::new("k")]),
        Arc::new(PushSource::new(
            "b",
            vec![SourceKind::new("k")],
            PushToken::new(&"b".repeat(32)).expect("valid"),
        )),
        Arc::new(Pulling("pull", vec![SourceKind::new("k")])),
    ])
    .expect("valid");
    assert_eq!(
        authenticate(&sources, "a", TOKEN).map(|source| source.id.clone()),
        Some("a".to_owned())
    );
    // Another source's token, an unknown source, a source without pushes
    assert!(authenticate(&sources, "b", TOKEN).is_none());
    assert!(authenticate(&sources, "unknown", TOKEN).is_none());
    assert!(authenticate(&sources, "pull", TOKEN).is_none());
    assert!(authenticate(&sources, "pull", &"\u{0}".repeat(32)).is_none());
}

#[test]
fn sources_file_needs_tokens_and_known_fields() {
    let file = r#"{"sources": [{"id": "probe", "kinds": [
        {"name": "latency", "public": true, "max_bytes": 2048, "retention_hours": 48}
    ]}]}"#;
    let env = |name: &str| (name == "FO_SOURCE_TOKEN_PROBE").then(|| TOKEN.to_owned());
    let sources = parse_push_sources(file, env).expect("valid");
    assert_eq!(sources.len(), 1);
    let kinds = sources[0].kinds();
    assert_eq!(
        kinds,
        vec![SourceKind::new("latency")
            .public()
            .max_bytes(2048)
            .retention(Duration::from_secs(48 * 60 * 60))]
    );

    // No token, a short one, an unknown field
    assert!(parse_push_sources(file, |_| None).is_err());
    assert!(parse_push_sources(file, |_| Some("short".to_owned())).is_err());
    let unknown = r#"{"sources": [{"id": "probe", "token": "x", "kinds": []}]}"#;
    assert!(parse_push_sources(unknown, env).is_err());
}

#[test]
fn metrics_count_each_sources_writes() {
    let sources = registry(vec![pushing("probe", vec![SourceKind::new("k")])]).expect("valid");
    let mut body = String::new();
    sources.render_metrics(&mut body);
    assert!(body.contains("fmo_source_observations_total{source=\"probe\",result=\"stored\"} 0"));
    let mut empty = String::new();
    registry(vec![]).expect("valid").render_metrics(&mut empty);
    assert!(empty.is_empty());
}

/// A schema of a test's own in `FMO_SOURCES_TEST_DATABASE`, holding the
/// tables sources use
struct TestSchema {
    database: String,
    name: String,
    client: tokio_postgres::Client,
    driver: tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
}

impl TestSchema {
    /// Tests run in parallel, so each names its own schema
    async fn new(test: &str) -> anyhow::Result<Self> {
        let database = std::env::var("FMO_SOURCES_TEST_DATABASE")?;
        let (client, driver) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
        let driver = tokio::spawn(driver);
        let name = format!("fmo_sources_test_{}_{test}", std::process::id());
        // One implicit transaction: a failing migration leaves no schema
        client
            .batch_execute(&format!(
                "CREATE SCHEMA {name}; SET search_path TO {name};
                 CREATE TABLE schema_version(version INTEGER);
                 CREATE TABLE federations(federation_id BYTEA PRIMARY KEY, config BYTEA NOT NULL);
                 {}",
                include_str!("../../schema/v13.sql")
            ))
            .await?;
        Ok(Self {
            database,
            name,
            client,
            driver,
        })
    }

    /// An observer whose connections see only this schema
    async fn observer(&self, sources: SourceRegistry) -> anyhow::Result<FederationObserver> {
        let pool = deadpool_postgres::Config {
            url: Some(self.database.clone()),
            options: Some(format!("-c search_path={}", self.name)),
            ..Default::default()
        }
        .create_pool(
            Some(deadpool_postgres::Runtime::Tokio1),
            tokio_postgres::NoTls,
        )?;
        FederationObserver::for_tests(pool, sources).await
    }

    async fn finish(self, result: anyhow::Result<()>) -> anyhow::Result<()> {
        self.client
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.name))
            .await?;
        drop(self.client);
        self.driver.await??;
        result
    }
}

async fn observe(
    client: &tokio_postgres::Client,
    federation_id: FederationId,
) -> anyhow::Result<()> {
    client
        .execute(
            "INSERT INTO federations VALUES ($1, '')",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires an isolated FMO_SOURCES_TEST_DATABASE"]
async fn postgres_newest_observation_wins_and_retention_prunes() -> anyhow::Result<()> {
    let db = TestSchema::new("upsert").await?;
    let result = async {
        let client = &db.client;
        let federation_id = FederationId::dummy();
        observe(client, federation_id).await?;
        let now = Utc::now();
        let earlier = now - chrono::Duration::minutes(10);
        let data = json!({ "p50_ms": 1 });

        assert_eq!(
            upsert(client, federation_id, "probe", "latency", earlier, &data).await?,
            Recorded::Stored
        );
        // Newer replaces, older and same-time retries don't
        let newer = json!({ "p50_ms": 2 });
        assert_eq!(
            upsert(client, federation_id, "probe", "latency", now, &newer).await?,
            Recorded::Stored
        );
        assert_eq!(
            upsert(client, federation_id, "probe", "latency", earlier, &data).await?,
            Recorded::Superseded
        );
        assert_eq!(
            upsert(client, federation_id, "probe", "latency", now, &data).await?,
            Recorded::Superseded
        );
        let stored: serde_json::Value = client
            .query_one("SELECT data FROM source_observations", &[])
            .await?
            .get(0);
        assert_eq!(stored, newer);

        // The table checks what the registry checks
        assert!(upsert(client, federation_id, "Bad", "latency", now, &data)
            .await
            .is_err());
        assert!(
            upsert(client, federation_id, "probe", "latency", now, &json!([1]))
                .await
                .is_err()
        );

        // Retention: an expired declared row and an old undeclared one go,
        // a recent undeclared one stays for its grace period
        client
            .batch_execute(
                "UPDATE source_observations SET observed_at = now() - INTERVAL '3 hours';
                 INSERT INTO source_observations (federation_id, source, kind, data, observed_at, received_at)
                 SELECT federation_id, 'gone', 'old', '{}', now() - INTERVAL '40 days', now() - INTERVAL '40 days'
                 FROM federations;
                 INSERT INTO source_observations (federation_id, source, kind, data, observed_at)
                 SELECT federation_id, 'gone', 'recent', '{}', now() FROM federations;",
            )
            .await?;
        let sources = registry(vec![pushing(
            "probe",
            vec![SourceKind::new("latency").retention(Duration::from_secs(60 * 60))],
        )])?;
        assert_eq!(prune(client, &sources).await?, 2);
        let left: Vec<String> = client
            .query("SELECT source || '/' || kind FROM source_observations", &[])
            .await?
            .iter()
            .map(|row| row.get(0))
            .collect();
        assert_eq!(left, ["gone/recent"]);
        anyhow::Ok(())
    }
    .await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires an isolated FMO_SOURCES_TEST_DATABASE"]
async fn postgres_retention_runs_without_sources() -> anyhow::Result<()> {
    let db = TestSchema::new("retention").await?;
    let result = async {
        observe(&db.client, FederationId::dummy()).await?;
        // Rows of a source a deploy no longer declares, one past its grace
        db.client
            .batch_execute(
                "INSERT INTO source_observations (federation_id, source, kind, data, observed_at, received_at)
                 SELECT federation_id, 'gone', 'old', '{}', now() - INTERVAL '40 days', now() - INTERVAL '40 days'
                 FROM federations;
                 INSERT INTO source_observations (federation_id, source, kind, data, observed_at)
                 SELECT federation_id, 'gone', 'recent', '{}', now() FROM federations;",
            )
            .await?;
        let observer = db.observer(registry(vec![])?).await?;
        observer.spawn_source_tasks();
        let pruned = async {
            loop {
                let left: Vec<String> = db
                    .client
                    .query("SELECT source || '/' || kind FROM source_observations", &[])
                    .await?
                    .iter()
                    .map(|row| row.get(0))
                    .collect();
                if left == ["gone/recent"] {
                    return anyhow::Ok(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        };
        let waited = tokio::time::timeout(Duration::from_secs(10), pruned).await;
        observer.task_group().shutdown();
        waited.context("Retention didn't run in a build without sources")?
    }
    .await;
    db.finish(result).await
}

#[tokio::test]
#[ignore = "requires an isolated FMO_SOURCES_TEST_DATABASE"]
async fn postgres_public_endpoint_serves_only_public_kinds() -> anyhow::Result<()> {
    let db = TestSchema::new("public").await?;
    let result = async {
        let federation_id = FederationId::dummy();
        observe(&db.client, federation_id).await?;
        let sources = registry(vec![pushing(
            "probe",
            vec![
                SourceKind::new("latency").public(),
                SourceKind::new("uptime")
                    .public()
                    .retention(Duration::from_secs(60 * 60)),
                SourceKind::new("raw"),
            ],
        )])?;
        // A public kind, one past its retention, a private kind and a source
        // the build doesn't declare
        db.client
            .batch_execute(
                r#"INSERT INTO source_observations (federation_id, source, kind, data, observed_at)
                   SELECT federation_id, v.source, v.kind, v.data::jsonb, v.observed_at
                   FROM federations, (VALUES
                       ('probe', 'latency', '{"p50_ms": 1}', now()),
                       ('probe', 'uptime', '{"up": true}', now() - INTERVAL '3 hours'),
                       ('probe', 'raw', '{"private": 1}', now()),
                       ('gone', 'latency', '{"private": 2}', now())
                   ) AS v(source, kind, data, observed_at)"#,
            )
            .await?;
        let observer = db.observer(sources).await?;
        let state = AppState {
            federation_config_cache: Default::default(),
            meta_override_cache: Default::default(),
            consensus_meta_cache: Default::default(),
            federation_observer: observer,
            http_metrics: Arc::new(crate::http::HttpMetrics::new(1)),
        };
        let Json(served) = federation_observations(Path(federation_id), State(state))
            .await
            .map_err(|_| anyhow::anyhow!("The request failed"))?;
        let served: Vec<_> = served
            .iter()
            .map(|observation| {
                (
                    observation.source.as_str(),
                    observation.kind.as_str(),
                    &observation.data,
                )
            })
            .collect();
        assert_eq!(served, [("probe", "latency", &json!({ "p50_ms": 1 }))]);
        anyhow::Ok(())
    }
    .await;
    db.finish(result).await
}
