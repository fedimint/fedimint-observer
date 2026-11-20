use std::str::FromStr;

use fedimint_core::bitcoin::hashes::{sha256, Hash as _};

use super::*;

fn federation(n: u8) -> FederationId {
    FederationId(sha256::Hash::hash(&[n]))
}

fn url(url: &str) -> SafeUrl {
    SafeUrl::from_str(url).expect("valid test URL")
}

fn peers(urls: &[&str]) -> ApiPeers {
    urls.iter()
        .enumerate()
        .map(|(peer, address)| (PeerId::from(peer as u16), url(address)))
        .collect()
}

fn small_limits() -> ApiLimits {
    ApiLimits {
        announced_endpoints: 16,
        urls_per_guardian: 2,
        selections: 4,
        lookup_pairs: 4,
        lookup_pairs_per_class: 2,
        lookup_clients: 4,
    }
}

async fn clients(limits: ApiLimits) -> anyhow::Result<ApiClients> {
    let connectors = ConnectorRegistry::build_from_testing_defaults()
        .bind()
        .await?;
    Ok(ApiClients::new(&connectors, limits))
}

#[tokio::test]
async fn hostile_guardian_exhausting_its_budget_holds_only_its_own_route() -> anyhow::Result<()> {
    let mut clients = clients(small_limits()).await?;
    let fed = federation(1);
    let canonical = peers(&["ws://g0/", "ws://g1/"]);
    for rotation in ["ws://a1/", "ws://a2/"] {
        clients.selected(fed, &canonical, &peers(&[rotation, "ws://g1/"]))?;
    }

    let (selected, _) = clients.selected(fed, &canonical, &peers(&["ws://a3/", "ws://b1/"]))?;
    assert_eq!(
        selected,
        peers(&["ws://a2/", "ws://b1/"]),
        "only guardian 0 is held"
    );
    assert_eq!(clients.held, HashSet::from([(fed, PeerId::from(0))]));

    // Another new URL keeps it held without a new transition.
    let (selected, _) = clients.selected(fed, &canonical, &peers(&["ws://a4/", "ws://b1/"]))?;
    assert_eq!(selected[&PeerId::from(0)], url("ws://a2/"));
    assert_eq!(clients.held.len(), 1);

    let other = federation(2);
    let (selected, _) = clients.selected(other, &peers(&["ws://h0/"]), &peers(&["ws://h1/"]))?;
    assert_eq!(
        selected,
        peers(&["ws://h1/"]),
        "other federations are unaffected"
    );

    clients.selected(fed, &canonical, &peers(&["ws://a1/", "ws://b1/"]))?;
    assert!(clients.held.is_empty(), "an admitted route clears the hold");
    Ok(())
}

#[tokio::test]
async fn announcements_cannot_exhaust_canonical_capacity() -> anyhow::Result<()> {
    let mut clients = clients(ApiLimits {
        announced_endpoints: 1,
        ..small_limits()
    })
    .await?;
    let first_canonical = peers(&["ws://canonical-1/"]);
    clients.selected(
        federation(1),
        &first_canonical,
        &peers(&["ws://announced-1/"]),
    )?;
    assert_eq!(clients.announced_urls.len(), 1);

    let second_canonical = peers(&["ws://canonical-2/", "ws://canonical-3/"]);
    let (selected, _) = clients.selected(federation(2), &second_canonical, &second_canonical)?;
    assert_eq!(selected, second_canonical);
    assert_eq!(clients.selections.len(), 2);
    assert_eq!(clients.announced_urls.len(), 1);
    Ok(())
}

#[tokio::test]
async fn selection_limit_bounds_federations_sharing_urls() -> anyhow::Result<()> {
    let mut clients = clients(ApiLimits {
        selections: 2,
        ..small_limits()
    })
    .await?;
    let shared = peers(&["ws://shared/"]);
    clients.selected(federation(1), &shared, &shared)?;
    clients.selected(federation(2), &shared, &shared)?;

    let rotated = peers(&["ws://rotated/"]);
    assert!(clients.selected(federation(3), &shared, &rotated).is_err());
    assert!(clients.canonical(federation(3), &shared).is_err());
    assert_eq!(clients.selections.len(), 2);
    assert!(clients.announced_urls.is_empty());
    assert!(clients.guardian_urls.is_empty());
    Ok(())
}

