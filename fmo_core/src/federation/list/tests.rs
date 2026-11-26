use fedimint_core::config::FederationId;
use fedimint_core::Amount;
use fmo_api_types::{
    FederationActivity, FederationHealth, FederationRating, FederationSummary, NostrFederationEntry,
};

use super::{
    active_assets, activity_scale, available_assets, config_outline, group_for, group_of,
    matches_search, nostr_group_of, nostr_sort_key, rows_needing_session_ranges, sort_key,
    ActiveAssets, FederationGroup, FederationSort, NostrGroup, SortDirection,
};
use crate::pagination::keyset_page;

fn federation(
    n: u8,
    name: Option<&str>,
    deposits: Option<u64>,
    health: FederationHealth,
    expiry: Option<u64>,
    tx: u64,
) -> FederationSummary {
    FederationSummary {
        id: format!("{n:064x}").parse::<FederationId>().unwrap(),
        name: name.map(str::to_owned),
        last_7d_activity: vec![FederationActivity {
            num_transactions: tx,
            amount_transferred: Amount::from_msats(tx * 1000),
        }],
        deposits: deposits.map(Amount::from_msats),
        first_session_timestamp: None,
        last_session_timestamp: None,
        invite: format!("fed1invite{n}"),
        nostr_votes: FederationRating {
            count: 0,
            avg: None,
        },
        health,
        expiry_timestamp: expiry,
        successor: None,
        successor_id: None,
        successor_name: None,
        module_kinds: None,
        network: None,
    }
}

fn list() -> Vec<FederationSummary> {
    vec![
        federation(
            1,
            Some("Bravo"),
            Some(500),
            FederationHealth::Online,
            None,
            3,
        ),
        federation(
            2,
            Some("alpha"),
            Some(900),
            FederationHealth::Degraded,
            None,
            1,
        ),
        federation(3, None, None, FederationHealth::Online, None, 9),
        federation(
            4,
            Some("Down"),
            Some(10),
            FederationHealth::Offline,
            None,
            0,
        ),
        federation(
            5,
            Some("Gone"),
            Some(10),
            FederationHealth::Online,
            Some(100),
            0,
        ),
    ]
}

fn names(sort: FederationSort, dir: SortDirection) -> Vec<Option<String>> {
    let active: Vec<_> = list()
        .into_iter()
        .filter(|f| group_of(f, 1_000) == FederationGroup::Active)
        .collect();
    keyset_page(active, |f| sort_key(f, sort, dir), None, 10)
        .unwrap()
        .items
        .into_iter()
        .map(|f| f.name)
        .collect()
}

#[test]
fn groups_follow_the_home_page_rules() {
    let groups: Vec<_> = list().iter().map(|f| group_of(f, 1_000)).collect();
    use FederationGroup::*;
    assert_eq!(groups, [Active, Active, Active, Offline, ShutDown]);
    assert_eq!(group_of(&list()[4], 99), Active);
}

fn grouped(now: u64) -> Vec<(FederationGroup, Option<Amount>)> {
    list()
        .iter()
        .map(|f| (group_of(f, now), f.deposits))
        .collect()
}

#[test]
fn only_shut_down_rows_get_session_ranges() {
    let mut rows = list();
    assert_eq!(
        rows_needing_session_ranges(FederationGroup::ShutDown, &mut rows).len(),
        5
    );
    assert!(rows_needing_session_ranges(FederationGroup::Active, &mut rows).is_empty());
    assert!(rows_needing_session_ranges(FederationGroup::Offline, &mut rows).is_empty());
}

#[test]
fn active_assets_sum_only_the_active_list() {
    assert_eq!(
        active_assets(grouped(1_000)).unwrap(),
        ActiveAssets {
            held: Amount::from_msats(1_400),
            reporting: 2,
            unknown: 1,
        }
    );
    assert_eq!(
        active_assets(grouped(99)).unwrap().held,
        Amount::from_msats(1_410)
    );
}

