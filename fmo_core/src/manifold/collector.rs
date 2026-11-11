//! The background collector of Manifold discovery events. It is the only
//! code that talks to relays: API handlers read what it stored.
//!
//! Each sync asks the relays for the latest addressable events of the three
//! advertisement kinds, bounded by a per-request limit, verifies every one
//! ([`super::discovery::verify_event`]), and keeps the newest per kind and
//! author. It also records, per Manifold Guardian and UTC day, how many syncs
//! found a fresh advertisement, and each new version it advertised.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::ensure;
use nostr_sdk::{Filter, FilterOptions, Kind, RelayOptions, RelayPool, RelayPoolOptions};
use tracing::{debug, info, warn};

use super::discovery::{verify_event, FLIP_KIND, MAX_AD_AGE_SECONDS, MG_LEGACY_KIND, MG_V2_KIND};
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
/// Comma-separated PeerBadge issuer keys trusted as roots; none by default,
/// as Manifold's production environment pins none yet
pub const TRUSTED_ISSUERS_ENV: &str = "FO_MANIFOLD_TRUSTED_ISSUERS";

/// Advertisements are republished every 30 minutes and expire after 60
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Per request; relays cap results, and one event per author and kind is kept
const FETCH_LIMIT: usize = 2000;
/// Events older than this are not stored, and stored ones expired this long ago
/// are dropped
const RETENTION_DAYS: i64 = 30;
const PRESENCE_RETENTION_DAYS: i64 = 90;

pub(crate) const MIGRATIONS: &[&str] = &[r"
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
"];

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

pub fn trusted_issuers() -> Vec<String> {
    std::env::var(TRUSTED_ISSUERS_ENV)
        .unwrap_or_default()
        .split(',')
        .map(|key| key.trim().to_lowercase())
        .filter(|key| key.len() == 64)
        .collect()
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
        info!(?relays, "Started Manifold discovery");

        let mut interval = tokio::time::interval(SYNC_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) = self.sync_manifold_once(&pool, &relays).await {
                // One failed sync waits for the next; it never stops the task
                warn!(%error, "Manifold discovery sync failed");
            }
        }
    }

    async fn sync_manifold_once(&self, pool: &RelayPool, relays: &[String]) -> anyhow::Result<()> {
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
                    let replace = newest.get(&key).is_none_or(
                        |current: &super::discovery::VerifiedAdvertisement| {
                            (ad.created_at, std::cmp::Reverse(&ad.event_id))
                                > (current.created_at, std::cmp::Reverse(&current.event_id))
                        },
                    );
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
        let dbtx = connection.transaction().await?;
        for ad in newest.values() {
            dbtx.execute(
                "INSERT INTO fmo_manifold.advertisements
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
                    &(ad.created_at as i64),
                    &(ad.issued_at as i64),
                    &(ad.expires_at as i64),
                    &ad.payload,
                ],
            )
            .await?;
            dbtx.execute(
                "UPDATE fmo_manifold.advertisements SET last_seen = now()
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
                    dbtx.execute(
                        "INSERT INTO fmo_manifold.version_changes (author, fedimintd_version, mg_version, first_seen)
                         VALUES ($1, $2, $3, $4)
                         ON CONFLICT (author, fedimintd_version, mg_version)
                         DO UPDATE SET first_seen = LEAST(version_changes.first_seen, EXCLUDED.first_seen)",
                        &[&ad.author, &fedimintd, &mg, &(ad.issued_at as i64)],
                    )
                    .await?;
                }
            }
        }

        let now_i64 = now as i64;
        dbtx.execute(
            "INSERT INTO fmo_manifold.presence_daily (author, day, polls, fresh_polls)
             SELECT author, (now() AT TIME ZONE 'UTC')::date, 1,
                    MAX(CASE WHEN issued_at <= $1 AND expires_at > $1 AND $1 - issued_at <= $2 THEN 1 ELSE 0 END)
             FROM fmo_manifold.advertisements
             WHERE kind IN (37701, 37709) AND created_at >= $1 - $3
             GROUP BY author
             ON CONFLICT (author, day) DO UPDATE
                 SET polls = presence_daily.polls + 1,
                     fresh_polls = presence_daily.fresh_polls + EXCLUDED.fresh_polls",
            &[&now_i64, &(MAX_AD_AGE_SECONDS as i64), &(RETENTION_DAYS * 86400)],
        )
        .await?;
        dbtx.execute(
            "DELETE FROM fmo_manifold.presence_daily WHERE day < (now() AT TIME ZONE 'UTC')::date - $1::int",
            &[&(PRESENCE_RETENTION_DAYS as i32)],
        )
        .await?;
        dbtx.execute(
            "DELETE FROM fmo_manifold.advertisements WHERE expires_at < $1",
            &[&(now_i64 - RETENTION_DAYS * 86400)],
        )
        .await?;
        dbtx.execute(
            "INSERT INTO fmo_manifold.sync_state (singleton, last_success, relays) VALUES (true, now(), $1)
             ON CONFLICT (singleton) DO UPDATE SET last_success = now(), relays = EXCLUDED.relays",
            &[&relays],
        )
        .await?;
        dbtx.commit().await?;
        info!(
            events = events.len(),
            stored = newest.len(),
            rejected,
            "Manifold discovery synced"
        );
        Ok(())
    }
}
