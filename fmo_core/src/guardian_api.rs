use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::sync::Arc;

use fedimint_api_client::api::global_api::with_cache::GlobalFederationApiWithCache;
use fedimint_api_client::api::{
    DynGlobalApi, DynModuleApi, IModuleFederationApi, IRawFederationApi, ServerError, ServerResult,
};
use fedimint_connectors::{
    ConnectionPool, ConnectorRegistry, DynGuaridianConnection, IGuardianConnection, PeerStatus,
};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::module::{ApiMethod, ApiRequestErased};
use fedimint_core::util::SafeUrl;
use fedimint_core::PeerId;
use futures::stream::BoxStream;
use futures::StreamExt;
use serde_json::Value;

#[cfg(test)]
mod tests;

pub(crate) type ApiPeers = BTreeMap<PeerId, SafeUrl>;

#[derive(Clone)]
pub(crate) struct ScopedPool {
    pool: ConnectionPool<dyn IGuardianConnection>,
    api_secret: Option<String>,
}

impl std::fmt::Debug for ScopedPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedPool")
            .field(
                "api_secret",
                &self.api_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish_non_exhaustive()
    }
}

impl ScopedPool {
    pub(crate) fn new(connectors: &ConnectorRegistry, api_secret: Option<String>) -> Self {
        Self {
            pool: ConnectionPool::new(connectors.clone()),
            api_secret,
        }
    }
}

/// A client for `peers` whose connections live in `scope`
pub(crate) fn guardian_api(scope: &ScopedPool, peers: ApiPeers) -> DynGlobalApi {
    GlobalFederationApiWithCache::new(PooledApi {
        scope: scope.clone(),
        peer_ids: Arc::new(peers.keys().copied().collect()),
        peers: Arc::new(peers),
        module_id: None,
    })
    .into()
}

/// Fedimint's pool handles dialing, backoff and sharing a URL
async fn connection_with<F, Fut>(
    scope: &ScopedPool,
    url: &SafeUrl,
    connect: F,
) -> ServerResult<DynGuaridianConnection>
where
    F: Fn(SafeUrl, Option<String>, ConnectorRegistry) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = ServerResult<DynGuaridianConnection>> + Send + 'static,
{
    scope
        .pool
        .get_or_create_connection(url, scope.api_secret.as_deref(), connect)
        .await
}

#[derive(Debug, Clone)]
struct PooledApi {
    scope: ScopedPool,
    peers: Arc<ApiPeers>,
    peer_ids: Arc<BTreeSet<PeerId>>,
    module_id: Option<ModuleInstanceId>,
}

impl PooledApi {
    async fn connection(&self, peer_id: PeerId) -> ServerResult<DynGuaridianConnection> {
        let url = self
            .peers
            .get(&peer_id)
            .ok_or(ServerError::InvalidPeerId { peer_id })?;
        connection_with(&self.scope, url, |url, secret, connectors| async move {
            connectors.connect_guardian(&url, secret.as_deref()).await
        })
        .await
    }
}

impl IModuleFederationApi for PooledApi {}

#[async_trait::async_trait]
impl IRawFederationApi for PooledApi {
    fn all_peers(&self) -> &BTreeSet<PeerId> {
        &self.peer_ids
    }

    fn self_peer(&self) -> Option<PeerId> {
        None
    }

    fn with_module(&self, id: ModuleInstanceId) -> DynModuleApi {
        Self {
            module_id: Some(id),
            ..self.clone()
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
        self.scope.pool.wait_for_initialized_connections().await;
    }

    async fn get_peer_connection(&self, peer_id: PeerId) -> ServerResult<DynGuaridianConnection> {
        self.connection(peer_id).await
    }
}
