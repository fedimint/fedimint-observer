use nostr_sdk::JsonUtil;
use serde_json::json;

use super::*;
use crate::manifold::discovery::{verify_event, MG_LEGACY_KIND};

const RELAY_EVENTS: &str = include_str!("../fixtures/relay-events.json");

fn real_ads() -> Vec<VerifiedAdvertisement> {
    let fixture: Value = serde_json::from_str(RELAY_EVENTS).unwrap();
    fixture["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| {
            verify_event(&nostr_sdk::Event::from_json(event.to_string()).unwrap()).unwrap()
        })
        .collect()
}

fn ad(kind: u16, payload: Value) -> VerifiedAdvertisement {
    VerifiedAdvertisement {
        kind,
        author: "aa".repeat(32),
        event_id: "bb".repeat(32),
        created_at: 1,
        issued_at: 1,
        expires_at: 2,
        payload,
    }
}

/// Real advertisements list in full, far below the bounds
#[test]
fn real_advertisements_list_unchanged() {
    for ad in real_ads() {
        let stored = listing(&ad).unwrap();
        if ad.kind == FLIP_KIND {
            let flip: FlipListing = serde_json::from_value(stored.clone()).unwrap();
            assert_eq!(
                stored,
                serde_json::to_value(flip_listing(&ad.payload)).unwrap()
            );
            assert!(!flip.api_endpoints.is_empty());
        } else {
            let mg: MgAdvertisement = serde_json::from_value(stored).unwrap();
            assert_eq!(mg.event_id, ad.event_id);
            assert_eq!(
                mg.service_pubkey,
                ad.payload["service_pubkey"].as_str().unwrap()
            );
            assert_eq!(
                mg.api_endpoints.len(),
                ad.payload["api_endpoints"].as_array().unwrap().len()
            );
            assert_eq!(mg.software.is_some(), ad.kind == MG_V2_KIND);
            assert!(mg.holder_authorization_count > 0);
        }
    }
}

#[test]
fn listings_are_bounded() {
    let long = "x".repeat(MAX_LISTED_TEXT + 1);
    let fits = "y".repeat(MAX_LISTED_TEXT);
    let endpoints: Vec<Value> = (0..100)
        .map(|n| json!({"transport": "https", "url": format!("https://{n}.example")}))
        .collect();
    let mut mixed = vec![json!({"transport": "https", "url": long})];
    mixed.extend(endpoints.clone());
    let uptime: serde_json::Map<String, Value> =
        (0..100).map(|n| (format!("{n}d"), json!(999))).collect();
    let mg = mg_listing(&ad(
        MG_V2_KIND,
        json!({
            "service_pubkey": long,
            "api_endpoints": mixed,
            "availability": {"fedimintd_version": fits, "federation_sizes": (0..1000).collect::<Vec<u32>>()},
            "plans": (0..100).map(|n| json!({"SubscriptionBased": {
                "initial_price_msats": n, "renewal_price_msats": n, "period": long
            }})).chain([json!({"InfiniteBestEffort": {"price_msats": 5}})]).collect::<Vec<_>>(),
            "uptime_permille": uptime,
            "version": {"mg_version": long, "api_version": 1},
        }),
    ));
    assert_eq!(mg.service_pubkey, "");
    assert_eq!(mg.api_endpoints.len(), MAX_LISTED_ITEMS);
    assert_eq!(mg.api_endpoints[0].url, "https://0.example");
    assert_eq!(mg.availability.fedimintd_version, fits);
    assert_eq!(mg.availability.federation_sizes.len(), MAX_LISTED_SIZES);
    assert_eq!(mg.plans.len(), 1);
    assert_eq!(
        mg.self_reported_uptime_permille.map(|u| u.len()),
        Some(MAX_LISTED_ITEMS)
    );
    assert!(mg.software.is_none());
    let legacy = mg_listing(&ad(MG_LEGACY_KIND, json!({"uptime_permille": {"1d": 1}})));
    assert!(legacy.self_reported_uptime_permille.is_none());

    let strings: Vec<Value> = (0..100).map(|n| json!(format!("s{n}"))).collect();
    let flip = flip_listing(&json!({
        "display": {"name": long, "website": fits, "contact": 7},
        "supported_sources": strings,
        "policy": {
            "supported_networks": strings,
            "accepted_attester_policies": (0..100).map(|_| json!({
                "attester_pubkey": "cc", "verification_requirement": "kyc"
            })).collect::<Vec<_>>(),
        },
        "api_endpoints": [long, "https://flip.example"],
        "api_versions": (0..100).collect::<Vec<u32>>(),
        "relay_hints": strings,
    }));
    let display = flip.display.unwrap();
    assert_eq!(display.name, None);
    assert_eq!(display.website.as_deref(), Some(fits.as_str()));
    assert_eq!(display.contact, None);
    for list in [
        &flip.supported_sources,
        &flip.supported_networks,
        &flip.relay_hints,
    ] {
        assert_eq!(list.len(), MAX_LISTED_ITEMS);
    }
    assert_eq!(flip.accepted_attester_policies.len(), MAX_LISTED_ITEMS);
    assert_eq!(flip.api_versions.len(), MAX_LISTED_ITEMS);
    assert_eq!(flip.api_endpoints, ["https://flip.example"]);
}
