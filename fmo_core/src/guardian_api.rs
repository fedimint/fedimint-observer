//! Bounded guardian connections shared by endpoint and credential
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use fedimint_api_client::api::global_api::with_cache::GlobalFederationApiWithCache;
use fedimint_api_client::api::{
    DynGlobalApi, DynModuleApi, IModuleFederationApi, IRawFederationApi, ServerError, ServerResult,
};
use fedimint_connectors::{ConnectorRegistry, DynGuaridianConnection, PeerStatus};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::module::{ApiMethod, ApiRequestErased};
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde_json::Value;

#[cfg(test)]
mod tests;

/// Bounds one connection attempt; requests queue behind it
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MIN_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(60);
/// Successful Iroh Next connections are retained by the pinned connector.
/// Never evict endpoint state, so connector-owned connections are bounded by
/// these admission limits even when route-client caches turn over.
const MAX_TRUSTED_CONNECTIONS: usize = 4096;
const MAX_LOOKUP_CONNECTIONS: usize = 512;
const MAX_LOOKUP_FALLBACK_CONNECTIONS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionClass {
    /// Canonical or signature-verified routes of observed federations
    Trusted,
    /// Public invite/config lookups, whose endpoints are caller-controlled
    Lookup,
    /// Literal invite routes, isolated so an announced attempt cannot consume
    /// the fallback's connection capacity
    LookupFallback,
}

/// Connection and retry state shared by every guardian API in this process.
#[derive(Debug, Clone)]
pub(crate) struct GuardianConnectionRegistry {
    inner: Arc<std::sync::Mutex<RegistryState>>,
    trusted_limit: usize,
    lookup_limit: usize,
    lookup_fallback_limit: usize,
}

#[derive(Debug, Default)]
struct RegistryState {
    connections: BTreeMap<ConnectionKey, Arc<PeerConnection>>,
    trusted: usize,
    lookups: usize,
    lookup_fallbacks: usize,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct ConnectionKey {
    url: SafeUrl,
    api_secret: Option<String>,
}

impl std::fmt::Debug for ConnectionKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectionKey")
            .field("url", &self.url)
            .field("has_api_secret", &self.api_secret.is_some())
            .finish()
    }
}

impl Default for GuardianConnectionRegistry {
    fn default() -> Self {
        Self::with_limits(
            MAX_TRUSTED_CONNECTIONS,
            MAX_LOOKUP_CONNECTIONS,
            MAX_LOOKUP_FALLBACK_CONNECTIONS,
        )
    }
}

impl GuardianConnectionRegistry {
    fn with_limits(
        trusted_limit: usize,
        lookup_limit: usize,
        lookup_fallback_limit: usize,
    ) -> Self {
        Self {
            inner: Arc::default(),
            trusted_limit,
            lookup_limit,
            lookup_fallback_limit,
        }
    }

    fn connection(
        &self,
        url: &SafeUrl,
        api_secret: Option<&str>,
        class: ConnectionClass,
    ) -> ServerResult<Arc<PeerConnection>> {
        let key = ConnectionKey {
            url: url.clone(),
            api_secret: api_secret.map(ToOwned::to_owned),
        };
        let mut inner = self
            .inner
            .lock()
            .expect("guardian connection registry lock poisoned");
        if let Some(connection) = inner.connections.get(&key) {
            return Ok(connection.clone());
        }
        let (used, limit) = match class {
            ConnectionClass::Trusted => (inner.trusted, self.trusted_limit),
            ConnectionClass::Lookup => (inner.lookups, self.lookup_limit),
            ConnectionClass::LookupFallback => (inner.lookup_fallbacks, self.lookup_fallback_limit),
        };
        if used >= limit {
            return Err(ServerError::Connection(anyhow!(
                "Guardian connection budget exhausted"
            )));
        }
        let connection = Arc::new(PeerConnection::default());
        inner.connections.insert(key, connection.clone());
        match class {
            ConnectionClass::Trusted => inner.trusted += 1,
            ConnectionClass::Lookup => inner.lookups += 1,
            ConnectionClass::LookupFallback => inner.lookup_fallbacks += 1,
        }
        Ok(connection)
    }
}

/// A client for `peers` using the process-wide bounded connection registry
pub(crate) fn guardian_api(
    connectors: &ConnectorRegistry,
    connections: &GuardianConnectionRegistry,
    peers: &BTreeMap<PeerId, SafeUrl>,
    api_secret: Option<String>,
    class: ConnectionClass,
) -> DynGlobalApi {
    GlobalFederationApiWithCache::new(OwnedConnectionsApi {
        peers: Arc::new(Peers {
            connectors: connectors.clone(),
            connections: connections.clone(),
            api_secret,
            class,
            peer_ids: peers.keys().copied().collect(),
            peer_urls: peers.clone(),
        }),
        module_id: None,
    })
    .into()
}