#[test]
fn unknown_balances_never_read_as_a_measured_zero() {
    use FederationGroup::*;
    let none_report = active_assets([(Active, None), (Active, None)]).unwrap();
    assert_eq!((none_report.reporting, none_report.unknown), (0, 2));
    assert_eq!(none_report.known_held(), None);
    let zero = active_assets([(Active, Some(Amount::ZERO)), (Active, None)]).unwrap();
    assert_eq!(zero.known_held(), Some(Amount::ZERO));
    let empty = active_assets([(Offline, None), (ShutDown, Some(Amount::from_msats(5)))]);
    assert_eq!(empty.unwrap().known_held(), Some(Amount::ZERO));
}

#[test]
fn group_for_matches_group_of() {
    for now in [99, 1_000] {
        for f in list() {
            assert_eq!(
                group_for(f.expiry_timestamp, f.health, now),
                group_of(&f, now)
            );
        }
    }
}

#[test]
fn unavailable_asset_data_leaves_the_kpi_empty_not_an_error() {
    assert_eq!(
        available_assets(Err(anyhow::anyhow!("Negative wallet balance"))),
        None
    );
    let assets = ActiveAssets {
        held: Amount::from_msats(1_400),
        reporting: 2,
        unknown: 1,
    };
    assert_eq!(available_assets(Ok(assets)), Some(assets));
}

#[test]
fn unknown_values_sort_last_in_both_directions() {
    let some = |s: &str| Some(s.to_owned());
    assert_eq!(
        names(FederationSort::Assets, SortDirection::Desc),
        [some("alpha"), some("Bravo"), None]
    );
    assert_eq!(
        names(FederationSort::Assets, SortDirection::Asc),
        [some("Bravo"), some("alpha"), None]
    );
    assert_eq!(
        names(FederationSort::Name, SortDirection::Asc),
        [some("alpha"), some("Bravo"), None]
    );
    assert_eq!(
        names(FederationSort::Name, SortDirection::Desc),
        [some("Bravo"), some("alpha"), None]
    );
    assert_eq!(
        names(FederationSort::Transactions, SortDirection::Desc),
        [None, some("Bravo"), some("alpha")]
    );
}

#[test]
fn search_matches_name_or_invite_and_scale_spans_every_federation() {
    let all = list();
    assert!(matches_search(&all[1], Some(" ALPHA ")));
    assert!(matches_search(&all[2], Some("invite3")));
    assert!(!matches_search(&all[0], Some("alpha")));
    assert!(matches_search(&all[2], Some("fedimint://fed1invite3")));
    assert!(matches_search(&all[2], Some("WEB+FEDIMINT:fed1invite3")));
    let scale = activity_scale(&all);
    assert_eq!(scale.max_transactions, 9);
    assert_eq!(scale.max_volume_msat, 9000);
}

#[test]
fn search_matches_federation_id_prefix_or_long_fragment() {
    let mut fed = federation(1, None, None, FederationHealth::Online, None, 0);
    fed.id = "245806c0fa1a9412e8477ee3a4a7536d5b00c3c52a921d7b3da27d2c3df440e8"
        .parse()
        .unwrap();
    assert!(matches_search(&fed, Some("245806")));
    assert!(matches_search(&fed, Some("2458")));
    assert!(matches_search(&fed, Some("A9412E")));
    assert!(!matches_search(&fed, Some("a941")));
    assert!(!matches_search(&fed, Some("ffffff")));
    assert!(!matches_search(&fed, Some("fedimint://")));
}

