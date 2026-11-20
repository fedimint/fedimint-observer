use serde_json::{json, Value};

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

type Driver = tokio::task::JoinHandle<Result<(), tokio_postgres::Error>>;

/// A connection whose `search_path` is a fresh schema of the test's own,
/// with every Manifold migration applied, so nothing else is touched
async fn test_schema(name: &str) -> anyhow::Result<(tokio_postgres::Client, Driver, String)> {
    test_schema_at(name, MIGRATIONS.len()).await
}

/// [`test_schema`] with only the first `migrations` applied
async fn test_schema_at(
    name: &str,
    migrations: usize,
) -> anyhow::Result<(tokio_postgres::Client, Driver, String)> {
    let database = std::env::var("FMO_MANIFOLD_TEST_DATABASE")?;
    let (client, connection) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    let driver = tokio::spawn(connection);
    let schema = format!("fmo_manifold_test_{name}_{}", std::process::id());
    client
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema};"
        ))
        .await?;
    for migration in &MIGRATIONS[..migrations] {
        client.batch_execute(migration).await?;
    }
    Ok((client, driver, schema))
}

async fn drop_schema(
    client: tokio_postgres::Client,
    driver: Driver,
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
    sync_protected(client, ads, now, retention, &Protection::default()).await
}

/// One sync of `ads`, derived as the collector derives them
async fn sync_protected(
    client: &mut tokio_postgres::Client,
    ads: &[&VerifiedAdvertisement],
    now: u64,
    retention: Retention,
    protection: &Protection,
) -> anyhow::Result<StoreOutcome> {
    let collected: Vec<Collected> = ads.iter().map(|ad| derive((*ad).clone(), None)).collect();
    let mut dbtx = client.transaction().await?;
    let outcome = store_sync(
        &mut dbtx,
        &collected,
        &["wss://relay.example".to_owned()],
        now,
        retention,
        protection,
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
        max_protected_per_kind: 3,
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

/// Real kind 37701, 37709 and 37702 events captured from Manifold's
/// production relays; the 37709 one carries a production credential
const RELAY_EVENTS: &str = include_str!("../fixtures/relay-events.json");

fn real_events() -> Vec<Event> {
    use nostr_sdk::JsonUtil;
    let fixture: Value = serde_json::from_str(RELAY_EVENTS).unwrap();
    fixture["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| Event::from_json(event.to_string()).unwrap())
        .collect()
}

/// The real v2 advertisement, whose credential verifies
fn credentialed_advertisement() -> VerifiedAdvertisement {
    let ad = verify_event(&real_events()[1]).unwrap();
    assert_eq!(ad.kind, MG_V2_KIND);
    ad
}

/// Before, advertisements could be dated up to two hours ahead, and retention
/// keeps the newest: a flood dated ahead evicted every real guardian
#[test]
fn advertisements_dated_ahead_are_refused() {
    let events = real_events();
    let newest = events
        .iter()
        .map(|event| event.created_at().as_u64())
        .max()
        .unwrap();
    let (kept, rejected) = select_newest(&events, newest - (MAX_CLOCK_SKEW_SECONDS - 60));
    assert_eq!((kept.len(), rejected), (3, 0));
    // A minute past the allowed skew: only the FLIP advertisement, created
    // 15 minutes before the others, is not ahead
    let (kept, rejected) = select_newest(&events, newest - (MAX_CLOCK_SKEW_SECONDS + 60));
    assert_eq!(rejected, 2);
    assert_eq!(
        kept.iter().map(|ad| ad.kind).collect::<Vec<_>>(),
        [FLIP_KIND]
    );
}

/// A relay holding `events` that answers each query with at most `cap`,
/// newest first, ties by lowest id (NIP-01), as relays cap answers below
/// what was asked (relay.primal.net at 500)
struct SimulatedRelay {
    events: Vec<Event>,
    cap: usize,
}

impl SimulatedRelay {
    fn page(&self, until: Option<Timestamp>) -> Vec<Event> {
        let mut matching: Vec<Event> = self
            .events
            .iter()
            .filter(|event| until.is_none_or(|until| event.created_at() <= until))
            .cloned()
            .collect();
        matching.sort_by(|a, b| {
            b.created_at()
                .cmp(&a.created_at())
                .then_with(|| a.id().cmp(&b.id()))
        });
        matching.truncate(self.cap.min(REVOCATION_PAGE_LIMIT));
        matching
    }

    async fn read(&self) -> anyhow::Result<Vec<Event>> {
        read_all_pages(|until| {
            let page = self.page(until);
            async move { Ok(page) }
        })
        .await
    }
}

/// Revocation events created at `times`, one per time
fn revocations_at(times: impl IntoIterator<Item = u64>) -> Vec<Event> {
    let keys = nostr_sdk::Keys::generate();
    times
        .into_iter()
        .enumerate()
        .map(|(n, time)| {
            nostr_sdk::EventBuilder::new(
                Kind::Custom(REVOCATION_KIND),
                "",
                [nostr_sdk::Tag::identifier(format!(
                    "credential-revocation:{n}"
                ))],
            )
            .custom_created_at(Timestamp::from(time))
            .to_event(&keys)
            .unwrap()
        })
        .collect()
}

fn ids(events: &[Event]) -> std::collections::BTreeSet<EventId> {
    events.iter().map(Event::id).collect()
}

/// Before, an answer shorter than the 5000 asked for counted as complete,
/// though relays cap answers at 500 or 4096: a revocation past the cap was
/// never read, and its credential kept showing verified
#[tokio::test]
async fn revocation_reads_page_past_relay_caps() -> anyhow::Result<()> {
    // Distinct seconds, then three events to a second
    for times in [
        (1000..2200).collect::<Vec<u64>>(),
        (0..1200).map(|n| 1000 + n / 3).collect(),
    ] {
        let relay = SimulatedRelay {
            events: revocations_at(times),
            cap: 500,
        };
        assert_eq!(relay.page(None).len(), 500);
        assert_eq!(ids(&relay.read().await?), ids(&relay.events));
    }
    // A relay capping lower than the page asked for is read in full too
    let relay = SimulatedRelay {
        events: revocations_at(1000..1300),
        cap: 70,
    };
    assert_eq!(ids(&relay.read().await?), ids(&relay.events));
    Ok(())
}

#[tokio::test]
async fn small_revocation_reads_complete() -> anyhow::Result<()> {
    for events in [
        Vec::new(),
        revocations_at([1000]),
        revocations_at([1000; 5]),
        revocations_at([1000, 1000, 999, 5]),
    ] {
        let relay = SimulatedRelay { events, cap: 500 };
        assert_eq!(ids(&relay.read().await?), ids(&relay.events));
    }
    Ok(())
}

/// Paging continues from the oldest second read, so it cannot get past a
/// second holding more events than a page: the read fails, never claims to
/// be complete
#[tokio::test]
async fn a_second_fuller_than_a_page_leaves_the_read_incomplete() {
    let crowded = SimulatedRelay {
        events: revocations_at([1000; 600]),
        cap: 500,
    };
    assert!(crowded.read().await.is_err());

    // The relay's lower cap is learned from a page that turned out cut short
    let mut times: Vec<u64> = (2000..2150).collect();
    times.extend([1000; 150]);
    let low_cap = SimulatedRelay {
        events: revocations_at(times),
        cap: 100,
    };
    assert!(low_cap.read().await.is_err());

    // Fewer than a page in that second is fine
    let mut times: Vec<u64> = (2000..2250).collect();
    times.extend([1000; 50]);
    let relay = SimulatedRelay {
        events: revocations_at(times),
        cap: 100,
    };
    assert_eq!(relay.read().await.unwrap().len(), 300);
}

#[tokio::test]
async fn revocation_reads_are_bounded_and_fail_with_any_page() {
    let endless = SimulatedRelay {
        events: revocations_at(0..1000),
        cap: 10,
    };
    assert!(endless.read().await.is_err());

    let relay = SimulatedRelay {
        events: revocations_at(0..1000),
        cap: 500,
    };
    let mut pages = 0;
    let failing = read_all_pages(|until| {
        pages += 1;
        let page = relay.page(until);
        let fail = pages == 2;
        async move {
            if fail {
                bail!("lost the connection");
            }
            Ok(page)
        }
    })
    .await;
    assert!(failing.is_err());
}

fn protection(seat_bound: &[&str]) -> Protection {
    Protection {
        seat_bound: seat_bound.iter().map(|key| (*key).to_owned()).collect(),
        trusted: parse_trusted_issuers(None).unwrap(),
    }
}

/// A flood of advertisements from new keys evicts only its own: seat-bound
/// guardians and guardians with a sound, unrevoked credential from a trusted
/// issuer have a quota of their own
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_protected_guardians_survive_a_flood() -> anyhow::Result<()> {
    let (mut client, driver, schema) = test_schema("flood").await?;
    let retention = Retention {
        max_age_seconds: 86400,
        max_per_kind: 2,
        max_protected_per_kind: 2,
        max_versions_per_author: 2,
    };
    let now = unix_now();
    let seated = advertisement('s', now - 1000, json!({}));
    let credentialed = VerifiedAdvertisement {
        created_at: now - 900,
        ..credentialed_advertisement()
    };
    let flood: Vec<_> = ['f', 'g', 'h', 'i', 'j']
        .into_iter()
        .zip(1u64..)
        .map(|(author, n)| advertisement(author, now - 100 + n, json!({})))
        .collect();
    let mut ads: Vec<&VerifiedAdvertisement> = vec![&seated, &credentialed];
    ads.extend(&flood);
    let guard = protection(&[&seated.author]);
    sync_protected(&mut client, &ads, now, retention, &guard).await?;
    let mut kept = column(&client, "SELECT author FROM advertisements ORDER BY author").await?;
    let mut expected = vec![
        seated.author.clone(),
        credentialed.author.clone(),
        flood[3].author.clone(),
        flood[4].author.clone(),
    ];
    kept.sort();
    expected.sort();
    assert_eq!(kept, expected);

    // Derived when stored: a sound credential, and where to look up its
    // revocation
    let row = client
        .query_one(
            "SELECT credential_sound, credential_issuer, credential_digest, derived_stamp,
                    listing IS NOT NULL
             FROM advertisements WHERE author = $1",
            &[&credentialed.author],
        )
        .await?;
    assert!(row.get::<_, bool>(0));
    let (issuer, digest): (String, String) = (row.get(1), row.get(2));
    assert_eq!(row.get::<_, String>(3), derivation_stamp());
    assert!(row.get::<_, bool>(4));
    let dbtx = client.transaction().await?;
    assert_eq!(
        credentialed_authors(&dbtx, &guard.trusted, 10).await?,
        std::slice::from_ref(&credentialed.author)
    );
    // An untrusted issuer protects no one
    assert!(credentialed_authors(&dbtx, &["00".repeat(32)], 10)
        .await?
        .is_empty());
    dbtx.commit().await?;

    // Once its issuer revoked the credential, the guardian is anonymous, and
    // the next flood evicts it
    client
        .execute(
            "INSERT INTO revocations (issuer, credential_digest, state, event_id)
             VALUES ($1, $2, 'revoked', 'e')",
            &[&issuer, &digest],
        )
        .await?;
    let dbtx = client.transaction().await?;
    assert!(credentialed_authors(&dbtx, &guard.trusted, 10)
        .await?
        .is_empty());
    dbtx.commit().await?;
    sync_protected(&mut client, &[&flood[0]], now, retention, &guard).await?;
    let kept = column(&client, "SELECT author FROM advertisements").await?;
    assert!(!kept.contains(&credentialed.author));
    assert!(kept.contains(&seated.author));
    drop_schema(client, driver, &schema).await
}

/// Presence rows go with a key's last advertisement unless it holds a seat;
/// before, every key that ever advertised kept a row per day for 90 days
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_presence_goes_with_the_last_advertisement() -> anyhow::Result<()> {
    let (mut client, driver, schema) = test_schema("presence").await?;
    let retention = Retention {
        max_age_seconds: 3600,
        ..Retention::DEFAULT
    };
    let now = unix_now();
    let anonymous = advertisement('x', now - 60, json!({}));
    let seated = advertisement('s', now - 60, json!({}));
    let guard = protection(&[&seated.author]);
    sync_protected(&mut client, &[&anonymous, &seated], now, retention, &guard).await?;
    let presence = "SELECT left(author, 1) FROM presence_daily ORDER BY author";
    assert_eq!(column(&client, presence).await?, ["s", "x"]);

    // Both aged out; only the seat-bound guardian's history stays
    sync_protected(&mut client, &[], now + 4000, retention, &guard).await?;
    assert!(column(&client, "SELECT author FROM advertisements")
        .await?
        .is_empty());
    assert_eq!(column(&client, presence).await?, ["s"]);

    // The 90-day cleanup has an index to use
    let indexed = client
        .query_opt(
            "SELECT 1 FROM pg_indexes WHERE schemaname = $1 AND indexname = 'presence_daily_day'",
            &[&schema],
        )
        .await?;
    assert!(indexed.is_some());
    drop_schema(client, driver, &schema).await
}

/// Migration 4 adds the derived columns to a populated table, and the
/// collector derives the rows already there; until then they are not
/// listed, since their stamp is not the binary's
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_existing_advertisements_are_derived_after_migration() -> anyhow::Result<()> {
    let (mut client, driver, schema) = test_schema_at("upgrade", 3).await?;
    let ad = credentialed_advertisement();
    client
        .execute(
            "INSERT INTO advertisements
                 (kind, author, event_id, created_at, issued_at, expires_at, payload)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[
                &i32::from(ad.kind),
                &ad.author,
                &ad.event_id,
                &(ad.created_at as i64),
                &(ad.issued_at as i64),
                &(ad.expires_at as i64),
                &ad.payload,
            ],
        )
        .await?;
    client.batch_execute(MIGRATIONS[3]).await?;
    let derived = "SELECT count(*) FROM advertisements WHERE derived_stamp IS NOT NULL";
    assert_eq!(client.query_one(derived, &[]).await?.get::<_, i64>(0), 0);

    assert_eq!(rederive_stale(&mut client, &schema).await?, 1);
    let row = client
        .query_one(
            "SELECT derived_stamp, listing, credential, credential_sound FROM advertisements",
            &[],
        )
        .await?;
    assert_eq!(row.get::<_, String>(0), derivation_stamp());
    assert_eq!(
        row.get::<_, Value>(1),
        crate::manifold::listing::listing(&ad)?
    );
    let evidence: CredentialEvidence = serde_json::from_value(row.get(2))?;
    assert_eq!(evidence, evaluate_credential(&ad.payload));
    assert!(row.get::<_, bool>(3));
    // Nothing is left to derive
    assert_eq!(rederive_stale(&mut client, &schema).await?, 0);
    drop_schema(client, driver, &schema).await
}

/// A republished advertisement presents the same credential: its stored
/// evidence is reused rather than verified again, unless it was derived
/// with another stamp
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_credential_evidence_is_reused() -> anyhow::Result<()> {
    let (mut client, driver, schema) = test_schema("reuse").await?;
    let ad = credentialed_advertisement();
    sync(&mut client, &[&ad], unix_now(), Retention::DEFAULT).await?;
    // Mark the stored evidence, so reuse shows
    let mut marked = evaluate_credential(&ad.payload);
    marked.schema = Some("marked".to_owned());
    client
        .execute(
            "UPDATE advertisements SET credential = $1",
            &[&serde_json::to_value(&marked)?],
        )
        .await?;
    let republished = VerifiedAdvertisement {
        event_id: "ff".repeat(32),
        created_at: ad.created_at + 1800,
        ..ad.clone()
    };
    let derived = derive_all(&mut client, &schema, vec![republished.clone()]).await?;
    assert_eq!(derived[0].derived.evidence, marked);

    // Evidence derived under another stamp is not trusted
    client
        .execute("UPDATE advertisements SET derived_stamp = 'v0-old'", &[])
        .await?;
    let derived = derive_all(&mut client, &schema, vec![republished]).await?;
    assert_eq!(
        derived[0].derived.evidence,
        evaluate_credential(&ad.payload)
    );
    drop_schema(client, driver, &schema).await
}

/// API-only replicas never migrate: one started before the worker ran the
/// Manifold migrations answered every `/manifold/*` request with an error
#[tokio::test]
#[ignore = "requires an isolated FMO_MANIFOLD_TEST_DATABASE"]
async fn postgres_api_replicas_need_every_manifold_migration() -> anyhow::Result<()> {
    let (client, driver, schema) = test_schema("replica").await?;
    client
        .batch_execute(
            "CREATE TABLE module_schema_versions (
                 module_kind TEXT PRIMARY KEY, migrations_applied INTEGER NOT NULL)",
        )
        .await?;
    assert!(check_manifold_schema(&client).await.is_err());
    let known = MIGRATIONS.len() as i32;
    for (applied, ready) in [(known - 1, false), (known, true), (known + 1, false)] {
        client
            .execute(
                "INSERT INTO module_schema_versions VALUES ('manifold', $1)
                 ON CONFLICT (module_kind) DO UPDATE SET migrations_applied = $1",
                &[&applied],
            )
            .await?;
        assert_eq!(
            check_manifold_schema(&client).await.is_ok(),
            ready,
            "{applied}"
        );
    }
    drop_schema(client, driver, &schema).await
}
