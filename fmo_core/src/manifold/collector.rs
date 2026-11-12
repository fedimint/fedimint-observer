//! The background collector of Manifold discovery events. It is the only
//! code that talks to relays: API handlers read what it stored.
//!
//! Each sync asks the relays for the latest addressable events of the three
//! advertisement kinds, bounded by a per-request limit, verifies every one
//! ([`super::discovery::verify_event`]), and keeps the newest per kind and
//! author. It also records, per Manifold Guardian and UTC day, how many syncs
//! found a fresh advertisement, and each new version it advertised.
//!
//! Each sync also reads the trusted PeerBadge issuers' credential revocations
//! from the revocation locations their pinned authorities name
//! ([`super::peerbadge`]), so credentials can be verified without relay I/O
//! per request.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use anyhow::ensure;
use nostr_sdk::{Filter, FilterOptions, Kind, RelayOptions, RelayPool, RelayPoolOptions};
use tracing::{debug, info, warn};

use super::discovery::{
    verify_event, VerifiedAdvertisement, FLIP_KIND, MAX_AD_AGE_SECONDS, MG_LEGACY_KIND, MG_V2_KIND,
};
use super::peerbadge::{
    admit_revocation, check_pinned_authorities, revocation_targets, RevocationKind,
    RevocationRecord, REVOCATION_KIND,
};
use crate::federation::observer::FederationObserver;

/// Manifold's production relays (`manifold-environment`,
/// `PRODUCTION_NOSTR_RELAYS`)
pub const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.dev.fedibtc.com",
    "wss://relay.primal.net",
    "wss://relay.damus.io/",
];
/// Comma-separated relay URLs; empty turns the collector off
pub const RELAYS_ENV: &str = "FO_MANIFOLD_RELAYS";
/// Comma-separated PeerBadge issuer keys (x-only hex) trusted as roots, in
/// place of [`MANIFOLD_PRODUCTION_ISSUERS`]; set empty, no issuer is trusted
pub const TRUSTED_ISSUERS_ENV: &str = "FO_MANIFOLD_TRUSTED_ISSUERS";

/// The `fedibtc/manifold` revision [`MANIFOLD_PRODUCTION_ISSUERS`] was copied
/// from: the one the Fedi app (`fedixyz/fedi`, `flake.lock`, `manifold-src`)
/// pins, so the observer trusts exactly the issuers Fedi's app does. The
/// `manifold-issuers` workflow flags a newer revision whose list differs.
pub const MANIFOLD_ISSUERS_REVISION: &str = "141d6b95ccf8ebaeec6788bdf0ae2a6f9d8543d0";

/// Manifold's production PeerBadge issuers, copied in roster order from
/// `PRODUCTION_ISSUERS` in `crates/manifold-environment/src/lib.rs` at
/// [`MANIFOLD_ISSUERS_REVISION`] (MIT). Each is a personal issuer key whose
/// secret its owner holds; Manifold's release process vets every entry.
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

/// Advertisements are republished every 30 minutes and expire after 60
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Per request; relays cap results, and one event per author and kind is kept
const FETCH_LIMIT: usize = 2000;
/// Events older than this are not stored, and stored ones expired this long ago
/// are dropped
const RETENTION_DAYS: i64 = 30;
const PRESENCE_RETENTION_DAYS: i64 = 90;
/// Per revocation location; an answer this long may be cut short, so it does
/// not count as complete
const REVOCATION_FETCH_LIMIT: usize = 5000;

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

/// The trusted issuers for `setting`, the value of [`TRUSTED_ISSUERS_ENV`]:
/// Manifold's production issuers when it is unset. A key that is not 64 hex
/// characters is an error, not skipped, so a typo cannot pass unnoticed.
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

/// Checked when the observer starts, so a bad setting stops it right away,
/// and logged, so the trust roots in use are on record
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

