use std::collections::{BTreeMap, HashMap};

use anyhow::Context;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, TimeDelta, Utc};
use fedimint_core::config::{ClientConfig, FederationId};
use fmo_api_types::{
    AdFreshness, AdObservationDay, FlipProvider, ListPage, ManifoldDiscovery,
    ManifoldGuardianProfile, ManifoldGuardianSummary, MgAdvertisement, MgVersionChange,
    OperatedSeat,
};
use serde_json::Value;
use tracing::debug;

use super::collector::{configured_relays, derivation_stamp, trusted_issuers, Retention};
use super::directory::{facets, matches, sort_key, DirectoryQuery};
use super::discovery::{
    assess_credential, freshness, CredentialEvidence, FLIP_KIND, MG_LEGACY_KIND, MG_V2_KIND,
};
use super::listing::FlipListing;
use super::peerbadge::{IssuerRevocationCheck, RevocationKind, RevocationState, TrustContext};
use super::seat_bindings::{verify_for_federation, FederationSeats};
use super::{mg_display_name, SEAT_BINDINGS_OBSERVATION};
use crate::error::ClientError;
use crate::federation::observer::{federation_id_from_db, FederationObserver};
use crate::pagination::{keyset_page, KeysetPage, Page};
use crate::AppState;

const LISTED_AFTER_EXPIRY: TimeDelta = TimeDelta::days(7);
const PRESENCE_DAYS: i64 = 30;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/guardians", get(list_guardians))
        .route("/guardians/:fman_pubkey", get(get_guardian))
        .route("/flip-providers", get(list_flip_providers))
}

enum Listing {
    Mg(Box<MgAdvertisement>),
    Flip(Box<FlipListing>),
}

struct StoredAd {
    kind: u16,
    author: String,
    event_id: String,
    issued_at: u64,
    expires_at: u64,
    listing: Listing,
    evidence: CredentialEvidence,
}

type RevocationEntries = HashMap<(String, String), RevocationKind>;

fn unix_now() -> u64 {
    Utc::now().timestamp().max(0) as u64
}

async fn load_ads(
    observer: &FederationObserver,
    kinds: &[i32],
    author: Option<&str>,
) -> anyhow::Result<(Vec<StoredAd>, RevocationEntries)> {
    let now = Utc::now().timestamp();
    let listed_since = now - LISTED_AFTER_EXPIRY.num_seconds();
    // The collector keeps at most this many per kind.
    let retention = Retention::DEFAULT;
    let created_since = now - retention.max_age_seconds;
    let max_rows = retention
        .max_rows_per_kind()
        .saturating_mul(kinds.len() as i64);
    let rows = observer
        .connection()
        .await?
        .query(
            "SELECT a.kind, a.author, a.event_id, a.issued_at, a.expires_at, a.listing,
                    a.credential, a.credential_issuer, a.credential_digest, r.state
             FROM fmo_manifold.advertisements a
             LEFT JOIN fmo_manifold.revocations r
                 ON r.issuer = a.credential_issuer AND r.credential_digest = a.credential_digest
             WHERE a.kind = ANY($1) AND ($2::text IS NULL OR a.author = $2) AND a.expires_at >= $3
               AND a.created_at >= $4 AND a.derived_stamp = $6
             ORDER BY a.created_at DESC
             LIMIT $5",
            &[
                &kinds,
                &author,
                &listed_since,
                &created_since,
                &max_rows,
                &derivation_stamp(),
            ],
        )
        .await?;
    let mut ads = Vec::with_capacity(rows.len());
    let mut revocations = RevocationEntries::new();
    for row in rows {
        let kind = row.get::<_, i32>(0) as u16;
        let author: String = row.get(1);
        let event_id: String = row.get(2);
        let listing: Option<Value> = row.get(5);
        let evidence: Option<Value> = row.get(6);
        let derived = listing.zip(evidence).and_then(|(listing, evidence)| {
            let listing = if kind == FLIP_KIND {
                Listing::Flip(serde_json::from_value(listing).ok()?)
            } else {
                Listing::Mg(serde_json::from_value(listing).ok()?)
            };
            Some((listing, serde_json::from_value(evidence).ok()?))
        });
        let Some((listing, evidence)) = derived else {
            debug!(%author, event = %event_id, "Skipping a Manifold advertisement without a usable derivation");
            continue;
        };
        let issuer: Option<String> = row.get(7);
        let digest: Option<String> = row.get(8);
        if let (Some(issuer), Some(digest), Some(kind)) = (
            issuer,
            digest,
            row.get::<_, Option<&str>>(9)
                .and_then(RevocationKind::parse),
        ) {
            revocations.insert((issuer, digest), kind);
        }
        ads.push(StoredAd {
            kind,
            author,
            event_id,
            issued_at: row.get::<_, i64>(3).max(0) as u64,
            expires_at: row.get::<_, i64>(4).max(0) as u64,
            listing,
            evidence,
        });
    }
    Ok((ads, revocations))
}

