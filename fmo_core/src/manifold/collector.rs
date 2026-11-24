use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::time::Duration;

use anyhow::{bail, ensure, Context};
use bitcoin::hashes::{sha256, Hash, HashEngine};
use nostr_sdk::pool::RelayNotification;
use nostr_sdk::{
    Event, EventId, Filter, FilterOptions, Kind, Relay, RelayMessage, RelayOptions, RelayPool,
    RelayPoolOptions, RelayStatus, SubscribeAutoCloseOptions, SubscribeOptions, SubscriptionId,
    Timestamp,
};
use serde_json::Value;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, info, warn};

use super::discovery::{
    evaluate_credential, kind_tags, verify_event, CredentialEvidence, VerifiedAdvertisement,
    FLIP_KIND, MG_LEGACY_KIND, MG_V2_KIND,
};
use super::peerbadge::{
    admit_revocation, check_pinned_authorities, pinned_authorities_fingerprint, revocation_targets,
    RevocationKind, RevocationRecord, REVOCATION_KIND,
};
use crate::federation::observer::FederationObserver;

pub const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.dev.fedibtc.com",
    "wss://relay.primal.net",
    "wss://relay.damus.io/",
];
pub const RELAYS_ENV: &str = "FO_MANIFOLD_RELAYS";
pub const TRUSTED_ISSUERS_ENV: &str = "FO_MANIFOLD_TRUSTED_ISSUERS";

pub const MANIFOLD_ISSUERS_REVISION: &str = "141d6b95ccf8ebaeec6788bdf0ae2a6f9d8543d0";

pub const MANIFOLD_PRODUCTION_ISSUERS: &[&str] = &[
    "ac9647df59ada5c432ce7ff9a6afbac4507d29cdf6ed9e9dd26010e641ff9aeb",
    "db37f7c6399b2eba7a5ec0cf8258c556406bf881db35e8fd0fe1499755f4850b",
    "8cdf45bcfe110b128f5cff355b24cb85f549e79de879aa36915570fdf9b50b02",
    "f6c7bc122e74acef6101d2e343a7e68a0de3d9dc9f357c79276743b86ffdb283",
    "96eb3609d9cb7c07da825e43a7fa73e7e8020c999c5de00b3333476c58d840e4",
    "ef245cb47056133e595cd54d66de8bddbe71ba8583ac2c2eede78f951981e7f6",
    "c3cfb8370910591401f31937da09ffedc0ed4ff64e46772ba7c3e267784a4c1a",
    "5546e9caca6b8a474da9da8799f9f8f988888018b42429ebe00dd4b9cd2f6a31",
];

const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const FETCH_LIMIT: usize = 2000;
const RETENTION_DAYS: i64 = 30;
const PRESENCE_RETENTION_DAYS: i64 = 90;
pub(crate) const MAX_CLOCK_SKEW_SECONDS: u64 = 5 * 60;
const MAX_FETCHED_AUTHORS: usize = 2000;
const AUTHORS_PER_FETCH: usize = 100;
const EVENTS_PER_FETCHED_AUTHOR: usize = 4;
/// Revocations asked for per page.
const REVOCATION_PAGE_LIMIT: usize = 500;
const MAX_REVOCATION_PAGES: usize = 40;
const DERIVE_BATCH: i64 = 200;
const MAX_DERIVE_BATCHES: usize = 100;
/// Bump when persisted listing or credential evidence changes shape or meaning.
const DERIVATION_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Retention {
    pub max_age_seconds: i64,
    pub max_per_kind: i64,
    pub max_protected_per_kind: i64,
    pub max_versions_per_author: i64,
}

impl Retention {
    pub(crate) const DEFAULT: Retention = Retention {
        max_age_seconds: RETENTION_DAYS * 86400,
        max_per_kind: 5000,
        max_protected_per_kind: 1000,
        max_versions_per_author: 50,
    };

    /// The most advertisements kept of each kind
    pub(crate) fn max_rows_per_kind(&self) -> i64 {
        self.max_per_kind
            .saturating_add(self.max_protected_per_kind)
    }
}

