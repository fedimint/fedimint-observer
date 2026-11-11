//! `/manifold/*`: discovery data as the collector stored it, joined with the
//! seats the observer verified from federations' own consensus metadata.
//! Handlers do no remote I/O; their work is bounded by the stored rows.

use std::collections::{BTreeMap, HashMap};

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, TimeDelta, Utc};
use fmo_api_types::{
    AdFreshness, AdObservationDay, FlipAttesterPolicy, FlipDisplay, FlipProvider, ListPage,
    ManifoldDiscovery, ManifoldGuardianProfile, ManifoldGuardianSummary, MgAdvertisement,
    MgApiEndpoint, MgAvailability, MgPlan, MgSoftware, MgVersionChange, OperatedSeat,
};
use serde_json::Value;

use super::collector::{configured_relays, trusted_issuers};
use super::directory::{facets, matches, sort_key, DirectoryQuery};
use super::discovery::{credential, freshness, FLIP_KIND, MG_LEGACY_KIND, MG_V2_KIND};
use super::seat_bindings::{verify_for_federation, FederationSeats};
use super::{mg_display_name, SEAT_BINDINGS_OBSERVATION};
use crate::error::ClientError;
use crate::federation::observer::{federation_id_from_db, FederationObserver};
use crate::pagination::{keyset_page, KeysetPage, Page};
use crate::AppState;

/// Expired advertisements stay listed this long, marked expired
const LISTED_AFTER_EXPIRY: TimeDelta = TimeDelta::days(7);
const PRESENCE_DAYS: i64 = 30;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/guardians", get(list_guardians))
        .route("/guardians/:fman_pubkey", get(get_guardian))
        .route("/flip-providers", get(list_flip_providers))
}

struct StoredAd {
    kind: u16,
    author: String,
    event_id: String,
    issued_at: u64,
    expires_at: u64,
    payload: Value,
}

fn unix_now() -> u64 {
    Utc::now().timestamp().max(0) as u64
}

