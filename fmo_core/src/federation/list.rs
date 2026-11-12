//! `GET /federations/page`, `GET /federations/:id/summary` and
//! `GET /nostr/federations/page`: the home page's and the Nostr page's lists,
//! searched, grouped, ordered and paged on the server.
//!
//! `GET /federations` and `GET /nostr/federations` keep returning every
//! federation, unchanged, for existing API clients.

use std::cmp::Reverse;
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Utc;
use fedimint_core::config::FederationId;
use fmo_api_types::{
    ActivityScale, FederationHealth, FederationPage, FederationSummary, NostrFederationEntry,
    NostrFederationPage,
};
use serde::Deserialize;

use crate::error::ClientError;
use crate::pagination::{key_text, keyset_page, Page};
use crate::AppState;

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FederationGroup {
    /// Neither shut down nor offline
    #[default]
    Active,
    /// Too few guardians answer to reach consensus
    Offline,
    /// The expiry its guardians published has passed
    ShutDown,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FederationSort {
    /// Assets under management
    #[default]
    Assets,
    /// Amount transferred over 7 days
    Volume,
    /// Transactions over 7 days
    Transactions,
    Name,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct FederationListQuery {
    #[serde(default)]
    pub group: FederationGroup,
    /// Matches the name or the invite code
    pub search: Option<String>,
    #[serde(default)]
    pub sort: FederationSort,
    /// Defaults to the sort's "best first": descending, ascending for names
    pub dir: Option<SortDirection>,
    pub after: Option<String>,
    pub limit: Option<u16>,
}

pub(crate) fn group_of(federation: &FederationSummary, now: u64) -> FederationGroup {
    if federation
        .expiry_timestamp
        .is_some_and(|expiry| expiry <= now)
    {
        FederationGroup::ShutDown
    } else if federation.health == FederationHealth::Offline {
        FederationGroup::Offline
    } else {
        FederationGroup::Active
    }
}

pub(crate) fn matches_search(federation: &FederationSummary, search: Option<&str>) -> bool {
    let Some(needle) = search.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    let needle = needle.to_lowercase();
    federation
        .name
        .as_deref()
        .is_some_and(|name| name.to_lowercase().contains(&needle))
        || federation.invite.to_lowercase().contains(&needle)
}

fn volume_msat(federation: &FederationSummary) -> u64 {
    federation
        .last_7d_activity
        .iter()
        .map(|day| day.amount_transferred.msats)
        .sum()
}

fn transactions(federation: &FederationSummary) -> u64 {
    federation
        .last_7d_activity
        .iter()
        .map(|day| day.num_transactions)
        .sum()
}

/// The row's position in the chosen order, unique per federation. Unknown
/// values always sort last; ties go by name, then id, so the order never
/// depends on the order the rows were loaded in.
pub(crate) type SortKey = (
    bool,
    i128,
    Option<String>,
    Option<Reverse<String>>,
    String,
    String,
);

pub(crate) fn sort_key(
    federation: &FederationSummary,
    sort: FederationSort,
    direction: SortDirection,
) -> SortKey {
    let name = federation
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .map(|name| key_text(&name.to_lowercase()));
    let id = federation.id.to_string();
    let signed = |value: u64| match direction {
        SortDirection::Asc => i128::from(value),
        SortDirection::Desc => -i128::from(value),
    };
    match sort {
        FederationSort::Name => {
            let missing = name.is_none();
            let (asc, desc) = match direction {
                SortDirection::Asc => (name, None),
                SortDirection::Desc => (None, name.map(Reverse)),
            };
            // The name is already in the key; the tie-breaker is the id alone
            (missing, 0, asc, desc, String::new(), id)
        }
        metric => {
            let value = match metric {
                FederationSort::Assets => federation.deposits.map(|amount| amount.msats),
                FederationSort::Volume => Some(volume_msat(federation)),
                FederationSort::Transactions => Some(transactions(federation)),
                FederationSort::Name => unreachable!("handled above"),
            };
            (
                value.is_none(),
                value.map_or(0, signed),
                None,
                None,
                name.unwrap_or_default(),
                id,
            )
        }
    }
}

pub(crate) fn activity_scale(federations: &[FederationSummary]) -> ActivityScale {
    federations
        .iter()
        .flat_map(|federation| &federation.last_7d_activity)
        .fold(ActivityScale::default(), |scale, day| ActivityScale {
            max_transactions: scale.max_transactions.max(day.num_transactions),
            max_volume_msat: scale.max_volume_msat.max(day.amount_transferred.msats),
        })
}

fn unix_now() -> u64 {
    Utc::now().timestamp().max(0) as u64
}

pub(crate) async fn list_federation_page(
    Query(query): Query<FederationListQuery>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<FederationPage>> {
    let limit = Page::<String> {
        after: None,
        limit: query.limit,
    }
    .limit()?;
    let all = state
        .federation_observer
        .list_federation_summaries()
        .await?;
    let activity_scale = activity_scale(&all);
    let now = unix_now();
    let direction = query.dir.unwrap_or(match query.sort {
        FederationSort::Name => SortDirection::Asc,
        _ => SortDirection::Desc,
    });
    let matching: Vec<FederationSummary> = all
        .into_iter()
        .filter(|f| group_of(f, now) == query.group && matches_search(f, query.search.as_deref()))
        .collect();
    let page = keyset_page(
        matching,
        |f| sort_key(f, query.sort, direction),
        query.after.as_deref(),
        limit,
    )?;
    Ok(Json(FederationPage {
        federations: page.items,
        next_after: page.next_after,
        total: page.total as u64,
        activity_scale,
    }))
}

pub(crate) async fn get_federation_summary(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<FederationSummary>> {
    state
        .federation_observer
        .list_federation_summaries()
        .await?
        .into_iter()
        .find(|federation| federation.id == federation_id)
        .map(Json)
        .ok_or_else(|| anyhow::Error::from(ClientError::not_found("Federation not found")).into())
}

/// The Nostr page's groups, as the home page divides federations
#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NostrGroup {
    /// Observed, and enough guardians answer
    #[default]
    Online,
    /// Observed, with too few guardians answering
    Offline,
    /// Announced, but not observed: the observer knows nothing of its health
    Unobserved,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct NostrListQuery {
    #[serde(default)]
    pub group: NostrGroup,
    pub after: Option<String>,
    pub limit: Option<u16>,
}

pub(crate) fn nostr_group_of(entry: &NostrFederationEntry) -> NostrGroup {
    match entry.health {
        None => NostrGroup::Unobserved,
        Some(FederationHealth::Offline) => NostrGroup::Offline,
        Some(_) => NostrGroup::Online,
    }
}

/// Named federations first, by name, then the rest by id
pub(crate) type NostrSortKey = (bool, String, String);

pub(crate) fn nostr_sort_key(entry: &NostrFederationEntry) -> NostrSortKey {
    let name = entry
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .map(|name| key_text(&name.to_lowercase()));
    (
        name.is_none(),
        name.unwrap_or_default(),
        entry.id.to_string(),
    )
}

pub(crate) async fn list_nostr_federation_page(
    Query(query): Query<NostrListQuery>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<NostrFederationPage>> {
    let limit = Page::<String> {
        after: None,
        limit: query.limit,
    }
    .limit()?;
    let observer = &state.federation_observer;
    let announced = observer.list_nostr_federations().await?;
    let known: HashMap<FederationId, FederationSummary> = observer
        .list_federation_summaries()
        .await?
        .into_iter()
        .map(|federation| (federation.id, federation))
        .collect();
    let entries: Vec<NostrFederationEntry> = announced
        .into_iter()
        .map(|federation| {
            let observed = known.get(&federation.federation_id);
            NostrFederationEntry {
                id: federation.federation_id,
                invite: federation.invite_code.to_string(),
                name: observed.and_then(|f| f.name.clone()),
                health: observed.map(|f| f.health),
            }
        })
        .filter(|entry| nostr_group_of(entry) == query.group)
        .collect();
    let page = keyset_page(entries, nostr_sort_key, query.after.as_deref(), limit)?;
    Ok(Json(NostrFederationPage {
        federations: page.items,
        next_after: page.next_after,
        total: page.total as u64,
    }))
}

#[cfg(test)]
mod tests;
