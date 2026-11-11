use fedimint_core::config::FederationId;
use fedimint_core::Amount;
use fmo_api_types::{
    FederationActivity, FederationHealth, FederationRating, FederationSummary, NostrFederationEntry,
};

use super::{
    activity_scale, group_of, matches_search, nostr_group_of, nostr_sort_key, sort_key,
    FederationGroup, FederationSort, NostrGroup, SortDirection,
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
    // An expired federation is shut down whatever its health
    assert_eq!(groups, [Active, Active, Active, Offline, ShutDown]);
    // Before its expiry it is just active
    assert_eq!(group_of(&list()[4], 99), Active);
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
    let scale = activity_scale(&all);
    assert_eq!(scale.max_transactions, 9);
    assert_eq!(scale.max_volume_msat, 9000);
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
