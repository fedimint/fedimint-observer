//! The gateway directory's search, status filter and order, applied on the
//! server so each page of the overview is the right slice of the result.

use fmo_api_types::{GatewayInfo, GatewayStatus};
use serde::Deserialize;

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GatewayFilter {
    /// Every gateway that is not retired
    #[default]
    All,
    Online,
    Degraded,
    Offline,
    Unknown,
    Retired,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum GatewaySort {
    /// By when it was last seen
    #[default]
    Freshness,
    Status,
    Uptime,
    Activity,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SortDirection {
    Asc,
    #[default]
    Desc,
}

fn status(gateway: &GatewayInfo) -> GatewayStatus {
    gateway.status.unwrap_or(GatewayStatus::Unknown)
}

pub(crate) fn matches(gateway: &GatewayInfo, filter: GatewayFilter, search: Option<&str>) -> bool {
    let status = status(gateway);
    let status_ok = match filter {
        GatewayFilter::All => status != GatewayStatus::Retired,
        GatewayFilter::Online => status == GatewayStatus::Online,
        GatewayFilter::Degraded => status == GatewayStatus::Degraded,
        GatewayFilter::Offline => status == GatewayStatus::Offline,
        GatewayFilter::Unknown => status == GatewayStatus::Unknown,
        GatewayFilter::Retired => status == GatewayStatus::Retired,
    };
    let Some(needle) = search.map(str::trim).filter(|s| !s.is_empty()) else {
        return status_ok;
    };
    let needle = needle.to_lowercase();
    status_ok
        && [
            &gateway.lightning_alias,
            &gateway.gateway_id,
            &gateway.node_pub_key,
            &gateway.api_endpoint,
        ]
        .iter()
        .any(|value| value.to_lowercase().contains(&needle))
}

/// The page's activity score: funds + 3 × settles − 1.5 × cancels, plus a
/// small volume bonus; None without recorded activity
pub(crate) fn activity_score(gateway: &GatewayInfo) -> Option<i64> {
    let activity = gateway
        .activity_window
        .as_ref()
        .or(gateway.activity_7d.as_ref())?;
    let score = activity.fund_count as f64 + 3.0 * activity.settle_count as f64
        - 1.5 * activity.cancel_count as f64
        + 0.5 * (activity.total_volume_msat as f64 / 1_000_000.0).ln_1p();
    Some(score.round().max(0.0) as i64)
}

/// The row's position in the chosen order, unique per gateway (it ends with
/// the id), and the cursor a page continues after
pub(crate) type SortKey = (i64, i64, String);

pub(crate) fn sort_key(
    gateway: &GatewayInfo,
    sort: GatewaySort,
    direction: SortDirection,
) -> SortKey {
    let last_seen = gateway.last_seen.map_or(0, |at| at.timestamp_millis());
    let (primary, secondary) = match sort {
        GatewaySort::Freshness => (last_seen, 0),
        // Healthiest first ascending; within a status, the most recently seen
        GatewaySort::Status => {
            let rank = match status(gateway) {
                GatewayStatus::Online => 0,
                GatewayStatus::Degraded => 1,
                GatewayStatus::Offline => 2,
                GatewayStatus::Unknown => 3,
                GatewayStatus::Retired => 4,
            };
            (rank, -last_seen)
        }
        GatewaySort::Uptime => (
            (gateway.estimated_uptime_pct.unwrap_or(0.0) * 1000.0).round() as i64,
            0,
        ),
        GatewaySort::Activity => (activity_score(gateway).unwrap_or(-1), 0),
    };
    let id = gateway.gateway_id.clone();
    match direction {
        SortDirection::Asc => (primary, secondary, id),
        SortDirection::Desc => (-primary, -secondary, id),
    }
}

#[cfg(test)]
mod tests;