pub(crate) const MIGRATIONS: &[&str] = &[
    r"
CREATE TABLE advertisements (
    kind INTEGER NOT NULL,
    -- Lowercase x-only hex, the event author
    author TEXT NOT NULL,
    event_id TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    issued_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    -- The verified, signed payload
    payload JSONB NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (kind, author)
);
CREATE INDEX advertisements_expires_at ON advertisements (expires_at);

-- Each Fedimint and MG version a guardian advertised, and when first
CREATE TABLE version_changes (
    author TEXT NOT NULL,
    fedimintd_version TEXT NOT NULL,
    mg_version TEXT NOT NULL,
    first_seen BIGINT NOT NULL,
    PRIMARY KEY (author, fedimintd_version, mg_version)
);

-- Syncs per guardian and UTC day, and of those the ones that found a fresh
-- advertisement
CREATE TABLE presence_daily (
    author TEXT NOT NULL,
    day DATE NOT NULL,
    polls INTEGER NOT NULL,
    fresh_polls INTEGER NOT NULL,
    PRIMARY KEY (author, day)
);

CREATE TABLE sync_state (
    singleton BOOLEAN PRIMARY KEY DEFAULT true CHECK (singleton),
    last_success TIMESTAMPTZ NOT NULL,
    relays TEXT[] NOT NULL
);
",
    r"
-- What the trusted PeerBadge issuers published about credentials, from their
-- revocation locations. Revocations are permanent, so rows are never removed.
CREATE TABLE revocations (
    -- Lowercase x-only hex
    issuer TEXT NOT NULL,
    -- Unpadded base64url SHA-256, as in the event's d tag
    credential_digest TEXT NOT NULL,
    -- 'revoked': a revocation whose issuer proof verifies;
    -- 'invalid': only events for this credential that do not verify
    state TEXT NOT NULL CHECK (state IN ('revoked', 'invalid')),
    event_id TEXT NOT NULL,
    first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (issuer, credential_digest)
);
-- When every revocation location last answered completely
ALTER TABLE sync_state ADD COLUMN revocations_checked TIMESTAMPTZ;
",
    r"
-- Per issuer: when every revocation location its pinned authority names last
-- answered in full, and which locations those were. Completeness is per
-- issuer: a lookup that covered other issuers says nothing about this one.
CREATE TABLE revocation_checks (
    -- Lowercase x-only hex
    issuer TEXT PRIMARY KEY,
    -- Normalized relay URLs, sorted
    locations TEXT[] NOT NULL,
    checked_at TIMESTAMPTZ NOT NULL
);
-- One time for every issuer could vouch for issuers it never covered
ALTER TABLE sync_state DROP COLUMN revocations_checked;
CREATE INDEX advertisements_kind_created_at ON advertisements (kind, created_at DESC);
",
    r"
-- What the collector derives from each stored event, so requests neither
-- verify credentials nor read payloads. Rows whose derived_stamp is not the
-- running binary's are not listed until the collector derives them again.
ALTER TABLE advertisements
    ADD COLUMN derived_stamp TEXT,
    -- The bounded view the directory lists
    ADD COLUMN listing JSONB,
    -- The credential's checks that do not change over time
    ADD COLUMN credential JSONB,
    -- The credential and its advertiser, hashed: a republished advertisement
    -- reuses the evidence of the same credential instead of verifying again
    ADD COLUMN credential_key TEXT,
    -- Lowercase issuer and digest wire form, as revocations are stored
    ADD COLUMN credential_issuer TEXT,
    ADD COLUMN credential_digest TEXT,
    -- Genuine, issued under a pinned authority, authorizing this advertiser
    ADD COLUMN credential_sound BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX advertisements_author ON advertisements (author);
CREATE INDEX advertisements_credential_key ON advertisements (credential_key);
CREATE INDEX presence_daily_day ON presence_daily (day);
",
];

pub fn configured_relays() -> Vec<String> {
    match std::env::var(RELAYS_ENV) {
        Ok(value) => value
            .split(',')
            .map(str::trim)
            .filter(|r| !r.is_empty())
            .map(str::to_owned)
            .collect(),
        Err(_) => DEFAULT_RELAYS.iter().map(|r| (*r).to_owned()).collect(),
    }
}

pub fn parse_trusted_issuers(setting: Option<&str>) -> anyhow::Result<Vec<String>> {
    let Some(setting) = setting else {
        return Ok(MANIFOLD_PRODUCTION_ISSUERS
            .iter()
            .map(|key| (*key).to_owned())
            .collect());
    };
    setting
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(|key| {
            ensure!(
                key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
                "{TRUSTED_ISSUERS_ENV}: {key:?} is not an x-only public key in hex (64 characters)"
            );
            Ok(key.to_lowercase())
        })
        .collect()
}

/// Checked when the observer starts.
pub fn check_trusted_issuers() -> anyhow::Result<()> {
    check_pinned_authorities()?;
    let setting = std::env::var(TRUSTED_ISSUERS_ENV).ok();
    let issuers = parse_trusted_issuers(setting.as_deref())?;
    match setting {
        None => info!(
            issuers = issuers.len(),
            revision = MANIFOLD_ISSUERS_REVISION,
            "Trusting Manifold's production PeerBadge issuers"
        ),
        Some(_) => info!(
            issuers = issuers.len(),
            "Trusting the PeerBadge issuers in {TRUSTED_ISSUERS_ENV}"
        ),
    }
    Ok(())
}

pub fn trusted_issuers() -> Vec<String> {
    parse_trusted_issuers(std::env::var(TRUSTED_ISSUERS_ENV).ok().as_deref()).unwrap_or_default()
}

fn unix_now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

/// What stored derivations were worked out with.
pub(crate) fn derivation_stamp() -> String {
    format!("v{DERIVATION_VERSION}-{}", pinned_authorities_fingerprint())
}

