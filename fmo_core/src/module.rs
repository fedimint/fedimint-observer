//! The interface a Fedimint module implements to be observed, and the registry
//! of modules an observer build includes.
//!
//! Core fetches sessions and writes the module-agnostic rows (`transactions`,
//! `transaction_inputs`/`_outputs`, their `_details` and `consensus_items`),
//! asking the owning module for a summary of every item (amount and JSON
//! details). Items of modules this build doesn't include are stored with their
//! kind and no amount or details.
//!
//! A module's own tables are derived from the stored sessions afterwards, by
//! the module's projector (`federation/projector.rs`): one per module and
//! federation, it runs the module's `process_*` on the items of its kind,
//! batches of sessions at a time, each batch in one transaction with the move
//! of the module's cursor in `module_cursors`. A module added to a build later
//! starts at session 0 and also completes the core rows of its items from its
//! summaries, a module's bug is fixed by resetting its cursor, and a module
//! that fails holds back only itself. Rows a module writes may lag a moment
//! behind the core rows of the same session.
//!
//! A module's tables live in its own schema, `fmo_<kind>` (see
//! [`schema_name`]), created by core at startup. Core's tables stay in
//! `public`. Module SQL names its tables schema-qualified; only the module's
//! own migrations run with its schema first on the search path.

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
use fedimint_core::{PeerId, TransactionId};

pub use crate::error::{AppError as ApiError, Result as ApiResult};
pub use crate::federation::db::Federation;
use crate::federation::observer::FederationObserver;
pub use crate::util::{
    federation_api, module_instance_id, query, query_one, query_opt, query_value,
};

/// The database transaction a session is processed in
pub type DbTransaction<'a> = deadpool_postgres::Transaction<'a>;

/// Where a transaction input or output sits
#[derive(Debug, Clone, Copy)]
pub struct ItemMeta {
    pub federation_id: FederationId,
    /// Federations can run several instances of the same module kind
    pub module_instance_id: ModuleInstanceId,
    pub txid: TransactionId,
    pub session_index: u64,
    pub item_index: u64,
    /// Index of the input or output within its transaction
    pub index: u64,
}

/// Where a module consensus item sits, and who proposed it
#[derive(Debug, Clone, Copy)]
pub struct CiMeta {
    pub federation_id: FederationId,
    pub module_instance_id: ModuleInstanceId,
    pub session_index: u64,
    pub item_index: u64,
    pub peer: PeerId,
}

/// What core stores about an input in `transaction_inputs` and
/// `transaction_input_details`
#[derive(Debug, Default, Clone)]
pub struct InputSummary {
    pub amount_msat: Option<u64>,
    /// Consensus-encoded Lightning contract id, kept in a core column until the
    /// Lightning tables move into their module's schema
    pub ln_contract_id: Option<Vec<u8>>,
    pub details: Option<serde_json::Value>,
}

/// What core stores about an output in `transaction_outputs` and
/// `transaction_output_details`
#[derive(Debug, Default, Clone)]
pub struct OutputSummary {
    pub amount_msat: Option<u64>,
    /// How the output interacts with a Lightning contract (`fund`, `offer`,
    /// `cancel`) and the consensus-encoded contract id; core columns until the
    /// Lightning tables move into their module's schema
    pub ln_contract: Option<(&'static str, Vec<u8>)>,
    pub details: Option<serde_json::Value>,
}

/// Access a module gets while processing a batch of sessions
pub struct ProcessCtx<'a> {
    /// The batch's transaction, committed with the move of the module's cursor
    pub dbtx: &'a DbTransaction<'a>,
    pub config: &'a ClientConfig,
    /// Client of the configured mempool instance
    pub esplora: &'a esplora_client::AsyncClient,
}

/// What core offers a module's endpoints and background tasks
#[derive(Clone)]
pub struct Services(FederationObserver);

impl Services {
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

    /// Fails unless the federation is observed. For ids from request paths,
    /// before anything is cached for them.
    pub async fn ensure_observed(&self, federation_id: FederationId) -> anyhow::Result<()> {
        self.0.ensure_observed(federation_id).await
    }

    /// The config behind an invite code, downloaded from its guardians once.
    /// For federations that aren't observed.
    pub async fn config_for_invite(&self, invite: &InviteCode) -> anyhow::Result<JsonClientConfig> {
        self.0
            .federation_config_cache()
            .fetch_config_cached(invite, &self.0)
            .await
    }
}

/// A module's background task
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

/// A Fedimint module the observer can decode and process.
///
/// `summarize_*` and `block_height_vote` must not do I/O: core calls them
/// while storing a session, before writing its own rows. `process_*` run in
/// the module's projector, after the session and its core rows are committed,
/// so a module's tables may reference the core rows of the item. Return errors
/// rather than panicking: an error stalls only this module's projector for
/// this federation, which retries the batch 30 s later.
#[async_trait]
pub trait ObserverModule: Send + Sync + 'static {
    /// The module kind as it appears in client configs, e.g. `wallet`
    fn kind(&self) -> ModuleKind;

    fn decoder(&self) -> Decoder;

    /// Migrations of the module's own schema, appended to over time and never
    /// changed: core runs the ones a database hasn't seen, in order, each with
    /// the module's schema first on the search path so table names can be
    /// unqualified. The tables core's v12 moved into the schema are what the
    /// first one builds on.
    fn migrations(&self) -> &'static [&'static str] {
        &[]
    }

    /// Materialized views in the module's schema, schema-qualified, that core
    /// refreshes every minute
    fn materialized_views(&self) -> &'static [&'static str] {
        &[]
    }

    /// Endpoints under `/federations/:federation_id`, whose handlers get the
    /// id as a `Path<FederationId>` and [`Services`] as their state
    fn federation_routes(&self) -> Option<Router<Services>> {
        None
    }

    /// Endpoints under `/config/:invite`, for federations known by invite code
    /// only
    fn config_routes(&self) -> Option<Router<Services>> {
        None
    }

    /// The module's background task for one federation, if it has one for it.
    /// Core runs it supervised: 30 s after it ends or fails, it asks for the
    /// task again and runs that, so this must answer the same for a federation
    /// every time.
    fn federation_task(
        &self,
        _services: &Services,
        _federation_id: FederationId,
        _config: ClientConfig,
    ) -> Option<Task> {
        None
    }

    /// The module's background task for all federations, if it has one,
    /// supervised the same way
    fn task(&self, _services: &Services) -> Option<Task> {
        None
    }

    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary>;

    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary>;

    /// JSON stored in `consensus_items`, or `None` to store nothing
    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value>;

    /// The block height this consensus item votes for, if it is a vote. The
    /// votes date the session, so core records them in `block_height_votes`
    /// while storing it: `session_times`, a core read model, is built from
    /// them.
    fn block_height_vote(&self, _ci: &DynModuleConsensusItem) -> Option<u32> {
        None
    }

    /// Deletes everything the module derived from the federation's sessions,
    /// before a clean replay from session 0 rebuilds it. Tables the module's
    /// tasks keep, which no session feeds, stay.
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

/// The modules an observer build includes, by kind
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

    /// Every module's endpoints under `/federations/:federation_id`, or `None`
    /// when no module has any
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

    /// Decoders for the given module instances; instances of kinds this build
    /// doesn't include stay undecoded
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

/// JSON details of an item that should be a `T`. Logs and returns `None` if the
/// decoder registry produced something else.
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
