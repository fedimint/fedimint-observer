use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use anyhow::Context;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Utc;
use fedimint_core::config::FederationId;
use fedimint_core::Amount;
use fmo_api_types::{
    ActivityScale, FederationHealth, FederationPage, FederationSummary, NostrFederationEntry,
    NostrFederationPage,
};
use serde::Deserialize;
use tracing::warn;

use crate::error::ClientError;
use crate::pagination::{key_text, keyset_page, Page};
use crate::AppState;

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FederationGroup {
    #[default]
    Active,
    Offline,
    ShutDown,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FederationSort {
    #[default]
    Assets,
    Volume,
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
    pub search: Option<String>,
    #[serde(default)]
    pub sort: FederationSort,
    pub dir: Option<SortDirection>,
    pub after: Option<String>,
    pub limit: Option<u16>,
}

pub(crate) fn group_of(federation: &FederationSummary, now: u64) -> FederationGroup {
    group_for(federation.expiry_timestamp, federation.health, now)
}

pub(crate) fn group_for(
    expiry_timestamp: Option<u64>,
    health: FederationHealth,
    now: u64,
) -> FederationGroup {
    if expiry_timestamp.is_some_and(|expiry| expiry <= now) {
        FederationGroup::ShutDown
    } else if health == FederationHealth::Offline {
        FederationGroup::Offline
    } else {
        FederationGroup::Active
    }
}

const MIN_ID_SUBSTRING_LEN: usize = 6;

/// Drops a `fedimint:` or `web+fedimint:` scheme, with or without `//`
fn strip_invite_scheme(input: &str) -> &str {
    for scheme in ["web+fedimint:", "fedimint:"] {
        if input
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        {
            let rest = &input[scheme.len()..];
            return rest.strip_prefix("//").unwrap_or(rest);
        }
    }
    input
}

/// invite code or federation ID (prefix.
pub(crate) fn matches_search(federation: &FederationSummary, search: Option<&str>) -> bool {
    let Some(query) = search.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    let name_needle = query.to_lowercase();
    let needle = strip_invite_scheme(query).to_lowercase();
    let id = federation.id.to_string();
    federation
        .name
        .as_deref()
        .is_some_and(|name| name.to_lowercase().contains(&name_needle))
        || (!needle.is_empty()
            && (federation.invite.to_lowercase().contains(&needle)
                || id.starts_with(&needle)
                || (needle.len() >= MIN_ID_SUBSTRING_LEN && id.contains(&needle))))
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

/// Only shut-down rows show observed session dates, so only they pay for the
/// range lookups
pub(crate) fn rows_needing_session_ranges(
    group: FederationGroup,
    rows: &mut [FederationSummary],
) -> &mut [FederationSummary] {
    match group {
        FederationGroup::ShutDown => rows,
        FederationGroup::Active | FederationGroup::Offline => &mut [],
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

pub(crate) fn unix_now() -> u64 {
    Utc::now().timestamp().max(0) as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActiveAssets {
    pub held: Amount,
    pub reporting: u64,
    pub unknown: u64,
}

impl ActiveAssets {
    /// `None` when none reports, so unknown never reads as a measured zero
    pub(crate) fn known_held(&self) -> Option<Amount> {
        (self.reporting > 0 || self.unknown == 0).then_some(self.held)
    }
}

pub(crate) fn active_assets(
    federations: impl IntoIterator<Item = (FederationGroup, Option<Amount>)>,
) -> anyhow::Result<ActiveAssets> {
    federations
        .into_iter()
        .filter(|(group, _)| *group == FederationGroup::Active)
        .try_fold(
            ActiveAssets {
                held: Amount::ZERO,
                reporting: 0,
                unknown: 0,
            },
            |total, (_, deposits)| match deposits {
                Some(amount) => Ok(ActiveAssets {
                    held: total
                        .held
                        .checked_add(amount)
                        .context("Active federation asset total overflow")?,
                    reporting: total.reporting + 1,
                    ..total
                }),
                None => Ok(ActiveAssets {
                    unknown: total.unknown + 1,
                    ..total
                }),
            },
        )
}

/// `None` with a warning, so asset data never fails the other totals
pub(crate) fn available_assets(result: anyhow::Result<ActiveAssets>) -> Option<ActiveAssets> {
    result
        .inspect_err(|err| {
            warn!(
                ?err,
                "Active asset total unavailable; serving the other totals"
            )
        })
        .ok()
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
    let mut page = keyset_page(
        matching,
        |f| sort_key(f, query.sort, direction),
        query.after.as_deref(),
        limit,
    )?;
    state
        .federation_observer
        .attach_session_ranges(rows_needing_session_ranges(query.group, &mut page.items))
        .await?;
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
    let summary = state
        .federation_observer
        .federation_summary(federation_id)
        .await?
        .ok_or_else(|| anyhow::Error::from(ClientError::not_found("Federation not found")))?;
    Ok(Json(summary))
}

/// Module kind per instance id and wallet network, as parseConfig reads them
pub(crate) fn config_outline(
    config: &serde_json::Value,
) -> (BTreeMap<u16, String>, Option<String>) {
    let modules: BTreeMap<u16, &serde_json::Value> = config["modules"]
        .as_object()
        .map(|modules| {
            modules
                .iter()
                .filter_map(|(instance, module)| match instance.parse() {
                    Ok(instance) => Some((instance, module)),
                    Err(err) => {
                        warn!(%instance, ?err, "Skipping a config module with a non-numeric instance id");
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let module_kinds = modules
        .iter()
        .map(|(instance, module)| {
            let kind = module["kind"].as_str().unwrap_or("unknown");
            (*instance, kind.to_owned())
        })
        .collect();
    let network = ["wallet", "walletv2"]
        .into_iter()
        .find_map(|kind| modules.values().find(|module| module["kind"] == kind))
        .and_then(|module| module["network"].as_str())
        .map(str::to_owned);
    (module_kinds, network)
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NostrGroup {
    #[default]
    Online,
    Offline,
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
