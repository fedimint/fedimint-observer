use chrono::{TimeZone, Utc};
use fmo_api_types::{GatewayActivityMetrics, GatewayInfo, GatewayStatus};
use fmo_core::module::keyset_page;

use super::{activity_score, matches, sort_key, GatewayFilter, GatewaySort, SortDirection};

fn gateway(
    id: &str,
    status: GatewayStatus,
    seen_min: i64,
    uptime: f64,
    funds: Option<u64>,
) -> GatewayInfo {
    GatewayInfo {
        gateway_id: id.into(),
        node_pub_key: format!("node-{id}"),
        lightning_alias: format!("alias-{id}"),
        api_endpoint: format!("https://{id}.example/v1"),
        vetted: false,
        raw: None,
        first_seen: None,
        last_seen: Some(Utc.timestamp_opt(1_800_000_000 + seen_min * 60, 0).unwrap()),
        activity_7d: None,
        activity_window: funds.map(|fund_count| GatewayActivityMetrics {
            fund_count,
            settle_count: 0,
            cancel_count: 0,
            total_volume_msat: 0,
        }),
        uptime_window: None,
        metrics_window: None,
        status: Some(status),
        coverage_pct: None,
        estimated_uptime_pct: Some(uptime),
        estimated_online_minutes: None,
        estimated_offline_minutes: None,
        estimated_unknown_minutes: None,
    }
}

fn rows() -> Vec<GatewayInfo> {
    vec![
        gateway("a", GatewayStatus::Online, 10, 99.0, Some(5)),
        gateway("b", GatewayStatus::Offline, 5, 40.0, None),
        gateway("c", GatewayStatus::Online, 20, 100.0, Some(1)),
        gateway("d", GatewayStatus::Retired, 0, 0.0, None),
    ]
}

fn ids(
    filter: GatewayFilter,
    search: Option<&str>,
    sort: GatewaySort,
    dir: SortDirection,
) -> Vec<String> {
    let matching: Vec<_> = rows()
        .into_iter()
        .filter(|g| matches(g, filter, search))
        .collect();
    keyset_page(matching, |g| sort_key(g, sort, dir), None, 10)
        .unwrap()
        .items
        .into_iter()
        .map(|g| g.gateway_id)
        .collect()
}

#[test]
fn all_leaves_out_retired_gateways_and_search_spans_every_identifier() {
    assert_eq!(
        ids(
            GatewayFilter::All,
            None,
            GatewaySort::Freshness,
            SortDirection::Desc
        ),
        ["c", "a", "b"]
    );
    assert_eq!(
        ids(
            GatewayFilter::Retired,
            None,
            GatewaySort::Freshness,
            SortDirection::Desc
        ),
        ["d"]
    );
    assert_eq!(
        ids(
            GatewayFilter::All,
            Some("NODE-B"),
            GatewaySort::Freshness,
            SortDirection::Desc
        ),
        ["b"]
    );
    assert_eq!(
        ids(
            GatewayFilter::All,
            Some("c.example"),
            GatewaySort::Freshness,
            SortDirection::Desc
        ),
        ["c"]
    );
}

#[test]
fn every_order_reverses_with_its_direction() {
    assert_eq!(
        ids(
            GatewayFilter::All,
            None,
            GatewaySort::Uptime,
            SortDirection::Desc
        ),
        ["c", "a", "b"]
    );
    assert_eq!(
        ids(
            GatewayFilter::All,
            None,
            GatewaySort::Uptime,
            SortDirection::Asc
        ),
        ["b", "a", "c"]
    );
    // Status ascending: online first, the most recently seen online first
    assert_eq!(
        ids(
            GatewayFilter::All,
            None,
            GatewaySort::Status,
            SortDirection::Asc
        ),
        ["c", "a", "b"]
    );
    // Activity: no recorded activity ranks below any score
    assert_eq!(
        ids(
            GatewayFilter::All,
            None,
            GatewaySort::Activity,
            SortDirection::Desc
        ),
        ["a", "c", "b"]
    );
}

#[test]
fn activity_score_matches_the_page_formula() {
    let mut g = gateway("x", GatewayStatus::Online, 0, 0.0, Some(76));
    let activity = g.activity_window.as_mut().unwrap();
    activity.settle_count = 1;
    activity.cancel_count = 75;
    activity.total_volume_msat = 23_700_000;
    // 76 + 3 − 112.5 + 0.5·ln(24.7) < 0, so clamped to zero
    assert_eq!(activity_score(&g), Some(0));
    assert_eq!(
        activity_score(&gateway("y", GatewayStatus::Online, 0, 0.0, None)),
        None
    );
}