/// The issuers credentials are checked against. An invalid setting stops the
/// observer at startup ([`check_trusted_issuers`]); should it change after,
/// no issuer is trusted, failing closed.
pub fn trusted_issuers() -> Vec<String> {
    parse_trusted_issuers(std::env::var(TRUSTED_ISSUERS_ENV).ok().as_deref()).unwrap_or_default()
}

fn unix_now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

impl FederationObserver {
    /// Creates `fmo_manifold` and runs its migrations, tracked like a
    /// module's in `module_schema_versions`
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
        // Revocations are read where each issuer's pinned authority says,
        // whatever relays discovery uses
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
            if let Err(error) = self.sync_manifold_once(&pool, &relays, &revocations).await {
                // One failed sync waits for the next; it never stops the task
                warn!(%error, "Manifold discovery sync failed");
            }
        }
    }

    async fn sync_manifold_once(
        &self,
        pool: &RelayPool,
        relays: &[String],
        revocations: &RevocationLookup,
    ) -> anyhow::Result<()> {
        let since =
            nostr_sdk::Timestamp::from(unix_now().saturating_sub(RETENTION_DAYS as u64 * 86400));
        let events = pool
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
        // An empty answer means the relays could not be reached, not that
        // every guardian vanished; presence is only counted from real answers
        ensure!(!events.is_empty(), "No relay returned any Manifold event");

        let now = unix_now();
        // The newest valid event per kind and author; ties go to the lowest id (NIP-01)
        let mut newest = HashMap::new();
        let mut rejected = 0usize;
        for event in &events {
            match verify_event(event) {
                Ok(ad) if ad.created_at <= now + MAX_AD_AGE_SECONDS => {
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
                Ok(_) => rejected += 1,
                Err(error) => {
                    // Other applications use the same kinds, so most rejects are not Manifold's
                    debug!(?error, id = %event.id(), "Ignoring discovery event");
                    rejected += 1;
                }
            }
        }

        let mut connection = self.connection().await?;
        let mut dbtx = connection.transaction().await?;
        dbtx.batch_execute("SET LOCAL search_path TO fmo_manifold, public")
            .await?;
        let ads: Vec<&VerifiedAdvertisement> = newest.values().collect();
        let outcome = store_sync(&mut dbtx, &ads, relays, now).await?;
        store_revocations(&dbtx, revocations).await?;
        dbtx.commit().await?;
        let rejected = rejected + outcome.refused;
        info!(
            events = events.len(),
            stored = outcome.stored,
            rejected,
            revocations = revocations.records.len(),
            revocations_complete = revocations.complete,
            "Manifold discovery synced"
        );
        Ok(())
    }
}

/// What one sync wrote
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StoreOutcome {
    pub stored: usize,
    /// Advertisements the database refused; each was skipped alone
    pub refused: usize,
}

