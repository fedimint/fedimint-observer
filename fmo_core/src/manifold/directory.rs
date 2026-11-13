//! The Guardian directory's search, filters and order, applied on the server
//! so each page of `/manifold/guardians` is the right slice of the result.

use std::collections::BTreeSet;

use fmo_api_types::{
    AdFreshness, CredentialState, ManifoldDirectoryFacets, ManifoldGuardianSummary, MgPlan,
};
use serde::Deserialize;

use crate::pagination::key_text;

/// `GET /manifold/guardians` query; every filter is optional
#[derive(Debug, Default, Deserialize)]
pub(crate) struct DirectoryQuery {
    /// Matches the name, the Manifold Guardian key, the service key and the
    /// holder key
    pub search: Option<String>,
    pub verification: Option<CredentialState>,
    /// A minor version such as "0.12"
    pub version: Option<String>,
    pub size: Option<u16>,
    #[serde(default)]
    pub fresh_only: bool,
    #[serde(default)]
    pub sort: DirectorySort,
    pub after: Option<String>,
    pub limit: Option<u16>,
}

#[derive(Debug, Default, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DirectorySort {
    #[default]
    Freshness,
    PriceAsc,
    PriceDesc,
    Name,
}

/// "0.12.0+fedi" -> "0.12"; Manifold groups compatibility by major and minor
pub(crate) fn minor_version(version: &str) -> String {
    let mut parts = version.split(|c: char| !c.is_ascii_digit());
    match (parts.next(), parts.next()) {
        (Some(major), Some(minor)) if !major.is_empty() && !minor.is_empty() => {
            format!("{major}.{minor}")
        }
        _ => version.to_owned(),
    }
}

fn upfront_msats(plan: &MgPlan) -> u64 {
    match plan {
        MgPlan::InfiniteBestEffort { price_msats } => *price_msats,
        MgPlan::Subscription {
            initial_price_msats,
            ..
        } => *initial_price_msats,
    }
}

/// Lowest up-front price among the advertised plans
pub(crate) fn cheapest_msats(guardian: &ManifoldGuardianSummary) -> Option<u64> {
    guardian
        .advertisement
        .as_ref()?
        .plans
        .iter()
        .map(upfront_msats)
        .min()
}

pub(crate) fn matches(guardian: &ManifoldGuardianSummary, query: &DirectoryQuery) -> bool {
    let availability = guardian.advertisement.as_ref().map(|ad| &ad.availability);
    if let Some(search) = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let needle = search.to_lowercase();
        let found = [
            Some(guardian.display_name.as_str()),
            Some(guardian.fman_pubkey.as_str()),
            guardian
                .advertisement
                .as_ref()
                .map(|ad| ad.service_pubkey.as_str()),
            guardian.credential.holder_pubkey.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(&needle));
        if !found {
            return false;
        }
    }
    if query
        .verification
        .is_some_and(|state| guardian.credential.state != state)
    {
        return false;
    }
    if let Some(version) = &query.version {
        if !availability.is_some_and(|a| &minor_version(&a.fedimintd_version) == version) {
            return false;
        }
    }
    if let Some(size) = query.size {
        if !availability.is_some_and(|a| a.federation_sizes.contains(&size)) {
            return false;
        }
    }
    !(query.fresh_only && guardian.freshness != Some(AdFreshness::Fresh))
}

/// The row's position in the chosen order, unique per guardian (it ends with
/// the key), and the cursor a page continues after
pub(crate) type SortKey = (i64, i64, String, String);

pub(crate) fn sort_key(guardian: &ManifoldGuardianSummary, sort: DirectorySort) -> SortKey {
    let name = || key_text(&guardian.display_name);
    let key = guardian.fman_pubkey.clone();
    let signed = |msats: u64| i64::try_from(msats).unwrap_or(i64::MAX);
    match sort {
        DirectorySort::Name => (0, 0, name(), key),
        // Guardians without a price go last in both directions
        DirectorySort::PriceAsc => match cheapest_msats(guardian) {
            Some(price) => (0, signed(price), name(), key),
            None => (1, 0, name(), key),
        },
        DirectorySort::PriceDesc => match cheapest_msats(guardian) {
            Some(price) => (0, -signed(price), name(), key),
            None => (1, 0, name(), key),
        },
        DirectorySort::Freshness => {
            let rank = match guardian.freshness {
                Some(AdFreshness::Fresh) => 0,
                Some(AdFreshness::Stale) => 1,
                Some(AdFreshness::NotYetValid) => 2,
                Some(AdFreshness::Expired) => 3,
                None => 4,
            };
            let issued = guardian
                .advertisement
                .as_ref()
                .map_or(0, |ad| signed(ad.issued_at));
            (rank, -issued, name(), key)
        }
    }
}

/// Filter choices over every listed guardian, so they do not depend on the page
pub(crate) fn facets(guardians: &[ManifoldGuardianSummary]) -> ManifoldDirectoryFacets {
    let mut versions = BTreeSet::new();
    let mut sizes = BTreeSet::new();
    for availability in guardians
        .iter()
        .filter_map(|g| g.advertisement.as_ref().map(|ad| &ad.availability))
    {
        versions.insert(minor_version(&availability.fedimintd_version));
        sizes.extend(availability.federation_sizes.iter().copied());
    }
    let mut versions: Vec<String> = versions.into_iter().collect();
    // Newest first, comparing numerically ("0.10" after "0.9")
    versions.sort_by_key(|v| {
        std::cmp::Reverse(
            v.split('.')
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>(),
        )
    });
    ManifoldDirectoryFacets {
        guardians: guardians.len() as u64,
        versions,
        sizes: sizes.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests;
