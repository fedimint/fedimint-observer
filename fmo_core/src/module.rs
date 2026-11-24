use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use fedimint_connectors::ConnectorRegistry;
use fedimint_core::config::{ClientConfig, FederationId, JsonClientConfig};
use fedimint_core::core::{
    Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleInstanceId, ModuleKind,
};
use fedimint_core::invite_code::InviteCode;
use fedimint_core::module::registry::ModuleDecoderRegistry;
use fedimint_core::{OutPoint, PeerId, TransactionId};

pub use crate::config::INVITE_FETCH_TIMEOUT;
pub use crate::error::{AppError as ApiError, ClientError, Result as ApiResult};
pub use crate::federation::db::Federation;
use crate::federation::observer::FederationObserver;
pub use crate::pagination::{key_text, keyset_page, KeysetPage, Page};
pub use crate::util::{module_instance_id, query, query_one, query_opt, query_value};

pub type DbTransaction<'a> = deadpool_postgres::Transaction<'a>;

#[derive(Debug, Clone, Copy)]
pub struct ItemMeta {
    pub federation_id: FederationId,
    pub module_instance_id: ModuleInstanceId,
    pub txid: TransactionId,
    pub session_index: u64,
    pub item_index: u64,
    pub index: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct CiMeta {
    pub federation_id: FederationId,
    pub module_instance_id: ModuleInstanceId,
    pub session_index: u64,
    pub item_index: u64,
    pub peer: PeerId,
}

#[derive(Debug, Default, Clone)]
pub struct InputSummary {
    pub amount_msat: Option<u64>,
    pub ln_contract_id: Option<Vec<u8>>,
    pub claimed_output: Option<OutPoint>,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Default, Clone)]
pub struct OutputSummary {
    pub amount_msat: Option<u64>,
    pub ln_contract: Option<(&'static str, Vec<u8>)>,
    pub details: Option<serde_json::Value>,
}

pub struct ProcessCtx<'a> {
    pub dbtx: &'a DbTransaction<'a>,
    pub config: &'a ClientConfig,
}

#[derive(Clone)]
pub struct Services(FederationObserver);

impl Services {
    /// Subscribe before reading work. Call `borrow_and_update` before each
    /// query, then wait for `changed` or the next database retry deadline.
    pub fn work_notifications(&self, topic: &str) -> tokio::sync::watch::Receiver<u64> {
        self.0.work_notifications.subscribe(topic)
    }
    /// Clamps past deadlines to zero. Preserve SQL NULL and wait for a
    /// notification when there is no deadline.
    pub fn wait_for_deadline(seconds: f64) -> std::time::Duration {
        std::time::Duration::try_from_secs_f64(seconds.max(0.0))
            .unwrap_or(std::time::Duration::from_secs(60))
    }
    /// Days of health history to keep (`FO_HEALTH_RETENTION_DAYS`).
    pub fn health_retention_days(&self) -> Option<u32> {
        Some(self.0.health_retention_days).filter(|days| *days > 0)
    }
    pub async fn external_io_permit(&self) -> anyhow::Result<tokio::sync::OwnedSemaphorePermit> {
        Ok(self.0.external_io.clone().acquire_owned().await?)
    }
    pub(crate) fn new(observer: FederationObserver) -> Self {
        Self(observer)
    }

    /// A pooled database connection
    pub async fn connection(&self) -> anyhow::Result<deadpool_postgres::Object> {
        self.0.connection().await
    }

    /// Connections to federations' guardians
    pub fn connectors(&self) -> &ConnectorRegistry {
        self.0.connectors()
    }

    pub fn federation_api(
        &self,
        global: &fedimint_core::config::GlobalClientConfig,
    ) -> anyhow::Result<fedimint_api_client::api::DynGlobalApi> {
        self.0.federation_api(global)
    }

    pub fn lookup_api(
        &self,
        global: &fedimint_core::config::GlobalClientConfig,
        api_secret: Option<String>,
    ) -> anyhow::Result<fedimint_api_client::api::DynGlobalApi> {
        self.0.lookup_api(global, api_secret)
    }

    /// Client of the configured mempool instance
    pub fn esplora(&self) -> &esplora_client::AsyncClient {
        self.0.esplora()
    }

    /// Every observed federation
    pub async fn federations(&self) -> anyhow::Result<Vec<Federation>> {
        self.0.list_federations().await
    }

    /// An observed federation, or `None` if the id is not one
    pub async fn federation(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Option<Federation>> {
        self.0.get_federation(federation_id).await
    }

    /// Fails unless the federation is observed.
    pub async fn ensure_observed(&self, federation_id: FederationId) -> anyhow::Result<()> {
        self.0.ensure_observed(federation_id).await
    }

    /// The config behind an invite code.
    pub async fn config_for_invite(&self, invite: &InviteCode) -> anyhow::Result<JsonClientConfig> {
        self.0
            .federation_config_cache()
            .fetch_config_cached(invite, &self.0)
            .await
    }
}

pub type Task = Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>;

/// The schema a module's tables live in, `fmo_<kind>`
pub fn schema_name(kind: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        !kind.is_empty()
            && kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "Module kind {kind:?} can't name a schema"
    );
    Ok(format!("fmo_{kind}"))
}

