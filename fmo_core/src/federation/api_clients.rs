//! Guardian API clients and the admission that bounds their connections
use std::collections::{HashMap, HashSet};

use fedimint_api_client::api::DynGlobalApi;
use fedimint_connectors::ConnectorRegistry;
use fedimint_core::config::FederationId;
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;
use tracing::{info, warn};

use crate::error::ClientError;
use crate::guardian_api::{guardian_api, ApiPeers, ScopedPool};

#[cfg(test)]
mod tests;

/// Distinct trusted guardian URLs, canonical and announced
const MAX_TRUSTED_ENDPOINTS: usize = 4096;
/// Announced URLs each guardian may introduce
const MAX_URLS_PER_GUARDIAN: usize = 16;
/// Federations with a selected route
const MAX_SELECTIONS: usize = 4096;
/// Distinct invite lookup `(secret, URL)` pairs
const MAX_LOOKUP_PAIRS: usize = 1024;
/// Lookup pairs each [`LookupClass`] may introduce
const MAX_LOOKUP_PAIRS_PER_CLASS: usize = 512;
/// Cached lookup clients; evicting one frees no connection
const MAX_LOOKUP_CLIENTS: usize = 512;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ApiLimits {
    trusted_endpoints: usize,
    urls_per_guardian: usize,
    selections: usize,
    lookup_pairs: usize,
    lookup_pairs_per_class: usize,
    lookup_clients: usize,
}

impl Default for ApiLimits {
    fn default() -> Self {
        Self {
            trusted_endpoints: MAX_TRUSTED_ENDPOINTS,
            urls_per_guardian: MAX_URLS_PER_GUARDIAN,
            selections: MAX_SELECTIONS,
            lookup_pairs: MAX_LOOKUP_PAIRS,
            lookup_pairs_per_class: MAX_LOOKUP_PAIRS_PER_CLASS,
            lookup_clients: MAX_LOOKUP_CLIENTS,
        }
    }
}

#[cfg(test)]
impl ApiLimits {
    pub(crate) fn with_urls_per_guardian(urls_per_guardian: usize) -> Self {
        Self {
            urls_per_guardian,
            ..Self::default()
        }
    }
}

/// The invite route a lookup uses
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LookupClass {
    /// Stored signed routes
    Announced,
    /// The invite's own URLs
    Literal,
}

/// The route this process dials for an observed federation
struct Selection {
    peers: ApiPeers,
    api: DynGlobalApi,
    canonical_peers: ApiPeers,
    canonical_api: DynGlobalApi,
}

struct CachedApi {
    api: DynGlobalApi,
    last_used: u64,
}

pub(crate) struct ApiClients {
    limits: ApiLimits,
    connectors: ConnectorRegistry,
    /// Observed and canonical clients; no API secret
    trusted: ScopedPool,
    trusted_urls: HashSet<SafeUrl>,
    /// Announced URLs admitted per guardian
    guardian_urls: HashMap<(FederationId, PeerId), Vec<SafeUrl>>,
    selections: HashMap<FederationId, Selection>,
    /// Guardians whose latest route isn't admitted
    held: HashSet<(FederationId, PeerId)>,
    /// Per invite secret; created only after admission
    lookup_pools: HashMap<Option<String>, ScopedPool>,
    lookup_urls: HashMap<(Option<String>, SafeUrl), LookupClass>,
    announced_lookup_pairs: usize,
    literal_lookup_pairs: usize,
    /// Route-set wrappers; the pools hold the connections
    lookups: HashMap<(ApiPeers, Option<String>), CachedApi>,
    generation: u64,
}

impl std::fmt::Debug for ApiClients {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClients")
            .field("trusted_urls", &self.trusted_urls.len())
            .field("selections", &self.selections.len())
            .field("held", &self.held.len())
            .field("lookup_pools", &self.lookup_pools.len())
            .field("lookup_urls", &self.lookup_urls.len())
            .field("lookups", &self.lookups.len())
            .finish_non_exhaustive()
    }
}

impl ApiClients {
    pub(crate) fn new(connectors: &ConnectorRegistry, limits: ApiLimits) -> Self {
        Self {
            limits,
            connectors: connectors.clone(),
            trusted: ScopedPool::new(connectors, None),
            trusted_urls: HashSet::new(),
            guardian_urls: HashMap::new(),
            selections: HashMap::new(),
            held: HashSet::new(),
            lookup_pools: HashMap::new(),
            lookup_urls: HashMap::new(),
            announced_lookup_pairs: 0,
            literal_lookup_pairs: 0,
            lookups: HashMap::new(),
            generation: 0,
        }
    }

    /// Trusted URLs admitted, and guardians held on an older route
    pub(crate) fn admission_status(&self) -> (usize, usize) {
        (self.trusted_urls.len(), self.held.len())
    }

    /// The federation's selection; admits its canonical URLs all or nothing
    fn ensure_selection(
        &mut self,
        federation_id: FederationId,
        canonical: &ApiPeers,
    ) -> anyhow::Result<&mut Selection> {
        if !self.selections.contains_key(&federation_id) {
            if self.selections.len() >= self.limits.selections {
                return Err(ClientError::unavailable(
                    "Too many federations observed; try again after the observer restarts",
                )
                .into());
            }
            let new_urls = canonical
                .values()
                .filter(|url| !self.trusted_urls.contains(*url))
                .cloned()
                .collect::<HashSet<_>>();
            if self.trusted_urls.len() + new_urls.len() > self.limits.trusted_endpoints {
                return Err(ClientError::unavailable(
                    "Too many guardian endpoints; try again after the observer restarts",
                )
                .into());
            }
            self.trusted_urls.extend(new_urls);
            let api = guardian_api(&self.trusted, canonical.clone());
            self.selections.insert(
                federation_id,
                Selection {
                    peers: canonical.clone(),
                    api: api.clone(),
                    canonical_peers: canonical.clone(),
                    canonical_api: api,
                },
            );
        }
        Ok(self
            .selections
            .get_mut(&federation_id)
            .expect("selection exists or was just inserted"))
    }