/// Writes one sync: its advertisements, their versions, the day's presence,
/// retention and the sync state. Runs in `dbtx`, whose `search_path` must
/// lead with the Manifold schema.
///
/// [`verify_event`] already rejects what the database cannot store, but each
/// advertisement still goes in under a savepoint of its own: should the
/// database refuse one anyway, only that one is skipped, and the rest of the
/// sync commits.
pub(crate) async fn store_sync(
    dbtx: &mut tokio_postgres::Transaction<'_>,
    ads: &[&VerifiedAdvertisement],
    relays: &[String],
    now: u64,
) -> anyhow::Result<StoreOutcome> {
    let mut outcome = StoreOutcome {
        stored: 0,
        refused: 0,
    };
    for ad in ads {
        let savepoint = dbtx.transaction().await?;
        match store_advertisement(&savepoint, ad).await {
            Ok(()) => {
                savepoint.commit().await?;
                outcome.stored += 1;
            }
            Err(error) => {
                savepoint.rollback().await?;
                warn!(%error, author = %ad.author, event = %ad.event_id, "Skipping a Manifold advertisement the database refused");
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
        &[&now, &(MAX_AD_AGE_SECONDS as i64), &(RETENTION_DAYS * 86400)],
    )
    .await?;
    dbtx.execute(
        "DELETE FROM presence_daily WHERE day < (now() AT TIME ZONE 'UTC')::date - $1::int",
        &[&(PRESENCE_RETENTION_DAYS as i32)],
    )
    .await?;
    dbtx.execute(
        "DELETE FROM advertisements WHERE expires_at < $1",
        &[&(now - RETENTION_DAYS * 86400)],
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

/// One sync's revocation lookup
#[derive(Debug, Clone, Default)]
pub(crate) struct RevocationLookup {
    /// One per (issuer, credential); a valid revocation wins over invalid ones
    pub records: Vec<RevocationRecord>,
    /// Every revocation location answered in full
    pub complete: bool,
}

/// Reads the trusted issuers' revocations from each location. A location
/// that fails, times out or answers at the limit leaves the lookup
/// incomplete, so the revocation state is not refreshed by it.
async fn fetch_revocations(
    pool: &RelayPool,
    targets: &BTreeMap<String, Vec<String>>,
    trusted: &[String],
) -> RevocationLookup {
    let mut found: BTreeMap<(String, String), RevocationRecord> = BTreeMap::new();
    let mut complete = !targets.is_empty();
    for (relay, issuers) in targets {
        let authors: Vec<nostr_sdk::PublicKey> = issuers
            .iter()
            .filter_map(|issuer| nostr_sdk::PublicKey::from_hex(issuer).ok())
            .collect();
        let filter = Filter::new()
            .kind(Kind::Custom(REVOCATION_KIND))
            .authors(authors)
            .limit(REVOCATION_FETCH_LIMIT);
        // One relay's query ends with its EOSE; a timeout or a lost
        // connection is an error, never a silently empty answer
        let answer = match pool.relay(relay.as_str()).await {
            Ok(connection) => connection
                .get_events_of(vec![filter], FETCH_TIMEOUT, FilterOptions::ExitOnEOSE)
                .await
                .map_err(anyhow::Error::from),
            Err(error) => Err(error.into()),
        };
        match answer {
            Ok(events) if events.len() < REVOCATION_FETCH_LIMIT => {
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
            Ok(_) => {
                warn!(%relay, "A revocation answer reached the limit and may be cut short");
                complete = false;
            }
            Err(error) => {
                warn!(%error, %relay, "Could not read PeerBadge revocations");
                complete = false;
            }
        }
    }
    RevocationLookup {
        records: found.into_values().collect(),
        complete,
    }
}

/// Stores what a lookup found, and when it was complete, records it as the
/// current revocation state. Runs after [`store_sync`], in its transaction.
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
    if lookup.complete {
        dbtx.execute("UPDATE sync_state SET revocations_checked = now()", &[])
            .await?;
    }
    Ok(())
}

async fn store_advertisement(
    savepoint: &tokio_postgres::Transaction<'_>,
    ad: &VerifiedAdvertisement,
) -> anyhow::Result<()> {
    let created_at = i64::try_from(ad.created_at)?;
    let issued_at = i64::try_from(ad.issued_at)?;
    let expires_at = i64::try_from(ad.expires_at)?;
    savepoint
        .execute(
            "INSERT INTO advertisements
                 (kind, author, event_id, created_at, issued_at, expires_at, payload)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (kind, author) DO UPDATE
                 SET event_id = EXCLUDED.event_id, created_at = EXCLUDED.created_at,
                     issued_at = EXCLUDED.issued_at, expires_at = EXCLUDED.expires_at,
                     payload = EXCLUDED.payload, last_seen = now()
                 WHERE advertisements.created_at < EXCLUDED.created_at
                    OR (advertisements.created_at = EXCLUDED.created_at
                        AND EXCLUDED.event_id < advertisements.event_id)",
            &[
                &i32::from(ad.kind),
                &ad.author,
                &ad.event_id,
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
        // Legacy payloads carry an integer `version`; only v2 names the MG release
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