pub(crate) async fn check_manifold_schema(client: &tokio_postgres::Client) -> anyhow::Result<()> {
    let applied = client
        .query_opt(
            "SELECT migrations_applied FROM module_schema_versions WHERE module_kind = 'manifold'",
            &[],
        )
        .await?
        .map_or(0, |row| row.get::<_, i32>(0));
    ensure!(
        usize::try_from(applied).is_ok_and(|applied| applied == MIGRATIONS.len()),
        "The database ran {applied} Manifold migrations but this binary needs {}; start the worker to migrate the database before API replicas",
        MIGRATIONS.len()
    );
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct Collected {
    pub ad: VerifiedAdvertisement,
    pub derived: Derived,
}

#[derive(Debug, Clone)]
pub(crate) struct Derived {
    pub listing: Value,
    pub evidence: CredentialEvidence,
    pub credential_key: Option<String>,
}

pub(crate) fn credential_key(ad: &VerifiedAdvertisement) -> Option<String> {
    let envelope = ad
        .payload
        .get("holder_authorizations")?
        .as_array()?
        .first()?;
    let mut engine = sha256::Hash::engine();
    engine.input(ad.author.as_bytes());
    engine.input(b"\0");
    engine.input(envelope.to_string().as_bytes());
    Some(hex::encode(
        sha256::Hash::from_engine(engine).to_byte_array(),
    ))
}

pub(crate) fn derive(ad: VerifiedAdvertisement, evidence: Option<CredentialEvidence>) -> Collected {
    let listing = super::listing::listing(&ad).unwrap_or_else(|error| {
        warn!(%error, event = %ad.event_id, "Could not serialize a Manifold listing");
        Value::Null
    });
    let derived = Derived {
        listing,
        evidence: evidence.unwrap_or_else(|| evaluate_credential(&ad.payload)),
        credential_key: credential_key(&ad),
    };
    Collected { ad, derived }
}

const SCHEMA: &str = "fmo_manifold";

async fn manifold_transaction<'a>(
    client: &'a mut tokio_postgres::Client,
    schema: &str,
) -> anyhow::Result<tokio_postgres::Transaction<'a>> {
    let dbtx = client.transaction().await?;
    dbtx.batch_execute(&format!("SET LOCAL search_path TO {schema}, public"))
        .await?;
    Ok(dbtx)
}

/// Derives `ads`.
pub(crate) async fn derive_all(
    client: &mut tokio_postgres::Client,
    schema: &str,
    ads: Vec<VerifiedAdvertisement>,
) -> anyhow::Result<Vec<Collected>> {
    let keys: Vec<String> = ads.iter().filter_map(credential_key).collect();
    let dbtx = manifold_transaction(client, schema).await?;
    let known: HashMap<String, CredentialEvidence> = dbtx
        .query(
            "SELECT DISTINCT ON (credential_key) credential_key, credential FROM advertisements
             WHERE credential_key = ANY($1) AND derived_stamp = $2 AND credential IS NOT NULL",
            &[&keys, &derivation_stamp()],
        )
        .await?
        .into_iter()
        .filter_map(|row| {
            let evidence = serde_json::from_value(row.get(1)).ok()?;
            Some((row.get(0), evidence))
        })
        .collect();
    dbtx.commit().await?;
    tokio::task::spawn_blocking(move || {
        ads.into_iter()
            .map(|ad| {
                let evidence = credential_key(&ad).and_then(|key| known.get(&key).cloned());
                derive(ad, evidence)
            })
            .collect()
    })
    .await
    .context("Deriving Manifold advertisements panicked")
}

/// Returns how many were updated.
pub(crate) async fn rederive_stale(
    client: &mut tokio_postgres::Client,
    schema: &str,
) -> anyhow::Result<usize> {
    let stamp = derivation_stamp();
    let mut updated = 0;
    for _ in 0..MAX_DERIVE_BATCHES {
        let dbtx = manifold_transaction(client, schema).await?;
        let rows = dbtx
            .query(
                "SELECT kind, author, event_id, created_at, issued_at, expires_at, payload
                 FROM advertisements WHERE derived_stamp IS DISTINCT FROM $1
                 ORDER BY kind, author LIMIT $2",
                &[&stamp, &DERIVE_BATCH],
            )
            .await?;
        dbtx.commit().await?;
        let batch = rows.len();
        let ads: Vec<VerifiedAdvertisement> = rows
            .into_iter()
            .map(|row| {
                Ok(VerifiedAdvertisement {
                    kind: u16::try_from(row.get::<_, i32>(0))?,
                    author: row.get(1),
                    event_id: row.get(2),
                    created_at: u64::try_from(row.get::<_, i64>(3))?,
                    issued_at: u64::try_from(row.get::<_, i64>(4))?,
                    expires_at: u64::try_from(row.get::<_, i64>(5))?,
                    payload: row.get(6),
                })
            })
            .collect::<anyhow::Result<_>>()?;
        let collected = derive_all(client, schema, ads).await?;
        let dbtx = manifold_transaction(client, schema).await?;
        for item in &collected {
            let params = derived_params(item, &stamp)?;
            dbtx.execute(
                "UPDATE advertisements
                 SET derived_stamp = $4, listing = $5, credential = $6, credential_key = $7,
                     credential_issuer = $8, credential_digest = $9, credential_sound = $10
                 WHERE kind = $1 AND author = $2 AND event_id = $3",
                &params.with_key(),
            )
            .await?;
        }
        dbtx.commit().await?;
        updated += batch;
        if batch < usize::try_from(DERIVE_BATCH)? {
            break;
        }
    }
    Ok(updated)
}