    /// Whether the guardian may dial `url`, admitting it if there's room
    fn admit_announced(
        &mut self,
        federation_id: FederationId,
        peer_id: PeerId,
        url: &SafeUrl,
    ) -> bool {
        let urls = self
            .guardian_urls
            .entry((federation_id, peer_id))
            .or_default();
        if urls.contains(url) {
            return true;
        }
        let needs_slot = !self.trusted_urls.contains(url);
        if urls.len() >= self.limits.urls_per_guardian
            || (needs_slot && self.trusted_urls.len() >= self.limits.trusted_endpoints)
        {
            return false;
        }
        urls.push(url.clone());
        self.trusted_urls.insert(url.clone());
        true
    }

    /// Each guardian's latest URL if admitted, else its previous one
    pub(crate) fn selected(
        &mut self,
        federation_id: FederationId,
        canonical: &ApiPeers,
        latest: &ApiPeers,
    ) -> anyhow::Result<(ApiPeers, DynGlobalApi)> {
        let previous = self
            .ensure_selection(federation_id, canonical)?
            .peers
            .clone();
        let mut peers = ApiPeers::new();
        for (&peer_id, url) in latest {
            let admitted = canonical.get(&peer_id) == Some(url)
                || self.admit_announced(federation_id, peer_id, url);
            if admitted {
                if self.held.remove(&(federation_id, peer_id)) {
                    info!(%federation_id, %peer_id, "Guardian is back on an admitted route");
                }
                peers.insert(peer_id, url.clone());
                continue;
            }
            if self.held.insert((federation_id, peer_id)) {
                warn!(
                    %federation_id,
                    %peer_id,
                    urls_per_guardian = self.limits.urls_per_guardian,
                    trusted_endpoints = self.trusted_urls.len(),
                    "Guardian route not admitted; keeping its previous route until restart"
                );
            }
            if let Some(url) = previous.get(&peer_id) {
                peers.insert(peer_id, url.clone());
            }
        }
        let selection = self
            .selections
            .get_mut(&federation_id)
            .expect("selection was ensured above");
        if selection.peers != peers {
            selection.api = guardian_api(&self.trusted, peers.clone());
            selection.peers = peers.clone();
        }
        Ok((peers, selection.api.clone()))
    }

    /// The client for the federation's canonical routes
    pub(crate) fn canonical(
        &mut self,
        federation_id: FederationId,
        canonical: &ApiPeers,
    ) -> anyhow::Result<DynGlobalApi> {
        Ok(self
            .ensure_selection(federation_id, canonical)?
            .canonical_api
            .clone())
    }

    /// An invite lookup client; its URLs are admitted all or nothing
    pub(crate) fn lookup(
        &mut self,
        class: LookupClass,
        federation_id: FederationId,
        peers: ApiPeers,
        api_secret: Option<String>,
    ) -> anyhow::Result<DynGlobalApi> {
        // Reuses an observed client; this admits nothing.
        if api_secret.is_none() {
            if let Some(selection) = self.selections.get(&federation_id) {
                if selection.peers == peers {
                    return Ok(selection.api.clone());
                }
                if selection.canonical_peers == peers {
                    return Ok(selection.canonical_api.clone());
                }
            }
        }

        let new_pairs = peers
            .values()
            .map(|url| (api_secret.clone(), url.clone()))
            .filter(|pair| !self.lookup_urls.contains_key(pair))
            .collect::<HashSet<_>>();
        let introduced = match class {
            LookupClass::Announced => &mut self.announced_lookup_pairs,
            LookupClass::Literal => &mut self.literal_lookup_pairs,
        };
        if *introduced + new_pairs.len() > self.limits.lookup_pairs_per_class
            || self.lookup_urls.len() + new_pairs.len() > self.limits.lookup_pairs
        {
            return Err(ClientError::unavailable(
                "Too many federations looked up; try again after the observer restarts",
            )
            .into());
        }
        *introduced += new_pairs.len();
        self.lookup_urls
            .extend(new_pairs.into_iter().map(|pair| (pair, class)));

        let pool = self
            .lookup_pools
            .entry(api_secret.clone())
            .or_insert_with(|| ScopedPool::new(&self.connectors, api_secret.clone()))
            .clone();
        self.generation = self.generation.wrapping_add(1);
        Ok(cached_api(
            &mut self.lookups,
            self.generation,
            (peers, api_secret),
            self.limits.lookup_clients,
            |(peers, _)| guardian_api(&pool, peers.clone()),
        ))
    }
}

/// Reuses `key`'s client or adds one, evicting the least recently used at `max`
fn cached_api<K: Clone + Eq + std::hash::Hash>(
    map: &mut HashMap<K, CachedApi>,
    generation: u64,
    key: K,
    max: usize,
    build: impl FnOnce(&K) -> DynGlobalApi,
) -> DynGlobalApi {
    if let Some(entry) = map.get_mut(&key) {
        entry.last_used = generation;
        return entry.api.clone();
    }
    if map.len() >= max {
        if let Some(oldest) = map
            .iter()
            .min_by_key(|(_, entry)| entry.last_used)
            .map(|(key, _)| key.clone())
        {
            map.remove(&oldest);
        }
    }
    let api = build(&key);
    map.insert(
        key,
        CachedApi {
            api: api.clone(),
            last_used: generation,
        },
    );
    api
}