async fn provenance(
    observer: &FederationObserver,
) -> anyhow::Result<(Option<DateTime<Utc>>, Vec<String>)> {
    let row = observer
        .connection()
        .await?
        .query_opt(
            "SELECT last_success, relays FROM fmo_manifold.sync_state",
            &[],
        )
        .await?;
    Ok(match row {
        Some(row) => (Some(row.get(0)), row.get(1)),
        None => (None, configured_relays()),
    })
}

async fn trust_context(
    observer: &FederationObserver,
    entries: RevocationEntries,
) -> anyhow::Result<TrustContext> {
    let issuers = trusted_issuers();
    let checks = observer
        .connection()
        .await?
        .query(
            "SELECT issuer, locations, checked_at FROM fmo_manifold.revocation_checks
             WHERE issuer = ANY($1)",
            &[&issuers],
        )
        .await?
        .iter()
        .map(|row| {
            (
                row.get::<_, String>(0),
                IssuerRevocationCheck {
                    locations: row.get(1),
                    checked_at: row.get(2),
                },
            )
        })
        .collect();
    Ok(TrustContext {
        issuers,
        revocations: RevocationState { checks, entries },
    })
}

fn wrap<T>(
    observed_at: Option<DateTime<Utc>>,
    relays: Vec<String>,
    data: T,
) -> Json<ManifoldDiscovery<T>> {
    Json(ManifoldDiscovery {
        observed_at,
        relays,
        data,
        page: None,
        facets: None,
    })
}

/// A list answer: one page of rows and where the list stands
fn wrap_page<T>(
    observed_at: Option<DateTime<Utc>>,
    relays: Vec<String>,
    page: KeysetPage<T>,
) -> ManifoldDiscovery<Vec<T>> {
    ManifoldDiscovery {
        observed_at,
        relays,
        data: page.items,
        page: Some(ListPage {
            next_after: page.next_after,
            total: page.total as u64,
        }),
        facets: None,
    }
}

fn choose_ad<'a>(ads: &[&'a StoredAd], now: u64) -> Option<&'a StoredAd> {
    let fresh = |ad: &&&StoredAd| freshness(ad.issued_at, ad.expires_at, now) == AdFreshness::Fresh;
    let newest = |kind: u16, fresh_only: bool| {
        ads.iter()
            .filter(|ad| ad.kind == kind)
            .filter(|ad| !fresh_only || fresh(ad))
            .max_by_key(|ad| (ad.issued_at, std::cmp::Reverse(ad.event_id.clone())))
            .copied()
    };
    newest(MG_V2_KIND, true)
        .or_else(|| newest(MG_LEGACY_KIND, true))
        .or_else(|| {
            ads.iter()
                .max_by_key(|ad| (ad.issued_at, ad.kind == MG_V2_KIND))
                .copied()
        })
}

pub(super) async fn seats_by_guardian(
    observer: &FederationObserver,
) -> anyhow::Result<HashMap<String, Vec<OperatedSeat>>> {
    let rows = observer
        .connection()
        .await?
        .query(
            "SELECT federation_id, data ->> 'value', observed_at FROM federation_observations
             WHERE kind = $1 AND data ->> 'value' IS NOT NULL",
            &[&SEAT_BINDINGS_OBSERVATION],
        )
        .await?;
    let mut directories: Vec<(FederationId, ClientConfig, String, DateTime<Utc>)> = Vec::new();
    for row in rows {
        let federation_id = federation_id_from_db(&row.get::<_, Vec<u8>>(0))?;
        let Some(federation) = observer.get_federation(federation_id).await? else {
            continue;
        };
        directories.push((federation_id, federation.config, row.get(1), row.get(2)));
    }
    tokio::task::spawn_blocking(move || verified_seats(directories))
        .await
        .context("Verifying seat bindings panicked")
}

