use serde_json::json;

use super::*;
use crate::manifold::peerbadge::{RevocationKind, RevocationRecord};

fn advertisement(
    author: char,
    issued_at: u64,
    payload: serde_json::Value,
) -> VerifiedAdvertisement {
    VerifiedAdvertisement {
        kind: MG_V2_KIND,
        author: author.to_string().repeat(64),
        event_id: author.to_string().repeat(64),
        created_at: issued_at,
        issued_at,
        expires_at: issued_at.saturating_add(3600),
        payload,
    }
}

#[test]
fn trusted_issuers_default_to_manifolds_production_roster() {
    let defaults = parse_trusted_issuers(None).unwrap();
    assert_eq!(defaults.len(), 8);
    assert!(defaults
        .iter()
        .all(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit())));
    let mut unique = defaults.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), defaults.len());

    // The setting replaces the roster; keys are trimmed and lowercased
    let own = "AB".repeat(32);
    assert_eq!(
        parse_trusted_issuers(Some(&format!(" {own} , "))).unwrap(),
        vec!["ab".repeat(32)]
    );
    // Set empty, no issuer is trusted
    assert!(parse_trusted_issuers(Some("")).unwrap().is_empty());
    // A typo is an error, not a silently ignored key
    for bad in ["ab", &"g".repeat(64), &"a".repeat(65), "npub1qqqq"] {
        assert!(parse_trusted_issuers(Some(bad)).is_err(), "{bad}");
    }
    assert!(parse_trusted_issuers(Some(&format!("{own},oops"))).is_err());
}

/// A connection whose `search_path` is a fresh schema of the test's own,
/// with every Manifold migration applied, so nothing else is touched
async fn test_schema(
    name: &str,
) -> anyhow::Result<(
    tokio_postgres::Client,
    tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
    String,
)> {
    let database = std::env::var("FMO_MANIFOLD_TEST_DATABASE")?;
    let (client, connection) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    let driver = tokio::spawn(connection);
    let schema = format!("fmo_manifold_test_{name}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    for migration in MIGRATIONS {
        client.batch_execute(migration).await?;
    }
    Ok((client, driver, schema))
}

async fn drop_schema(
    client: tokio_postgres::Client,
    driver: tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>,
    schema: &str,
) -> anyhow::Result<()> {
    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await?;
    drop(client);
    driver.await??;
    Ok(())
}

async fn sync(
    client: &mut tokio_postgres::Client,
    ads: &[&VerifiedAdvertisement],
    now: u64,
    retention: Retention,
) -> anyhow::Result<StoreOutcome> {
    let mut dbtx = client.transaction().await?;
    let outcome = store_sync(
        &mut dbtx,
        ads,
        &["wss://relay.example".to_owned()],
        now,
        retention,
    )
    .await?;
    dbtx.commit().await?;
    Ok(outcome)
}

async fn column(client: &tokio_postgres::Client, sql: &str) -> anyhow::Result<Vec<String>> {
    Ok(client
        .query(sql, &[])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect())
}

/// One advertisement the database refuses must not cost the others their
/// sync: before each was written under its own savepoint, a single poison
/// event rolled back every advertisement and the day's presence, on every
/// sync that fetched it.
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_poison_advertisements_cannot_block_a_sync() -> anyhow::Result<()> {
    let (mut client, driver, schema) = test_schema("poison").await?;
    let now = unix_now();
    let valid = advertisement(
        'a',
        now - 60,
        json!({"availability": {"fedimintd_version": "0.12.0+fedi"}, "version": {"mg_version": "0.1.11"}}),
    );
    // Both bypass `verify_event`, which rejects them, to reach the database:
    // a timestamp no BIGINT holds, and a NUL that JSONB refuses
    let overflowing = advertisement('b', u64::MAX, json!({}));
    let nul = advertisement('c', now - 60, json!({"display_name": "a\u{0}b"}));

    let outcome = sync(
        &mut client,
        &[&overflowing, &valid, &nul],
        now,
        Retention::DEFAULT,
    )
    .await?;
    assert_eq!(
        outcome,
        StoreOutcome {
            stored: 1,
            refused: 2
        }
    );
    assert_eq!(
        column(&client, "SELECT author FROM advertisements ORDER BY author").await?,
        vec![valid.author.clone()]
    );
    let presence = client
        .query_one(
            "SELECT polls, fresh_polls FROM presence_daily WHERE author = $1",
            &[&valid.author],
        )
        .await?;
    assert_eq!(
        (presence.get::<_, i32>(0), presence.get::<_, i32>(1)),
        (1, 1)
    );
    assert_eq!(
        column(&client, "SELECT author FROM version_changes").await?,
        vec![valid.author.clone()]
    );
    let synced: Vec<String> = client
        .query_one("SELECT relays FROM sync_state", &[])
        .await?
        .get(0);
    assert_eq!(synced, ["wss://relay.example"]);
    drop_schema(client, driver, &schema).await
}