struct DerivedParams {
    kind: i32,
    author: String,
    event_id: String,
    stamp: String,
    listing: Value,
    credential: Value,
    credential_key: Option<String>,
    credential_issuer: Option<String>,
    credential_digest: Option<String>,
    credential_sound: bool,
}

impl DerivedParams {
    /// Key columns, then the derived ones
    fn with_key(&self) -> [&(dyn tokio_postgres::types::ToSql + Sync); 10] {
        [
            &self.kind,
            &self.author,
            &self.event_id,
            &self.stamp,
            &self.listing,
            &self.credential,
            &self.credential_key,
            &self.credential_issuer,
            &self.credential_digest,
            &self.credential_sound,
        ]
    }
}

fn derived_params(item: &Collected, stamp: &str) -> anyhow::Result<DerivedParams> {
    let evidence = &item.derived.evidence;
    Ok(DerivedParams {
        kind: i32::from(item.ad.kind),
        author: item.ad.author.clone(),
        event_id: item.ad.event_id.clone(),
        stamp: stamp.to_owned(),
        listing: item.derived.listing.clone(),
        credential: serde_json::to_value(evidence)?,
        credential_key: item.derived.credential_key.clone(),
        credential_issuer: evidence.issuer_key(),
        credential_digest: evidence.digest.clone(),
        credential_sound: evidence.sound_for(&item.ad.author),
    })
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Protection {
    pub seat_bound: Vec<String>,
    pub trusted: Vec<String>,
}

pub(crate) async fn credentialed_authors(
    dbtx: &tokio_postgres::Transaction<'_>,
    trusted: &[String],
    limit: i64,
) -> anyhow::Result<Vec<String>> {
    Ok(dbtx
        .query(
            "SELECT DISTINCT author FROM advertisements a
             WHERE credential_sound AND credential_issuer = ANY($1)
               AND NOT EXISTS (
                   SELECT 1 FROM revocations r
                   WHERE r.issuer = a.credential_issuer
                     AND r.credential_digest = a.credential_digest AND r.state = 'revoked')
             ORDER BY author LIMIT $2",
            &[&trusted, &limit],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect())
}

impl FederationObserver {
    /// Creates `fmo_manifold` and runs its migrations.
    pub(crate) async fn setup_manifold_schema(&self) -> anyhow::Result<()> {
        let mut connection = self.connection().await?;
        connection
            .batch_execute("CREATE SCHEMA IF NOT EXISTS fmo_manifold")
            .await?;
        let applied = connection
            .query_opt(
                "SELECT migrations_applied FROM module_schema_versions WHERE module_kind = 'manifold'",
                &[],
            )
            .await?
            .map_or(0, |row| row.get::<_, i32>(0) as usize);
        ensure!(
            applied <= MIGRATIONS.len(),
            "Manifold schema ran {applied} migrations but this binary only knows {}; use a compatible observer version",
            MIGRATIONS.len()
        );
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(applied) {
            info!("Running Manifold migration {}", index + 1);
            let dbtx = connection.transaction().await?;
            dbtx.batch_execute(
                "SET LOCAL statement_timeout = 0; SET LOCAL lock_timeout = '30s';
                 SET LOCAL search_path TO fmo_manifold, public",
            )
            .await?;
            dbtx.batch_execute(sql).await?;
            dbtx.execute(
                "INSERT INTO module_schema_versions (module_kind, migrations_applied) VALUES ('manifold', $1)
                 ON CONFLICT (module_kind) DO UPDATE SET migrations_applied = EXCLUDED.migrations_applied",
                &[&(index as i32 + 1)],
            )
            .await?;
            dbtx.commit().await?;
        }
        Ok(())
    }

    pub(crate) async fn sync_manifold_discovery(self, relays: Vec<String>) -> anyhow::Result<()> {
        let pool = RelayPool::new(RelayPoolOptions::default());
        for relay in &relays {
            pool.add_relay(relay.as_str(), RelayOptions::default())
                .await?;
        }
        pool.connect(Some(Duration::from_secs(5))).await;
        let trusted = trusted_issuers();
        let revocation_targets = revocation_targets(&trusted)?;
        let revocation_pool = RelayPool::new(RelayPoolOptions::default());
        for relay in revocation_targets.keys() {
            revocation_pool
                .add_relay(relay.as_str(), RelayOptions::default())
                .await?;
        }
        revocation_pool.connect(Some(Duration::from_secs(5))).await;
        info!(
            ?relays,
            revocation_relays = ?revocation_targets.keys().collect::<Vec<_>>(),
            "Started Manifold discovery"
        );

        let mut interval = tokio::time::interval(SYNC_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let revocations =
                fetch_revocations(&revocation_pool, &revocation_targets, &trusted).await;
            if let Err(error) = self
                .sync_manifold_once(&pool, &relays, &revocations, &trusted)
                .await
            {
                // One failed sync waits for the next.
                warn!(%error, "Manifold discovery sync failed");
            }
        }
    }

    /// The guardians fetched by author and kept apart by retention
    async fn manifold_protection(
        &self,
        trusted: &[String],
    ) -> anyhow::Result<(Protection, Vec<String>)> {
        let seat_bound: Vec<String> = super::api::seats_by_guardian(self)
            .await?
            .into_keys()
            .collect();
        let mut connection = self.connection().await?;
        let dbtx = manifold_transaction(&mut connection, SCHEMA).await?;
        let credentialed =
            credentialed_authors(&dbtx, trusted, i64::try_from(MAX_FETCHED_AUTHORS)?).await?;
        dbtx.commit().await?;
        let mut fetched = seat_bound.clone();
        fetched.extend(credentialed);
        let mut seen = std::collections::HashSet::new();
        fetched.retain(|author| seen.insert(author.clone()));
        fetched.truncate(MAX_FETCHED_AUTHORS);
        Ok((
            Protection {
                seat_bound,
                trusted: trusted.to_vec(),
            },
            fetched,
        ))
    }

    async fn sync_manifold_once(
        &self,
        pool: &RelayPool,
        relays: &[String],
        revocations: &RevocationLookup,
        trusted: &[String],
    ) -> anyhow::Result<()> {
        let (protection, known_authors) = self.manifold_protection(trusted).await?;
        let since =
            nostr_sdk::Timestamp::from(unix_now().saturating_sub(RETENTION_DAYS as u64 * 86400));
        let mut events = pool
            .get_events_of(
                vec![
                    Filter::new()
                        .kinds([Kind::Custom(MG_LEGACY_KIND), Kind::Custom(MG_V2_KIND)])
                        .since(since)
                        .limit(FETCH_LIMIT),
                    Filter::new()
                        .kind(Kind::Custom(FLIP_KIND))
                        .custom_tag(
                            nostr_sdk::SingleLetterTag::lowercase(nostr_sdk::Alphabet::T),
                            ["fedi-flip"],
                        )
                        .since(since)
                        .limit(FETCH_LIMIT),
                ],
                FETCH_TIMEOUT,
                FilterOptions::ExitOnEOSE,
            )
            .await?;
        let latest = events.len();
        events.extend(fetch_by_authors(pool, &known_authors, since).await);
        ensure!(!events.is_empty(), "No relay returned any Manifold event");

        let now = unix_now();
        let (newest, rejected) = select_newest(&events, now);

        let mut connection = self.connection().await?;
        let collected = derive_all(&mut connection, SCHEMA, newest).await?;
        let mut dbtx = manifold_transaction(&mut connection, SCHEMA).await?;
        let outcome = store_sync(
            &mut dbtx,
            &collected,
            relays,
            now,
            Retention::DEFAULT,
            &protection,
        )
        .await?;
        store_revocations(&dbtx, revocations).await?;
        dbtx.commit().await?;
        let rederived = rederive_stale(&mut connection, SCHEMA).await?;
        let rejected = rejected + outcome.refused;
        info!(
            events = events.len(),
            latest,
            known_authors = known_authors.len(),
            stored = outcome.stored,
            rejected,
            rederived,
            revocations = revocations.records.len(),
            revocation_issuers_read = revocations.complete.len(),
            "Manifold discovery synced"
        );
        Ok(())
    }
}

pub(crate) fn select_newest(events: &[Event], now: u64) -> (Vec<VerifiedAdvertisement>, usize) {
    let mut newest = HashMap::new();
    let mut rejected = 0usize;
    for event in events {
        match verify_event(event) {
            Ok(ad) if ad.created_at <= now.saturating_add(MAX_CLOCK_SKEW_SECONDS) => {
                let key = (ad.kind, ad.author.clone());
                let replace = newest
                    .get(&key)
                    .is_none_or(|current: &VerifiedAdvertisement| {
                        (ad.created_at, std::cmp::Reverse(&ad.event_id))
                            > (current.created_at, std::cmp::Reverse(&current.event_id))
                    });
                if replace {
                    newest.insert(key, ad);
                }
            }
            Ok(ad) => {
                debug!(author = %ad.author, created_at = ad.created_at, "Ignoring a Manifold advertisement dated in the future");
                rejected += 1;
            }
            Err(error) => {
                debug!(?error, id = %event.id(), "Ignoring discovery event");
                rejected += 1;
            }
        }
    }
    (newest.into_values().collect(), rejected)
}

/// The advertisements of `authors`, in bounded requests.
async fn fetch_by_authors(pool: &RelayPool, authors: &[String], since: Timestamp) -> Vec<Event> {
    let identifiers: Vec<&str> = [MG_LEGACY_KIND, MG_V2_KIND, FLIP_KIND]
        .into_iter()
        .filter_map(kind_tags)
        .map(|(d, _)| d)
        .collect();
    let mut events = Vec::new();
    for chunk in authors.chunks(AUTHORS_PER_FETCH) {
        let keys: Vec<nostr_sdk::PublicKey> = chunk
            .iter()
            .filter_map(|author| nostr_sdk::PublicKey::from_hex(author).ok())
            .collect();
        let filter = Filter::new()
            .kinds([
                Kind::Custom(MG_LEGACY_KIND),
                Kind::Custom(MG_V2_KIND),
                Kind::Custom(FLIP_KIND),
            ])
            .authors(keys)
            .identifiers(identifiers.iter().copied())
            .since(since)
            .limit(chunk.len() * EVENTS_PER_FETCHED_AUTHOR);
        match pool
            .get_events_of(vec![filter], FETCH_TIMEOUT, FilterOptions::ExitOnEOSE)
            .await
        {
            Ok(found) => events.extend(found),
            Err(error) => {
                warn!(%error, authors = chunk.len(), "Could not fetch known Manifold guardians' advertisements");
            }
        }
    }
    events
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StoreOutcome {
    pub stored: usize,
    pub refused: usize,
}

pub(crate) async fn store_sync(
    dbtx: &mut tokio_postgres::Transaction<'_>,
    ads: &[Collected],
    relays: &[String],
    now: u64,
    retention: Retention,
    protection: &Protection,
) -> anyhow::Result<StoreOutcome> {
    let mut outcome = StoreOutcome {
        stored: 0,
        refused: 0,
    };
    let stamp = derivation_stamp();
    for item in ads {
        let savepoint = dbtx.transaction().await?;
        match store_advertisement(&savepoint, item, &stamp).await {
            Ok(()) => {
                savepoint.commit().await?;
                outcome.stored += 1;
            }
            Err(error) => {
                savepoint.rollback().await?;
                warn!(%error, author = %item.ad.author, event = %item.ad.event_id, "Skipping a Manifold advertisement the database refused");
                outcome.refused += 1;
            }
        }
    }

    let now = i64::try_from(now)?;
    dbtx.execute(
        "INSERT INTO presence_daily (author, day, polls, fresh_polls)
         SELECT author, (now() AT TIME ZONE 'UTC')::date, 1,
                MAX(CASE WHEN issued_at <= $1 AND expires_at > $1 AND $1 - issued_at <= $2 THEN 1 ELSE 0 END)
         FROM advertisements
         WHERE kind IN (37701, 37709) AND created_at >= $1 - $3
         GROUP BY author
         ON CONFLICT (author, day) DO UPDATE
             SET polls = presence_daily.polls + 1,
                 fresh_polls = presence_daily.fresh_polls + EXCLUDED.fresh_polls",
        &[
            &now,
            &(super::discovery::MAX_AD_AGE_SECONDS as i64),
            &(RETENTION_DAYS * 86400),
        ],
    )
    .await?;
    dbtx.execute(
        "DELETE FROM presence_daily WHERE day < (now() AT TIME ZONE 'UTC')::date - $1::int",
        &[&(PRESENCE_RETENTION_DAYS as i32)],
    )
    .await?;
    // The observer's own horizon: by creation time, never by claimed expiry
    let mut gone: Vec<String> = dbtx
        .query(
            "DELETE FROM advertisements WHERE created_at < $1 RETURNING author",
            &[&(now - retention.max_age_seconds)],
        )
        .await?
        .iter()
        .map(|row| row.get(0))
        .collect();
    gone.extend(
        dbtx.query(
            "DELETE FROM advertisements a USING (
                 SELECT kind, author FROM (
                     SELECT kind, author, protected,
                            row_number() OVER (PARTITION BY kind, protected
                                               ORDER BY created_at DESC, author) AS rank
                     FROM (
                         SELECT kind, author, created_at,
                                COALESCE(author = ANY($2)
                                    OR (credential_sound AND credential_issuer = ANY($3)
                                        AND NOT EXISTS (
                                            SELECT 1 FROM revocations r
                                            WHERE r.issuer = advertisements.credential_issuer
                                              AND r.credential_digest = advertisements.credential_digest
                                              AND r.state = 'revoked')),
                                    false) AS protected
                         FROM advertisements
                     ) flagged
                 ) ranked
                 WHERE rank > CASE WHEN protected THEN $4::bigint ELSE $1::bigint END
             ) excess
             WHERE a.kind = excess.kind AND a.author = excess.author
             RETURNING a.author",
            &[
                &retention.max_per_kind,
                &protection.seat_bound,
                &protection.trusted,
                &retention.max_protected_per_kind,
            ],
        )
        .await?
        .iter()
        .map(|row| row.get::<_, String>(0)),
    );
    gone.sort();
    gone.dedup();
    dbtx.execute(
        "DELETE FROM presence_daily p
         WHERE p.author = ANY($1) AND NOT p.author = ANY($2)
           AND NOT EXISTS (SELECT 1 FROM advertisements a WHERE a.author = p.author)",
        &[&gone, &protection.seat_bound],
    )
    .await?;
    dbtx.execute(
        "DELETE FROM version_changes v
         WHERE NOT EXISTS (SELECT 1 FROM advertisements a WHERE a.author = v.author)",
        &[],
    )
    .await?;
    dbtx.execute(
        "DELETE FROM version_changes v USING (
             SELECT author, fedimintd_version, mg_version FROM (
                 SELECT author, fedimintd_version, mg_version,
                        row_number() OVER (PARTITION BY author
                            ORDER BY first_seen DESC, fedimintd_version, mg_version) AS rank
                 FROM version_changes
             ) ranked WHERE rank > $1
         ) excess
         WHERE v.author = excess.author AND v.fedimintd_version = excess.fedimintd_version
           AND v.mg_version = excess.mg_version",
        &[&retention.max_versions_per_author],
    )
    .await?;
    dbtx.execute(
        "INSERT INTO sync_state (singleton, last_success, relays) VALUES (true, now(), $1)
         ON CONFLICT (singleton) DO UPDATE SET last_success = now(), relays = EXCLUDED.relays",
        &[&relays],
    )
    .await?;
    Ok(outcome)
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RevocationLookup {
    pub records: Vec<RevocationRecord>,
    pub complete: Vec<(String, Vec<String>)>,
}

async fn fetch_revocations(
    pool: &RelayPool,
    targets: &BTreeMap<String, Vec<String>>,
    trusted: &[String],
) -> RevocationLookup {
    let mut found: BTreeMap<(String, String), RevocationRecord> = BTreeMap::new();
    let mut answered = std::collections::BTreeSet::new();
    for (relay, issuers) in targets {
        let authors: Vec<nostr_sdk::PublicKey> = issuers
            .iter()
            .filter_map(|issuer| nostr_sdk::PublicKey::from_hex(issuer).ok())
            .collect();
        let filter = Filter::new()
            .kind(Kind::Custom(REVOCATION_KIND))
            .authors(authors);
        let answer = match pool.relay(relay.as_str()).await {
            Ok(connection) => {
                read_all_pages(|until| {
                    let connection = connection.clone();
                    let mut page = filter.clone().limit(REVOCATION_PAGE_LIMIT);
                    if let Some(until) = until {
                        page = page.until(until);
                    }
                    async move { fetch_to_eose(&connection, page, FETCH_TIMEOUT).await }
                })
                .await
            }
            Err(error) => Err(error.into()),
        };
        match answer {
            Ok(events) => {
                answered.insert(relay.clone());
                for record in events
                    .iter()
                    .filter_map(|event| admit_revocation(event, trusted))
                    .filter(|record| issuers.contains(&record.issuer))
                {
                    let key = (record.issuer.clone(), record.credential_digest.clone());
                    let keep_existing = found
                        .get(&key)
                        .is_some_and(|existing| existing.kind == RevocationKind::Revoked);
                    if !keep_existing {
                        found.insert(key, record);
                    }
                }
            }
            Err(error) => {
                warn!(%error, %relay, "Could not read PeerBadge revocations in full");
            }
        }
    }
    RevocationLookup {
        records: found.into_values().collect(),
        complete: complete_issuers(trusted, &answered),
    }
}

/// Reads relay events newest-first until a page adds nothing new.
/// Inclusive timestamps prevent skips; ambiguous capped pages fail closed.
pub(crate) async fn read_all_pages<F, Fut>(mut fetch_page: F) -> anyhow::Result<Vec<Event>>
where
    F: FnMut(Option<Timestamp>) -> Fut,
    Fut: Future<Output = anyhow::Result<Vec<Event>>>,
{
    let mut read: HashMap<EventId, Event> = HashMap::new();
    let mut until: Option<Timestamp> = None;
    let mut page_size = REVOCATION_PAGE_LIMIT;
    let mut previous_page: Option<usize> = None;
    for _ in 0..MAX_REVOCATION_PAGES {
        let page = fetch_page(until).await?;
        let length = page.len();
        let oldest = page.iter().map(Event::created_at).min();
        let within_one_second =
            until.is_some() && page.iter().all(|e| Some(e.created_at()) == until);
        let mut new = 0usize;
        for event in page {
            if read.insert(event.id(), event).is_none() {
                new += 1;
            }
        }
        if new == 0 {
            ensure!(
                !(length > 0 && within_one_second && length >= page_size),
                "More events share one second than the relay returns per page"
            );
            return Ok(read.into_values().collect());
        }
        if let Some(previous) = previous_page {
            page_size = page_size.min(previous);
        }
        previous_page = Some(length);
        until = oldest;
    }
    bail!("The relay did not run out of events within {MAX_REVOCATION_PAGES} pages")
}

async fn fetch_to_eose(
    relay: &Relay,
    filter: Filter,
    timeout: Duration,
) -> anyhow::Result<Vec<Event>> {
    let mut notifications = relay.notifications();
    let id = SubscriptionId::generate();
    relay
        .subscribe_with_id(
            id.clone(),
            vec![filter],
            SubscribeOptions::default().close_on(Some(
                SubscribeAutoCloseOptions::default()
                    .filter(FilterOptions::ExitOnEOSE)
                    .timeout(Some(timeout)),
            )),
        )
        .await?;
    let mut events = Vec::new();
    tokio::time::timeout(timeout, async {
        loop {
            match notifications.recv().await {
                Ok(RelayNotification::Message { message }) => match message {
                    RelayMessage::Event {
                        subscription_id,
                        event,
                    } if subscription_id == id => events.push(*event),
                    RelayMessage::EndOfStoredEvents(subscription_id) if subscription_id == id => {
                        return Ok(())
                    }
                    RelayMessage::Closed {
                        subscription_id,
                        message,
                    } if subscription_id == id => {
                        bail!("The relay closed the subscription: {message}")
                    }
                    _ => {}
                },
                Ok(RelayNotification::RelayStatus {
                    status: RelayStatus::Disconnected | RelayStatus::Terminated,
                }) => {
                    bail!("The relay disconnected")
                }
                Ok(RelayNotification::Shutdown) => bail!("The relay shut down"),
                Ok(_) => {}
                Err(RecvError::Lagged(missed)) => {
                    bail!("Missed {missed} relay messages, so the answer may be incomplete")
                }
                Err(RecvError::Closed) => bail!("The relay's notifications ended"),
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("The relay did not finish answering in time"))??;
    Ok(events)
}

/// The trusted issuers whose every revocation location is in `answered`
fn complete_issuers(
    trusted: &[String],
    answered: &std::collections::BTreeSet<String>,
) -> Vec<(String, Vec<String>)> {
    trusted
        .iter()
        .filter_map(|issuer| {
            let authority = super::peerbadge::pinned_authorities().get(issuer)?;
            let locations = super::peerbadge::revocation_locations(authority).ok()?;
            (!locations.is_empty() && locations.iter().all(|relay| answered.contains(relay)))
                .then(|| (issuer.clone(), locations))
        })
        .collect()
}

/// Runs after [`store_sync`], in its transaction.
pub(crate) async fn store_revocations(
    dbtx: &tokio_postgres::Transaction<'_>,
    lookup: &RevocationLookup,
) -> anyhow::Result<()> {
    for record in &lookup.records {
        dbtx.execute(
            "INSERT INTO revocations (issuer, credential_digest, state, event_id)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (issuer, credential_digest) DO UPDATE
                 SET state = EXCLUDED.state, event_id = EXCLUDED.event_id
                 WHERE revocations.state <> 'revoked'",
            &[
                &record.issuer,
                &record.credential_digest,
                &record.kind.as_str(),
                &record.event_id,
            ],
        )
        .await?;
    }
    for (issuer, locations) in &lookup.complete {
        dbtx.execute(
            "INSERT INTO revocation_checks (issuer, locations, checked_at) VALUES ($1, $2, now())
             ON CONFLICT (issuer) DO UPDATE
                 SET locations = EXCLUDED.locations, checked_at = EXCLUDED.checked_at",
            &[issuer, locations],
        )
        .await?;
    }
    Ok(())
}

async fn store_advertisement(
    savepoint: &tokio_postgres::Transaction<'_>,
    item: &Collected,
    stamp: &str,
) -> anyhow::Result<()> {
    let ad = &item.ad;
    let created_at = i64::try_from(ad.created_at)?;
    let issued_at = i64::try_from(ad.issued_at)?;
    let expires_at = i64::try_from(ad.expires_at)?;
    let derived = derived_params(item, stamp)?;
    savepoint
        .execute(
            "INSERT INTO advertisements
                 (kind, author, event_id, derived_stamp, listing, credential, credential_key,
                  credential_issuer, credential_digest, credential_sound,
                  created_at, issued_at, expires_at, payload)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
             ON CONFLICT (kind, author) DO UPDATE
                 SET event_id = EXCLUDED.event_id, created_at = EXCLUDED.created_at,
                     issued_at = EXCLUDED.issued_at, expires_at = EXCLUDED.expires_at,
                     payload = EXCLUDED.payload, last_seen = now(),
                     derived_stamp = EXCLUDED.derived_stamp, listing = EXCLUDED.listing,
                     credential = EXCLUDED.credential, credential_key = EXCLUDED.credential_key,
                     credential_issuer = EXCLUDED.credential_issuer,
                     credential_digest = EXCLUDED.credential_digest,
                     credential_sound = EXCLUDED.credential_sound
                 WHERE advertisements.created_at < EXCLUDED.created_at
                    OR (advertisements.created_at = EXCLUDED.created_at
                        AND EXCLUDED.event_id < advertisements.event_id)",
            &[
                &derived.kind,
                &derived.author,
                &derived.event_id,
                &derived.stamp,
                &derived.listing,
                &derived.credential,
                &derived.credential_key,
                &derived.credential_issuer,
                &derived.credential_digest,
                &derived.credential_sound,
                &created_at,
                &issued_at,
                &expires_at,
                &ad.payload,
            ],
        )
        .await?;
    savepoint
        .execute(
            "UPDATE advertisements SET last_seen = now()
             WHERE kind = $1 AND author = $2 AND event_id = $3",
            &[&i32::from(ad.kind), &ad.author, &ad.event_id],
        )
        .await?;
    if ad.kind != FLIP_KIND {
        let fedimintd = ad
            .payload
            .pointer("/availability/fedimintd_version")
            .and_then(|v| v.as_str());
        let mg = ad
            .payload
            .pointer("/version/mg_version")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some(fedimintd) = fedimintd {
            savepoint
                .execute(
                    "INSERT INTO version_changes (author, fedimintd_version, mg_version, first_seen)
                     VALUES ($1, $2, $3, $4)
                     ON CONFLICT (author, fedimintd_version, mg_version)
                     DO UPDATE SET first_seen = LEAST(version_changes.first_seen, EXCLUDED.first_seen)",
                    &[&ad.author, &fedimintd, &mg, &issued_at],
                )
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
