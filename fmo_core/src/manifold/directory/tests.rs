use fmo_api_types::{
    AdFreshness, CredentialState, ManifoldGuardianSummary, MgAdvertisement, MgAvailability, MgPlan,
    MgSoftware, TrustCredential,
};

use super::{facets, matches, minor_version, sort_key, DirectoryQuery, DirectorySort};
use crate::pagination::keyset_page;

fn guardian(
    name: &str,
    key: &str,
    version: &str,
    price: Option<u64>,
    fresh: bool,
    issued: u64,
) -> ManifoldGuardianSummary {
    let advertisement = price.map(|price| MgAdvertisement {
        event_kind: 37709,
        event_id: "e".into(),
        issued_at: issued,
        expires_at: issued + 3600,
        signature_valid: true,
        service_pubkey: format!("service-{key}"),
        api_endpoints: Vec::new(),
        availability: MgAvailability {
            fedimintd_version: version.into(),
            federation_sizes: vec![4, 7],
        },
        plans: vec![MgPlan::InfiniteBestEffort { price_msats: price }],
        self_reported_uptime_permille: None,
        software: None::<MgSoftware>,
        holder_authorization_count: 1,
    });
    ManifoldGuardianSummary {
        fman_pubkey: key.into(),
        display_name: name.into(),
        freshness: advertisement.as_ref().map(|_| {
            if fresh {
                AdFreshness::Fresh
            } else {
                AdFreshness::Expired
            }
        }),
        advertisement,
        credential: TrustCredential {
            state: CredentialState::Unavailable,
            issuer_pubkey: None,
            issuer_label: None,
            holder_pubkey: Some(format!("holder-{key}")),
            trust_level: Some(9),
            required_trust_level: 9,
            schema: None,
            checked_at: None,
            checks: Vec::new(),
            reason: None,
        },
        seat_count: 0,
        federation_count: 0,
    }
}

fn directory() -> Vec<ManifoldGuardianSummary> {
    vec![
        guardian("cheap-owl", "aa", "0.12.0+fedi", Some(2_000_000), true, 100),
        guardian("dear-fox", "bb", "0.12.1", Some(5_000_000), true, 200),
        guardian("old-cat", "cc", "0.9.3", Some(1_000_000), false, 50),
        guardian("seat-only", "dd", "", None, false, 0),
    ]
}

fn page(query: &DirectoryQuery, limit: i64) -> Vec<String> {
    let rows: Vec<_> = directory()
        .into_iter()
        .filter(|g| matches(g, query))
        .collect();
    keyset_page(
        rows,
        |g| sort_key(g, query.sort),
        query.after.as_deref(),
        limit,
    )
    .unwrap()
    .items
    .into_iter()
    .map(|g| g.display_name)
    .collect()
}

#[test]
fn minor_versions_group_like_manifold() {
    assert_eq!(minor_version("0.12.0+fedi"), "0.12");
    assert_eq!(minor_version("0.9.3"), "0.9");
    assert_eq!(minor_version("weird"), "weird");
}

#[test]
fn filters_match_the_directory_rules() {
    let mut query = DirectoryQuery {
        search: Some("HOLDER-BB".into()),
        ..Default::default()
    };
    assert_eq!(page(&query, 10), vec!["dear-fox"]);
    query = DirectoryQuery {
        version: Some("0.12".into()),
        ..Default::default()
    };
    assert_eq!(page(&query, 10), vec!["dear-fox", "cheap-owl"]);
    query = DirectoryQuery {
        fresh_only: true,
        size: Some(7),
        ..Default::default()
    };
    assert_eq!(page(&query, 10).len(), 2);
    query = DirectoryQuery {
        verification: Some(CredentialState::Failed),
        ..Default::default()
    };
    assert!(page(&query, 10).is_empty());
}

#[test]
fn orders_put_unpriced_guardians_last_and_page_through_all() {
    let asc = DirectoryQuery {
        sort: DirectorySort::PriceAsc,
        ..Default::default()
    };
    assert_eq!(
        page(&asc, 10),
        vec!["old-cat", "cheap-owl", "dear-fox", "seat-only"]
    );
    let desc = DirectoryQuery {
        sort: DirectorySort::PriceDesc,
        ..Default::default()
    };
    assert_eq!(
        page(&desc, 10),
        vec!["dear-fox", "cheap-owl", "old-cat", "seat-only"]
    );
    // Freshness: fresh first, newest first, then expired, then no advertisement
    let fresh = DirectoryQuery::default();
    assert_eq!(
        page(&fresh, 10),
        vec!["dear-fox", "cheap-owl", "old-cat", "seat-only"]
    );

    // Two pages of two cover the list exactly once
    let rows = directory();
    let first = keyset_page(rows.clone(), |g| sort_key(g, DirectorySort::Name), None, 2).unwrap();
    let second = keyset_page(
        rows,
        |g| sort_key(g, DirectorySort::Name),
        first.next_after.as_deref(),
        2,
    )
    .unwrap();
    let names: Vec<_> = first
        .items
        .iter()
        .chain(&second.items)
        .map(|g| g.display_name.as_str())
        .collect();
    assert_eq!(names, vec!["cheap-owl", "dear-fox", "old-cat", "seat-only"]);
    assert!(second.next_after.is_none());
}

#[test]
fn facets_cover_every_guardian_newest_version_first() {
    let facets = facets(&directory());
    assert_eq!(facets.guardians, 4);
    assert_eq!(facets.versions, vec!["0.12", "0.9"]);
    assert_eq!(facets.sizes, vec![4, 7]);
}