fn retry_delay(failures: u32) -> Duration {
    MIN_RETRY_DELAY
        .saturating_mul(1 << failures.saturating_sub(1).min(6))
        .min(MAX_RETRY_DELAY)
}

#[derive(Debug, Clone)]
struct OwnedConnectionsApi {
    peers: Arc<Peers>,
    module_id: Option<ModuleInstanceId>,
}

#[derive(Debug)]
struct Peers {
    connectors: ConnectorRegistry,
    connections: GuardianConnectionRegistry,
    api_secret: Option<String>,
    class: ConnectionClass,
    peer_ids: BTreeSet<PeerId>,
    peer_urls: BTreeMap<PeerId, SafeUrl>,
}

#[derive(Debug, Default)]
struct PeerConnection {
    /// Held while connecting, so concurrent requests share one attempt
    state: tokio::sync::Mutex<ConnectionState>,
}

#[derive(Debug, Default)]
struct ConnectionState {
    connection: Option<DynGuaridianConnection>,
    failures: u32,
    retry_at: Option<Instant>,
}

impl OwnedConnectionsApi {
    async fn connection(&self, peer_id: PeerId) -> ServerResult<DynGuaridianConnection> {
        let url = self
            .peers
            .peer_urls
            .get(&peer_id)
            .ok_or(ServerError::InvalidPeerId { peer_id })?;
        let peer = self.peers.connections.connection(
            url,
            self.peers.api_secret.as_deref(),
            self.peers.class,
        )?;
        peer.connection(|| {
            self.peers
                .connectors
                .connect_guardian(url, self.peers.api_secret.as_deref())
        })
        .await
    }
}

impl PeerConnection {
    async fn connection<F, Fut>(&self, connect: F) -> ServerResult<DynGuaridianConnection>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = ServerResult<DynGuaridianConnection>>,
    {
        let mut state = self.state.lock().await;
        if let Some(connection) = state.connection.as_ref().filter(|c| c.is_connected()) {
            return Ok(connection.clone());
        }
        state.connection = None;
        if state
            .retry_at
            .is_some_and(|retry_at| Instant::now() < retry_at)
        {
            return Err(ServerError::Connection(anyhow!(
                "Waiting to reconnect after a failed attempt"
            )));
        }
        // Record the attempt before awaiting it. If an outer request deadline
        // cancels this future, the next poll still observes the retry delay.
        state.failures = state.failures.saturating_add(1);
        state.retry_at = Some(Instant::now() + retry_delay(state.failures));
        let attempt = tokio::time::timeout(CONNECT_TIMEOUT, connect())
            .await
            .unwrap_or_else(|_| {
                Err(ServerError::Connection(anyhow!(
                    "Connection attempt timed out"
                )))
            });
        match attempt {
            Ok(connection) => {
                *state = ConnectionState {
                    connection: Some(connection.clone()),
                    ..ConnectionState::default()
                };
                Ok(connection)
            }
            Err(error) => Err(error),
        }
    }
}

impl IModuleFederationApi for OwnedConnectionsApi {}

#[async_trait::async_trait]
impl IRawFederationApi for OwnedConnectionsApi {
    fn all_peers(&self) -> &BTreeSet<PeerId> {
        &self.peers.peer_ids
    }

    fn self_peer(&self) -> Option<PeerId> {
        None
    }

    fn with_module(&self, id: ModuleInstanceId) -> DynModuleApi {
        Self {
            peers: self.peers.clone(),
            module_id: Some(id),
        }
        .into()
    }

    async fn request_raw(
        &self,
        peer_id: PeerId,
        method: &str,
        params: &ApiRequestErased,
    ) -> ServerResult<Value> {
        let method = match self.module_id {
            Some(module_id) => ApiMethod::Module(module_id, method.to_owned()),
            None => ApiMethod::Core(method.to_owned()),
        };
        self.connection(peer_id)
            .await?
            .request(method, params.clone())
            .await
    }

    // Unused by the observer
    fn connection_status_stream(&self) -> BoxStream<'static, BTreeMap<PeerId, PeerStatus>> {
        futures::stream::pending().boxed()
    }

    async fn wait_for_initialized_connections(&self) {
        self.peers
            .connectors
            .wait_for_initialized_connections()
            .await;
    }

    async fn get_peer_connection(&self, peer_id: PeerId) -> ServerResult<DynGuaridianConnection> {
        self.connection(peer_id).await
    }
}
