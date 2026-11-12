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

async fn checked_at(
    client: &tokio_postgres::Client,
) -> Result<Option<chrono::DateTime<chrono::Utc>>, tokio_postgres::Error> {
    client
        .query_one("SELECT revocations_checked FROM sync_state", &[])
        .await
        .map(|row| row.get(0))
}

/// One advertisement the database refuses must not cost the others their
/// sync: before each was written under its own savepoint, a single poison
/// event rolled back every advertisement and the day's presence, on every
/// sync that fetched it.
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_poison_advertisements_cannot_block_a_sync() -> anyhow::Result<()> {
    let database = std::env::var("FMO_MANIFOLD_TEST_DATABASE")?;
    let (mut client, driver) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    let driver = tokio::spawn(driver);
    // A schema of the test's own, so nothing else is touched
    let schema = format!("fmo_manifold_test_{}", std::process::id());
    client
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    for migration in MIGRATIONS {
        client.batch_execute(migration).await?;
    }

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
    let relays = vec!["wss://relay.example".to_owned()];

    let outcome = {
        let mut dbtx = client.transaction().await?;
        let outcome = store_sync(&mut dbtx, &[&overflowing, &valid, &nul], &relays, now).await?;
        dbtx.commit().await?;
        outcome
    };
    assert_eq!(
        outcome,
        StoreOutcome {
            stored: 1,
            refused: 2
        }
    );

    let authors: Vec<String> = client
        .query("SELECT author FROM advertisements ORDER BY author", &[])
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    assert_eq!(authors, vec![valid.author.clone()]);
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
    let versions = client
        .query_one(
            "SELECT count(*) FROM version_changes WHERE author = $1",
            &[&valid.author],
        )
        .await?;
    assert_eq!(versions.get::<_, i64>(0), 1);
    let synced: Vec<String> = client
        .query_one("SELECT relays FROM sync_state", &[])
        .await?
        .get(0);
    assert_eq!(synced, relays);

    // Revocations: a valid one wins over invalid events for the same
    // credential, and only a complete lookup moves the check time
    let record = |digest: &str, kind| RevocationRecord {
        issuer: "ab".repeat(32),
        credential_digest: digest.to_owned(),
        kind,
        event_id: format!("{digest}-{}", kind.as_str()),
    };
    assert_eq!(checked_at(&client).await?, None);
    for lookup in [
        RevocationLookup {
            records: vec![
                record("revoked", RevocationKind::Revoked),
                record("unclear", RevocationKind::Invalid),
            ],
            complete: true,
        },
        RevocationLookup {
            records: vec![
                record("revoked", RevocationKind::Invalid),
                record("unclear", RevocationKind::Revoked),
            ],
            complete: false,
        },
    ] {
        let dbtx = client.transaction().await?;
        store_revocations(&dbtx, &lookup).await?;
        dbtx.commit().await?;
        if lookup.complete {
            assert!(checked_at(&client).await?.is_some());
        }
    }
    let first_check = checked_at(&client).await?;
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
    // The incomplete lookup left the check time where the complete one put it
    assert_eq!(checked_at(&client).await?, first_check);

    client
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await?;
    drop(client);
    driver.await??;
    Ok(())
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
        "complete: {}, revocations: {:?}",
        lookup.complete, lookup.records
    );
    assert!(lookup.complete);
    Ok(())
}