fn verified_seats(
    directories: Vec<(FederationId, ClientConfig, String, DateTime<Utc>)>,
) -> HashMap<String, Vec<OperatedSeat>> {
    let mut seats: HashMap<String, Vec<OperatedSeat>> = HashMap::new();
    for (federation_id, config, value, observed_at) in directories {
        let Ok(verified) =
            FederationSeats::from_config(&config).and_then(|s| verify_for_federation(&value, &s))
        else {
            continue;
        };
        let name = config.global.meta.get("federation_name").cloned();
        let guardians = config.global.api_endpoints.len() as u32;
        for binding in verified {
            seats
                .entry(binding.fman_pubkey)
                .or_default()
                .push(OperatedSeat {
                    federation_id: federation_id.to_string(),
                    federation_name: name.clone(),
                    federation_guardians: guardians,
                    peer_id: binding.peer_id,
                    bound_at: binding.issued_at,
                    verified_at: observed_at,
                });
        }
    }
    seats
}

fn display_name(fman_pubkey: &str) -> String {
    hex::decode(fman_pubkey)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .map_or_else(
            || fman_pubkey.chars().take(12).collect(),
            |key| mg_display_name(&key),
        )
}

fn summary(
    author: &str,
    ads: &[&StoredAd],
    seats: &[OperatedSeat],
    now: u64,
    trust: &TrustContext,
) -> ManifoldGuardianSummary {
    let chosen = choose_ad(ads, now);
    let absent = CredentialEvidence::absent();
    ManifoldGuardianSummary {
        fman_pubkey: author.to_owned(),
        display_name: display_name(author),
        advertisement: chosen.and_then(|ad| match &ad.listing {
            Listing::Mg(listing) => Some(listing.as_ref().clone()),
            Listing::Flip(_) => None,
        }),
        freshness: chosen.map(|ad| freshness(ad.issued_at, ad.expires_at, now)),
        credential: assess_credential(
            chosen.map_or(&absent, |ad| &ad.evidence),
            author,
            now,
            trust,
        ),
        seat_count: seats.len() as u32,
        federation_count: seats
            .iter()
            .map(|s| &s.federation_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len() as u32,
    }
}

async fn list_guardians(
    Query(query): Query<DirectoryQuery>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<ManifoldDiscovery<Vec<ManifoldGuardianSummary>>>> {
    let limit = Page::<String> {
        after: None,
        limit: query.limit,
    }
    .limit()?;
    let observer = &state.federation_observer;
    let (observed_at, relays) = provenance(observer).await?;
    let (ads, revocations) = load_ads(
        observer,
        &[i32::from(MG_LEGACY_KIND), i32::from(MG_V2_KIND)],
        None,
    )
    .await?;
    let seats = seats_by_guardian(observer).await?;
    let now = unix_now();
    let trust = trust_context(observer, revocations).await?;

    let mut by_author: BTreeMap<&str, Vec<&StoredAd>> = BTreeMap::new();
    for ad in &ads {
        by_author.entry(ad.author.as_str()).or_default().push(ad);
    }
    for author in seats.keys() {
        by_author.entry(author.as_str()).or_default();
    }
    let list: Vec<ManifoldGuardianSummary> = by_author
        .iter()
        .map(|(author, ads)| {
            summary(
                author,
                ads,
                seats.get(*author).map_or(&[][..], Vec::as_slice),
                now,
                &trust,
            )
        })
        .collect();
    let facets = facets(&list);
    let matching = list.into_iter().filter(|g| matches(g, &query)).collect();
    let page = keyset_page(
        matching,
        |g| sort_key(g, query.sort),
        query.after.as_deref(),
        limit,
    )?;
    let mut answer = wrap_page(observed_at, relays, page);
    answer.facets = Some(facets);
    Ok(Json(answer))
}

async fn get_guardian(
    Path(fman_pubkey): Path<String>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<ManifoldDiscovery<ManifoldGuardianProfile>>> {
    if fman_pubkey.len() != 64
        || !fman_pubkey
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(anyhow::Error::from(ClientError::bad_request(
            "The key must be 64 lowercase hex characters",
        ))
        .into());
    }
    let observer = &state.federation_observer;
    let (observed_at, relays) = provenance(observer).await?;
    let (ads, revocations) = load_ads(
        observer,
        &[i32::from(MG_LEGACY_KIND), i32::from(MG_V2_KIND)],
        Some(&fman_pubkey),
    )
    .await?;
    let seats = seats_by_guardian(observer)
        .await?
        .remove(&fman_pubkey)
        .unwrap_or_default();
    if ads.is_empty() && seats.is_empty() {
        return Err(anyhow::Error::from(ClientError::not_found(
            "The observer has not seen this Manifold Guardian",
        ))
        .into());
    }
    let now = unix_now();
    let refs: Vec<&StoredAd> = ads.iter().collect();
    let trust = trust_context(observer, revocations).await?;
    let summary = summary(&fman_pubkey, &refs, &seats, now, &trust);

    let connection = observer.connection().await?;
    let mut versions: BTreeMap<String, (i64, Option<String>)> = BTreeMap::new();
    for row in connection
        .query(
            "SELECT fedimintd_version, mg_version, first_seen FROM fmo_manifold.version_changes WHERE author = $1",
            &[&fman_pubkey],
        )
        .await?
    {
        let (fedimintd, mg, first_seen): (String, String, i64) = (row.get(0), row.get(1), row.get(2));
        let entry = versions.entry(fedimintd).or_insert((first_seen, None));
        entry.0 = entry.0.min(first_seen);
        if !mg.is_empty() {
            entry.1 = Some(mg);
        }
    }
    let mut version_history: Vec<MgVersionChange> = versions
        .into_iter()
        .map(
            |(fedimintd_version, (first_seen, mg_version))| MgVersionChange {
                observed_at: DateTime::from_timestamp(first_seen, 0).unwrap_or_default(),
                fedimintd_version,
                mg_version,
            },
        )
        .collect();
    version_history.sort_by_key(|change| std::cmp::Reverse(change.observed_at));

    let presence: HashMap<chrono::NaiveDate, (i32, i32)> = connection
        .query(
            "SELECT day, polls, fresh_polls FROM fmo_manifold.presence_daily
             WHERE author = $1 AND day > (now() AT TIME ZONE 'UTC')::date - $2::int",
            &[&fman_pubkey, &(PRESENCE_DAYS as i32)],
        )
        .await?
        .into_iter()
        .map(|row| (row.get(0), (row.get(1), row.get(2))))
        .collect();
    let today = Utc::now().date_naive();
    let observation_history = (0..PRESENCE_DAYS)
        .rev()
        .map(|back| {
            let day = today - TimeDelta::days(back);
            let (polls, fresh) = presence.get(&day).copied().unwrap_or((0, 0));
            AdObservationDay {
                date: day.to_string(),
                polls: polls.max(0) as u32,
                fresh_polls: fresh.max(0) as u32,
            }
        })
        .collect();

    Ok(wrap(
        observed_at,
        relays.clone(),
        ManifoldGuardianProfile {
            summary,
            nostr_relays: relays,
            seats,
            version_history,
            observation_history,
        },
    ))
}

async fn list_flip_providers(
    Query(page): Query<Page<String>>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<ManifoldDiscovery<Vec<FlipProvider>>>> {
    let limit = page.limit()?;
    let observer = &state.federation_observer;
    let (observed_at, relays) = provenance(observer).await?;
    let now = unix_now();
    let (ads, revocations) = load_ads(observer, &[i32::from(FLIP_KIND)], None).await?;
    let trust = trust_context(observer, revocations).await?;
    let providers: Vec<FlipProvider> = ads
        .into_iter()
        .filter_map(|ad| {
            let Listing::Flip(listing) = ad.listing else {
                return None;
            };
            let listing = *listing;
            Some(FlipProvider {
                credential: assess_credential(&ad.evidence, &ad.author, now, &trust),
                freshness: freshness(ad.issued_at, ad.expires_at, now),
                provider_pubkey: ad.author,
                event_id: ad.event_id,
                issued_at: ad.issued_at,
                expires_at: ad.expires_at,
                signature_valid: true,
                display: listing.display,
                supported_sources: listing.supported_sources,
                supported_networks: listing.supported_networks,
                accepted_attester_policies: listing.accepted_attester_policies,
                api_endpoints: listing.api_endpoints,
                api_versions: listing.api_versions,
                relay_hints: listing.relay_hints,
                confirmed_federations: Vec::new(),
            })
        })
        .collect();
    let page = keyset_page(
        providers,
        |p| {
            (
                p.freshness != AdFreshness::Fresh,
                -i64::try_from(p.issued_at).unwrap_or(i64::MAX),
                p.provider_pubkey.clone(),
            )
        },
        page.after.as_deref(),
        limit,
    )?;
    Ok(Json(wrap_page(observed_at, relays, page)))
}