#[tokio::test]
async fn canonical_client_initializes_its_selection() -> anyhow::Result<()> {
    let mut clients = clients(small_limits()).await?;
    let canonical = peers(&["ws://g0/", "ws://g1/"]);
    clients.canonical(federation(1), &canonical)?;
    assert_eq!(clients.selections[&federation(1)].peers, canonical);
    assert!(clients.announced_urls.is_empty());
    Ok(())
}

#[tokio::test]
async fn already_admitted_url_needs_no_global_slot() -> anyhow::Result<()> {
    let mut clients = clients(ApiLimits {
        announced_endpoints: 1,
        ..small_limits()
    })
    .await?;
    let first = peers(&["ws://x/"]);
    let second = peers(&["ws://y/"]);
    clients.selected(federation(1), &first, &first)?;
    clients.selected(federation(2), &second, &peers(&["ws://z/"]))?;

    let announced = peers(&["ws://z/"]);
    let (selected, _) = clients.selected(federation(1), &first, &announced)?;
    assert_eq!(selected, announced, "a known URL takes no new global slot");
    assert_eq!(
        clients.guardian_urls[&(federation(1), PeerId::from(0))].len(),
        1
    );

    let (selected, _) = clients.selected(federation(1), &first, &peers(&["ws://w/"]))?;
    assert_eq!(selected, announced, "the global limit holds a new URL");
    Ok(())
}

#[tokio::test]
async fn literal_fallback_keeps_its_capacity() -> anyhow::Result<()> {
    let mut clients = clients(small_limits()).await?;
    let fed = federation(9);
    let invite = peers(&["ws://a/", "ws://b/"]);
    clients.lookup(LookupClass::Announced, fed, invite.clone(), None)?;
    assert!(
        clients
            .lookup(LookupClass::Announced, fed, peers(&["ws://c/"]), None)
            .is_err(),
        "the announced share is full"
    );

    // The identical route through the literal class reuses its pairs.
    clients.lookup(LookupClass::Literal, fed, invite, None)?;
    assert_eq!(clients.literal_lookup_pairs, 0);
    clients.lookup(LookupClass::Literal, fed, peers(&["ws://c/"]), None)?;
    assert_eq!(clients.literal_lookup_pairs, 1);
    assert_eq!(clients.lookup_urls.len(), 3);
    Ok(())
}

#[tokio::test]
async fn failed_lookup_creates_no_pool() -> anyhow::Result<()> {
    let mut clients = clients(ApiLimits {
        lookup_pairs_per_class: 1,
        ..small_limits()
    })
    .await?;
    let fed = federation(9);
    clients.lookup(
        LookupClass::Announced,
        fed,
        peers(&["ws://a/"]),
        Some("one".to_owned()),
    )?;
    assert!(clients
        .lookup(
            LookupClass::Announced,
            fed,
            peers(&["ws://b/"]),
            Some("two".to_owned()),
        )
        .is_err());
    assert_eq!(clients.lookup_pools.len(), 1);
    assert!(!clients.lookup_pools.contains_key(&Some("two".to_owned())));
    Ok(())
}

#[tokio::test]
async fn public_lookups_never_consume_announced_admission() -> anyhow::Result<()> {
    let mut clients = clients(ApiLimits {
        announced_endpoints: 1,
        ..small_limits()
    })
    .await?;
    let fed = federation(9);
    clients.lookup(
        LookupClass::Announced,
        fed,
        peers(&["ws://a/", "ws://b/"]),
        None,
    )?;
    clients.lookup(
        LookupClass::Literal,
        fed,
        peers(&["ws://c/", "ws://d/"]),
        None,
    )?;
    assert!(clients.announced_urls.is_empty());
    assert!(
        clients.selections.is_empty(),
        "lookups never create a selection"
    );

    let observed = peers(&["ws://observed/"]);
    clients.selected(federation(1), &observed, &observed)?;
    // A secret-less lookup of its route reuses the observed client.
    clients.lookup(LookupClass::Literal, federation(1), observed, None)?;
    assert_eq!(clients.lookup_urls.len(), 4);
    Ok(())
}
