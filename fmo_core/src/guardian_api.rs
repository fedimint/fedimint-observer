//! Guardian API clients that close their connections when dropped
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

/// A client for `peers` whose connections end with its last clone
pub(crate) fn guardian_api(
    connectors: &ConnectorRegistry,
    peers: &BTreeMap<PeerId, SafeUrl>,
    api_secret: Option<String>,
) -> DynGlobalApi {
    GlobalFederationApiWithCache::new(OwnedConnectionsApi {
        peers: Arc::new(Peers {
            connectors: connectors.clone(),
            api_secret,
            peer_ids: peers.keys().copied().collect(),
            connections: peers
                .iter()
                .map(|(&peer_id, url)| (peer_id, PeerConnection::new(url.clone())))
                .collect(),
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
    api_secret: Option<String>,
    peer_ids: BTreeSet<PeerId>,
    connections: BTreeMap<PeerId, PeerConnection>,
}

#[derive(Debug)]
struct PeerConnection {
    url: SafeUrl,
    /// Held while connecting, so concurrent requests share one attempt
    state: tokio::sync::Mutex<ConnectionState>,
}

#[derive(Debug, Default)]
struct ConnectionState {
    connection: Option<DynGuaridianConnection>,
    failures: u32,
    retry_at: Option<Instant>,
}

impl PeerConnection {
    fn new(url: SafeUrl) -> Self {
        Self {
            url,
            state: tokio::sync::Mutex::new(ConnectionState::default()),
        }
    }
}

impl OwnedConnectionsApi {
    async fn connection(&self, peer_id: PeerId) -> ServerResult<DynGuaridianConnection> {
        let peer = self
            .peers
            .connections
            .get(&peer_id)
            .ok_or(ServerError::InvalidPeerId { peer_id })?;
        let mut state = peer.state.lock().await;
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
        let attempt = tokio::time::timeout(
            CONNECT_TIMEOUT,
            self.peers
                .connectors
                .connect_guardian(&peer.url, self.peers.api_secret.as_deref()),
        )
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
            Err(error) => {
                state.failures = state.failures.saturating_add(1);
                state.retry_at = Some(Instant::now() + retry_delay(state.failures));
                Err(error)
            }
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