#[test]
fn nostr_groups_split_like_the_home_page_and_named_ones_come_first() {
    let entry =
        |n: u8, name: Option<&str>, health: Option<FederationHealth>| NostrFederationEntry {
            id: format!("{n:064x}").parse().unwrap(),
            invite: format!("fed1invite{n}"),
            name: name.map(str::to_owned),
            health,
        };
    let entries = vec![
        entry(1, Some("zeta"), Some(FederationHealth::Online)),
        entry(2, Some("Alpha"), Some(FederationHealth::Degraded)),
        entry(3, None, Some(FederationHealth::Offline)),
        entry(4, None, None),
    ];
    let groups: Vec<_> = entries.iter().map(nostr_group_of).collect();
    assert_eq!(
        groups,
        [
            NostrGroup::Online,
            NostrGroup::Online,
            NostrGroup::Offline,
            NostrGroup::Unobserved
        ]
    );
    let mut online: Vec<_> = entries.into_iter().take(2).collect();
    online.sort_by_key(nostr_sort_key);
    let names: Vec<_> = online.iter().map(|e| e.name.as_deref().unwrap()).collect();
    assert_eq!(names, ["Alpha", "zeta"]);
}

/// Every row of `rows`.
fn walk<T: Clone, K>(rows: &[T], key: impl Fn(&T) -> K + Copy) -> Vec<T>
where
    K: Ord + serde::Serialize + serde::de::DeserializeOwned,
{
    let mut seen = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = keyset_page(rows.to_vec(), key, after.as_deref(), 1)
            .expect("every cursor the server generates is accepted");
        seen.extend(page.items);
        match page.next_after {
            Some(next) => after = Some(next),
            None => return seen,
        }
    }
}

/// Names of any length.
#[test]
fn long_names_page_through_every_order() {
    let long = |fill: &str, n: usize| fill.repeat(n);
    let names = [
        long("a", 400),
        long("a", 399) + "b",
        long("\u{1}", 400),
        long("ü", 400),
        long("𝔽", 400),
    ];
    let rows: Vec<_> = names
        .iter()
        .enumerate()
        .map(|(n, name)| {
            federation(
                n as u8 + 1,
                Some(name),
                Some(100 * n as u64),
                FederationHealth::Online,
                None,
                n as u64,
            )
        })
        .collect();
    for sort in [
        FederationSort::Assets,
        FederationSort::Volume,
        FederationSort::Transactions,
        FederationSort::Name,
    ] {
        for dir in [SortDirection::Asc, SortDirection::Desc] {
            let mut ids: Vec<_> = walk(&rows, |f| sort_key(f, sort, dir))
                .into_iter()
                .map(|f| f.id)
                .collect();
            assert_eq!(ids.len(), rows.len(), "{sort:?} {dir:?}");
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), rows.len(), "{sort:?} {dir:?} repeated a row");
        }
    }

    let entries: Vec<_> = rows
        .iter()
        .map(|f| NostrFederationEntry {
            id: f.id,
            invite: f.invite.clone(),
            name: f.name.clone(),
            health: Some(f.health),
        })
        .collect();
    assert_eq!(walk(&entries, nostr_sort_key).len(), entries.len());
}

#[test]
fn config_outline_reads_module_kinds_and_the_wallet_network() {
    let config = serde_json::json!({"modules": {
        "0": {"kind": "ln", "network": "signet"},
        "1": {"kind": "mint"},
        "2": {"kind": "wallet", "network": "bitcoin"},
        "3": {"kind": "meta"},
    }});
    let (mut kinds, network) = config_outline(&config);
    kinds.sort();
    assert_eq!(kinds, ["ln", "meta", "mint", "wallet"]);
    assert_eq!(network.as_deref(), Some("bitcoin"));
}

#[test]
fn config_outline_takes_the_walletv2_network_without_a_wallet_module() {
    let config = serde_json::json!({"modules": {
        "0": {"kind": "lnv2", "network": "signet"},
        "1": {"kind": "walletv2", "network": "bitcoin"},
    }});
    assert_eq!(config_outline(&config).1.as_deref(), Some("bitcoin"));
}

#[test]
fn config_outline_has_no_network_without_a_wallet_module() {
    let config = serde_json::json!({"modules": {"0": {"kind": "mint"}, "1": {}}});
    assert_eq!(
        config_outline(&config),
        (vec!["mint".to_owned(), "unknown".to_owned()], None)
    );
    assert_eq!(config_outline(&serde_json::json!({})), (Vec::new(), None));
}