async fn load_ads(
    observer: &FederationObserver,
    kinds: &[i32],
    author: Option<&str>,
) -> anyhow::Result<Vec<StoredAd>> {
    let listed_since = (Utc::now() - LISTED_AFTER_EXPIRY).timestamp();
    Ok(observer
        .connection()
        .await?
        .query(
            "SELECT kind, author, event_id, issued_at, expires_at, payload
             FROM fmo_manifold.advertisements
             WHERE kind = ANY($1) AND ($2::text IS NULL OR author = $2) AND expires_at >= $3",
            &[&kinds, &author, &listed_since],
        )
        .await?
        .into_iter()
        .map(|row| StoredAd {
            kind: row.get::<_, i32>(0) as u16,
            author: row.get(1),
            event_id: row.get(2),
            issued_at: row.get::<_, i64>(3).max(0) as u64,
            expires_at: row.get::<_, i64>(4).max(0) as u64,
            payload: row.get(5),
        })
        .collect())
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

/// The FA's choice: the newest fresh v2 advertisement, else the newest fresh
/// legacy one; with neither fresh, the newest of any, so it can be shown
/// expired
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

fn plan(value: &Value) -> Option<MgPlan> {
    if let Some(plan) = value.get("InfiniteBestEffort") {
        return Some(MgPlan::InfiniteBestEffort {
            price_msats: plan.get("price_msats")?.as_u64()?,
        });
    }
    let plan = value.get("SubscriptionBased")?;
    Some(MgPlan::Subscription {
        initial_price_msats: plan.get("initial_price_msats")?.as_u64()?,
        renewal_price_msats: plan.get("renewal_price_msats")?.as_u64()?,
        period: plan
            .get("period")
            .map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_owned))?,
    })
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn advertisement_view(ad: &StoredAd) -> MgAdvertisement {
    let p = &ad.payload;
    MgAdvertisement {
        event_kind: ad.kind,
        event_id: ad.event_id.clone(),
        issued_at: ad.issued_at,
        expires_at: ad.expires_at,
        signature_valid: true,
        service_pubkey: p
            .get("service_pubkey")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        api_endpoints: p
            .get("api_endpoints")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|e| {
                        Some(MgApiEndpoint {
                            transport: e.get("transport")?.as_str()?.to_owned(),
                            url: e.get("url")?.as_str()?.to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        availability: MgAvailability {
            fedimintd_version: p
                .pointer("/availability/fedimintd_version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            federation_sizes: p
                .pointer("/availability/federation_sizes")
                .and_then(Value::as_array)
                .map(|s| {
                    s.iter()
                        .filter_map(|n| n.as_u64().and_then(|n| u16::try_from(n).ok()))
                        .collect()
                })
                .unwrap_or_default(),
        },
        plans: p
            .get("plans")
            .and_then(Value::as_array)
            .map(|ps| ps.iter().filter_map(plan).collect())
            .unwrap_or_default(),
        self_reported_uptime_permille: (ad.kind == MG_V2_KIND)
            .then(|| p.get("uptime_permille").and_then(Value::as_object))
            .flatten()
            .map(|uptime| {
                uptime
                    .iter()
                    .filter_map(|(window, value)| {
                        Some((
                            window.clone(),
                            u16::try_from(value.as_u64()?).ok().filter(|v| *v <= 1000)?,
                        ))
                    })
                    .collect::<BTreeMap<_, _>>()
            }),
        software: (ad.kind == MG_V2_KIND)
            .then(|| {
                Some(MgSoftware {
                    mg_version: p.pointer("/version/mg_version")?.as_str()?.to_owned(),
                    api_version: p.pointer("/version/api_version")?.as_u64()?,
                })
            })
            .flatten(),
        holder_authorization_count: p
            .get("holder_authorizations")
            .and_then(Value::as_array)
            .map_or(0, |a| a.len() as u32),
    }
}

/// Seats bound to each Manifold Guardian in observed federations whose
/// seat-binding directory verifies against their config. Bounded by the
/// federations that publish a directory, each of at most 64 bindings.
async fn seats_by_guardian(
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
    let mut seats: HashMap<String, Vec<OperatedSeat>> = HashMap::new();
    for row in rows {
        let federation_id = federation_id_from_db(&row.get::<_, Vec<u8>>(0))?;
        let value: String = row.get(1);
        let observed_at: DateTime<Utc> = row.get(2);
        let Some(federation) = observer.get_federation(federation_id).await? else {
            continue;
        };
        let Ok(verified) = FederationSeats::from_config(&federation.config)
            .and_then(|s| verify_for_federation(&value, &s))
        else {
            continue;
        };
        let name = federation
            .config
            .global
            .meta
            .get("federation_name")
            .cloned();
        let guardians = federation.config.global.api_endpoints.len() as u32;
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
    Ok(seats)
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
    issuers: &[String],
) -> ManifoldGuardianSummary {
    let chosen = choose_ad(ads, now);
    ManifoldGuardianSummary {
        fman_pubkey: author.to_owned(),
        display_name: display_name(author),
        advertisement: chosen.map(advertisement_view),
        freshness: chosen.map(|ad| freshness(ad.issued_at, ad.expires_at, now)),
        credential: credential(
            chosen.map_or(&Value::Null, |ad| &ad.payload),
            author,
            now,
            issuers,
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
    // Validated before any work
    let limit = Page::<String> {
        after: None,
        limit: query.limit,
    }
    .limit()?;
    let observer = &state.federation_observer;
    let (observed_at, relays) = provenance(observer).await?;
    let ads = load_ads(
        observer,
        &[i32::from(MG_LEGACY_KIND), i32::from(MG_V2_KIND)],
        None,
    )
    .await?;
    let seats = seats_by_guardian(observer).await?;
    let now = unix_now();
    let issuers = trusted_issuers();

    let mut by_author: BTreeMap<&str, Vec<&StoredAd>> = BTreeMap::new();
    for ad in &ads {
        by_author.entry(ad.author.as_str()).or_default().push(ad);
    }
    // Guardians that operate a verified seat but no longer advertise are listed too
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
                &issuers,
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
    let ads = load_ads(
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
    let summary = summary(&fman_pubkey, &refs, &seats, now, &trusted_issuers());

    let connection = observer.connection().await?;
    // One entry per Fedimint version, with the MG release v2 named for it
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
    let issuers = trusted_issuers();
    let providers: Vec<FlipProvider> = load_ads(observer, &[i32::from(FLIP_KIND)], None)
        .await?
        .into_iter()
        .map(|ad| {
            let p = &ad.payload;
            let text = |key: &str| p.pointer(key).and_then(Value::as_str).map(str::to_owned);
            FlipProvider {
                credential: credential(p, &ad.author, now, &issuers),
                freshness: freshness(ad.issued_at, ad.expires_at, now),
                provider_pubkey: ad.author.clone(),
                event_id: ad.event_id.clone(),
                issued_at: ad.issued_at,
                expires_at: ad.expires_at,
                signature_valid: true,
                display: p
                    .get("display")
                    .filter(|d| d.is_object())
                    .map(|_| FlipDisplay {
                        name: text("/display/name"),
                        website: text("/display/website"),
                        contact: text("/display/contact"),
                    }),
                supported_sources: strings(p.get("supported_sources")),
                supported_networks: strings(p.pointer("/policy/supported_networks")),
                accepted_attester_policies: p
                    .pointer("/policy/accepted_attester_policies")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|i| {
                                Some(FlipAttesterPolicy {
                                    attester_pubkey: i.get("attester_pubkey")?.as_str()?.to_owned(),
                                    verification_requirement: i
                                        .get("verification_requirement")?
                                        .as_str()?
                                        .to_owned(),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                api_endpoints: strings(p.get("api_endpoints")),
                api_versions: p
                    .get("api_versions")
                    .and_then(Value::as_array)
                    .map(|v| v.iter().filter_map(Value::as_u64).collect())
                    .unwrap_or_default(),
                relay_hints: strings(p.get("relay_hints")),
                confirmed_federations: Vec::new(),
            }
        })
        .collect();
    // Fresh ones first, newest first; the key is the row's order and cursor
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