/// What a self-signed advertisement claims cannot decide how long it is kept
/// or how much is stored: before, rows were dropped only by their signed
/// expiry, so a far-future one under a fresh key stayed for good, and every
/// directory request read all of them.
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_retention_ignores_claimed_expiry_and_bounds_storage() -> anyhow::Result<()> {
    let (mut client, driver, schema) = test_schema("retention").await?;
    let retention = Retention {
        max_age_seconds: 3600,
        max_per_kind: 3,
        max_versions_per_author: 2,
    };
    let now = unix_now();
    let versioned = |author: char, created_at: u64, version: &str| VerifiedAdvertisement {
        created_at,
        issued_at: created_at,
        ..advertisement(
            author,
            created_at,
            json!({"availability": {"fedimintd_version": version}}),
        )
    };

    // Expiring in year 9999, but created beyond the horizon
    let far_future = VerifiedAdvertisement {
        expires_at: crate::manifold::discovery::MAX_TIMESTAMP,
        ..versioned('f', now - 7200, "0.1.0")
    };
    // Five fresh keys in repeated batches: only the newest three per kind stay
    let fresh: Vec<_> = ['g', 'h', 'i', 'j', 'k']
        .into_iter()
        .zip(1u64..)
        .map(|(author, n)| VerifiedAdvertisement {
            expires_at: crate::manifold::discovery::MAX_TIMESTAMP,
            ..versioned(author, now - 600 + n * 10, "0.1.0")
        })
        .collect();
    sync(
        &mut client,
        &[&far_future, &fresh[0], &fresh[1]],
        now,
        retention,
    )
    .await?;
    sync(&mut client, &[&fresh[2], &fresh[3]], now, retention).await?;
    sync(&mut client, &[&fresh[4]], now, retention).await?;
    let kept = column(
        &client,
        "SELECT left(author, 1) FROM advertisements ORDER BY author",
    )
    .await?;
    assert_eq!(kept, ["i", "j", "k"]);

    // A guardian's version history is bounded, newest kept, and goes with
    // its advertisements
    for (offset, version) in [(1, "0.2.0"), (2, "0.3.0"), (3, "0.4.0")] {
        let republished = versioned('k', now - 10 + offset, version);
        sync(&mut client, &[&republished], now, retention).await?;
    }
    assert_eq!(
        column(
            &client,
            "SELECT fedimintd_version FROM version_changes WHERE left(author, 1) = 'k' ORDER BY first_seen"
        )
        .await?,
        ["0.3.0", "0.4.0"]
    );
    assert_eq!(
        column(
            &client,
            "SELECT DISTINCT left(author, 1) FROM version_changes ORDER BY 1"
        )
        .await?,
        ["i", "j", "k"]
    );

    // An hour later, only what was republished since is left
    let republished = versioned('k', now + 3500, "0.4.0");
    sync(&mut client, &[&republished], now + 3600, retention).await?;
    assert_eq!(
        column(&client, "SELECT left(author, 1) FROM advertisements").await?,
        ["k"]
    );
    drop_schema(client, driver, &schema).await
}