/// A module decoder and projector.
/// Summary methods avoid I/O; projector failures retry in isolation.
#[async_trait]
pub trait ObserverModule: Send + Sync + 'static {
    /// The module kind as it appears in client configs, e.g. `wallet`
    fn kind(&self) -> ModuleKind;

    fn decoder(&self) -> Decoder;

    fn migrations(&self) -> &'static [&'static str] {
        &[]
    }

    /// Materialized views in the module's schema.
    fn materialized_views(&self) -> &'static [&'static str] {
        &[]
    }

    /// Endpoints under `/federations/:federation_id`.
    fn federation_routes(&self) -> Option<Router<Services>> {
        None
    }

    /// Endpoints under `/config/:invite`.
    fn config_routes(&self) -> Option<Router<Services>> {
        None
    }

    fn federation_task(
        &self,
        _services: &Services,
        _federation_id: FederationId,
        _config: ClientConfig,
    ) -> Option<Task> {
        None
    }

    /// The module's background task for all federations.
    fn task(&self, _services: &Services) -> Option<Task> {
        None
    }

    /// Called after a projector batch commits.
    fn projection_committed(&self) {}

    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary>;

    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary>;

    /// JSON stored in `consensus_items`, or `None` to store nothing
    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value>;

    fn block_height_vote(&self, _ci: &DynModuleConsensusItem) -> Option<u32> {
        None
    }

    fn unix_time_vote(&self, _ci: &DynModuleConsensusItem) -> Option<u64> {
        None
    }

    /// No snapshot means unknown, not zero.
    fn reports_assets(&self) -> bool {
        false
    }

    /// Delete session-derived state for replay from session zero.
    /// Preserve task-owned data that session replay cannot rebuild.
    async fn reset_federation(
        &self,
        _ctx: &ProcessCtx<'_>,
        _federation_id: FederationId,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn process_input(
        &self,
        _ctx: &ProcessCtx<'_>,
        _meta: &ItemMeta,
        _input: &DynInput,
        _summary: &InputSummary,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn process_output(
        &self,
        _ctx: &ProcessCtx<'_>,
        _meta: &ItemMeta,
        _output: &DynOutput,
        _summary: &OutputSummary,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn process_ci(
        &self,
        _ctx: &ProcessCtx<'_>,
        _meta: &CiMeta,
        _ci: &DynModuleConsensusItem,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct ModuleRegistry {
    modules: Arc<BTreeMap<String, Arc<dyn ObserverModule>>>,
}

impl std::fmt::Debug for ModuleRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.modules.keys()).finish()
    }
}

impl ModuleRegistry {
    pub(crate) fn new(modules: Vec<Arc<dyn ObserverModule>>) -> anyhow::Result<Self> {
        let mut by_kind = BTreeMap::new();
        for module in modules {
            let kind = module.kind().to_string();
            anyhow::ensure!(
                by_kind.insert(kind.clone(), module).is_none(),
                "Module {kind} was added twice"
            );
        }
        Ok(Self {
            modules: Arc::new(by_kind),
        })
    }

    pub fn get(&self, kind: &str) -> Option<&dyn ObserverModule> {
        self.modules.get(kind).map(|module| module.as_ref())
    }

    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.modules.keys().map(String::as_str)
    }

    pub(crate) fn modules(&self) -> impl Iterator<Item = Arc<dyn ObserverModule>> + '_ {
        self.modules.values().cloned()
    }

    /// Every module's endpoints under `/federations/:federation_id`.
    pub(crate) fn federation_routes(&self) -> Option<Router<Services>> {
        self.modules()
            .filter_map(|module| module.federation_routes())
            .reduce(Router::merge)
    }

    /// Every module's endpoints under `/config/:invite`
    pub(crate) fn config_routes(&self) -> Option<Router<Services>> {
        self.modules()
            .filter_map(|module| module.config_routes())
            .reduce(Router::merge)
    }

    /// Decoders for the given module instances.
    fn decoders(
        &self,
        modules: impl IntoIterator<Item = (ModuleInstanceId, ModuleKind)>,
    ) -> ModuleDecoderRegistry {
        ModuleDecoderRegistry::new(modules.into_iter().filter_map(|(instance_id, kind)| {
            let decoder = self.get(kind.as_str())?.decoder();
            Some((instance_id, kind, decoder))
        }))
        .with_fallback()
    }

    /// Decoders for every module instance in a federation's config
    pub fn decoders_for_config(&self, config: &ClientConfig) -> ModuleDecoderRegistry {
        self.decoders(
            config
                .modules
                .iter()
                .map(|(instance_id, module)| (*instance_id, module.kind.clone())),
        )
    }
}

/// JSON details of an item that should be a `T`.
pub fn details_json<T: serde::Serialize + 'static>(
    item: &dyn std::any::Any,
    item_debug: &dyn std::fmt::Debug,
    label: &str,
) -> Option<serde_json::Value> {
    match item.downcast_ref::<T>() {
        Some(item) => {
            let value = serde_json::to_value(item).expect("Should be able to serialize to JSON");
            tracing::debug!("found {label}: {value:?}");
            Some(value)
        }
        None => {
            tracing::warn!("could not downcast (check decoders registry). {item_debug:?}");
            None
        }
    }
}
