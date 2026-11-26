use std::collections::BTreeMap;

use fmo_api_types::{
    FlipAttesterPolicy, FlipDisplay, MgAdvertisement, MgApiEndpoint, MgAvailability, MgPlan,
    MgSoftware,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::discovery::{VerifiedAdvertisement, FLIP_KIND, MG_V2_KIND};

pub const MAX_LISTED_ITEMS: usize = 32;
pub const MAX_LISTED_SIZES: usize = 256;
pub const MAX_LISTED_TEXT: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlipListing {
    pub display: Option<FlipDisplay>,
    pub supported_sources: Vec<String>,
    pub supported_networks: Vec<String>,
    pub accepted_attester_policies: Vec<FlipAttesterPolicy>,
    pub api_endpoints: Vec<String>,
    pub api_versions: Vec<u64>,
    pub relay_hints: Vec<String>,
}

/// The listing of a verified advertisement, as stored
pub fn listing(ad: &VerifiedAdvertisement) -> serde_json::Result<Value> {
    if ad.kind == FLIP_KIND {
        serde_json::to_value(flip_listing(&ad.payload))
    } else {
        serde_json::to_value(mg_listing(ad))
    }
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| text.len() <= MAX_LISTED_TEXT)
        .map(str::to_owned)
}

/// The first [`MAX_LISTED_ITEMS`] entries `item` accepts
fn items<T>(value: Option<&Value>, item: impl Fn(&Value) -> Option<T>) -> Vec<T> {
    first_items(value, MAX_LISTED_ITEMS, item)
}

fn first_items<T>(
    value: Option<&Value>,
    limit: usize,
    item: impl Fn(&Value) -> Option<T>,
) -> Vec<T> {
    value
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(item).take(limit).collect())
        .unwrap_or_default()
}

fn strings(value: Option<&Value>) -> Vec<String> {
    items(value, |entry| text(Some(entry)))
}

fn plan(value: &Value) -> Option<MgPlan> {
    if let Some(plan) = value.get("InfiniteBestEffort") {
        return Some(MgPlan::InfiniteBestEffort {
            price_msats: plan.get("price_msats")?.as_u64()?,
        });
    }
    let plan = value.get("SubscriptionBased")?;
    let period = plan
        .get("period")
        .map(|p| p.as_str().map_or_else(|| p.to_string(), str::to_owned))?;
    Some(MgPlan::Subscription {
        initial_price_msats: plan.get("initial_price_msats")?.as_u64()?,
        renewal_price_msats: plan.get("renewal_price_msats")?.as_u64()?,
        period: (period.len() <= MAX_LISTED_TEXT).then_some(period)?,
    })
}

pub fn mg_listing(ad: &VerifiedAdvertisement) -> MgAdvertisement {
    let p = &ad.payload;
    MgAdvertisement {
        event_kind: ad.kind,
        event_id: ad.event_id.clone(),
        issued_at: ad.issued_at,
        expires_at: ad.expires_at,
        signature_valid: true,
        service_pubkey: text(p.get("service_pubkey")).unwrap_or_default(),
        api_endpoints: items(p.get("api_endpoints"), |e| {
            Some(MgApiEndpoint {
                transport: text(e.get("transport"))?,
                url: text(e.get("url"))?,
            })
        }),
        availability: MgAvailability {
            fedimintd_version: text(p.pointer("/availability/fedimintd_version"))
                .unwrap_or_default(),
            federation_sizes: first_items(
                p.pointer("/availability/federation_sizes"),
                MAX_LISTED_SIZES,
                |n| n.as_u64().and_then(|n| u16::try_from(n).ok()),
            ),
        },
        plans: items(p.get("plans"), plan),
        self_reported_uptime_permille: (ad.kind == MG_V2_KIND)
            .then(|| p.get("uptime_permille").and_then(Value::as_object))
            .flatten()
            .map(|uptime| {
                uptime
                    .iter()
                    .filter(|(window, _)| window.len() <= MAX_LISTED_TEXT)
                    .filter_map(|(window, value)| {
                        Some((
                            window.clone(),
                            u16::try_from(value.as_u64()?).ok().filter(|v| *v <= 1000)?,
                        ))
                    })
                    .take(MAX_LISTED_ITEMS)
                    .collect::<BTreeMap<_, _>>()
            }),
        software: (ad.kind == MG_V2_KIND)
            .then(|| {
                Some(MgSoftware {
                    mg_version: text(p.pointer("/version/mg_version"))?,
                    api_version: p.pointer("/version/api_version")?.as_u64()?,
                })
            })
            .flatten(),
        holder_authorization_count: p
            .get("holder_authorizations")
            .and_then(Value::as_array)
            .map_or(0, |a| u32::try_from(a.len()).unwrap_or(u32::MAX)),
    }
}

pub fn flip_listing(p: &Value) -> FlipListing {
    FlipListing {
        display: p
            .get("display")
            .filter(|d| d.is_object())
            .map(|_| FlipDisplay {
                name: text(p.pointer("/display/name")),
                website: text(p.pointer("/display/website")),
                contact: text(p.pointer("/display/contact")),
            }),
        supported_sources: strings(p.get("supported_sources")),
        supported_networks: strings(p.pointer("/policy/supported_networks")),
        accepted_attester_policies: items(p.pointer("/policy/accepted_attester_policies"), |i| {
            Some(FlipAttesterPolicy {
                attester_pubkey: text(i.get("attester_pubkey"))?,
                verification_requirement: text(i.get("verification_requirement"))?,
            })
        }),
        api_endpoints: strings(p.get("api_endpoints")),
        api_versions: items(p.get("api_versions"), Value::as_u64),
        relay_hints: strings(p.get("relay_hints")),
    }
}

#[cfg(test)]
mod tests;