async fn checked(
    client: &tokio_postgres::Client,
    issuer: &str,
) -> anyhow::Result<Option<(chrono::DateTime<chrono::Utc>, Vec<String>)>> {
    Ok(client
        .query_opt(
            "SELECT checked_at, locations FROM revocation_checks WHERE issuer = $1",
            &[&issuer],
        )
        .await?
        .map(|row| (row.get(0), row.get(1))))
}

/// Completeness is recorded per issuer, so a lookup that did not reach an
/// issuer's locations never refreshes it; a valid revocation wins over
/// invalid events for the same credential
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_revocation_checks_are_per_issuer() -> anyhow::Result<()> {
    let (mut client, driver, schema) = test_schema("revocations").await?;
    let (a, b) = ("aa".repeat(32), "bb".repeat(32));
    let locations = vec!["wss://relay.example/".to_owned()];
    client
        .execute(
            "INSERT INTO revocation_checks (issuer, locations, checked_at)
             VALUES ($1, $2, '2000-01-01T00:00:00Z')",
            &[&a, &locations],
        )
        .await?;
    let seeded = chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")?.to_utc();
    let record = |digest: &str, kind| RevocationRecord {
        issuer: a.clone(),
        credential_digest: digest.to_owned(),
        kind,
        event_id: format!("{digest}-{}", kind.as_str()),
    };

    // Complete for B only: A's check stays exactly as seeded
    let partial = RevocationLookup {
        records: vec![
            record("revoked", RevocationKind::Revoked),
            record("unclear", RevocationKind::Invalid),
        ],
        complete: vec![(b.clone(), locations.clone())],
    };
    let dbtx = client.transaction().await?;
    store_revocations(&dbtx, &partial).await?;
    dbtx.commit().await?;
    assert_eq!(
        checked(&client, &a).await?,
        Some((seeded, locations.clone()))
    );
    let (b_checked, _) = checked(&client, &b).await?.expect("B was read in full");
    assert!(chrono::Utc::now() - b_checked < chrono::TimeDelta::minutes(1));

    // Complete for A, from where its authority now says
    let moved = vec!["wss://relay.other/".to_owned()];
    let full = RevocationLookup {
        records: vec![
            record("revoked", RevocationKind::Invalid),
            record("unclear", RevocationKind::Revoked),
        ],
        complete: vec![(a.clone(), moved.clone())],
    };
    let dbtx = client.transaction().await?;
    store_revocations(&dbtx, &full).await?;
    dbtx.commit().await?;
    let (a_checked, a_locations) = checked(&client, &a).await?.unwrap();
    assert!(a_checked > seeded);
    assert_eq!(a_locations, moved);

    let states: Vec<(String, String)> = client
        .query(
            "SELECT credential_digest, state FROM revocations ORDER BY credential_digest",
            &[],
        )
        .await?
        .iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect();
    assert_eq!(
        states,
        [
            ("revoked".to_owned(), "revoked".to_owned()),
            ("unclear".to_owned(), "revoked".to_owned()),
        ]
    );
    drop_schema(client, driver, &schema).await
}

/// Reads the production issuers' revocations from where their pinned
/// authorities say, as each sync does
#[tokio::test]
#[ignore = "reads Manifold's production revocation relay over the network"]
async fn live_revocation_lookup_completes() -> anyhow::Result<()> {
    // As the server does at startup
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let trusted = parse_trusted_issuers(None)?;
    let targets = revocation_targets(&trusted)?;
    let pool = RelayPool::new(RelayPoolOptions::default());
    for relay in targets.keys() {
        pool.add_relay(relay.as_str(), RelayOptions::default())
            .await?;
    }
    pool.connect(Some(Duration::from_secs(10))).await;
    let lookup = fetch_revocations(&pool, &targets, &trusted).await;
    println!(
        "read in full: {:?}, revocations: {:?}",
        lookup.complete, lookup.records
    );
    assert_eq!(lookup.complete.len(), trusted.len());
    Ok(())
}
