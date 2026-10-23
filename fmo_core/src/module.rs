//! The interface a Fedimint module implements to be observed, and the registry
//! of modules an observer build includes.
//!
//! Core fetches sessions and writes the module-agnostic rows (`transactions`,
//! `transaction_inputs`/`_outputs`, their `_details` and `consensus_items`).
//! For every item it first asks the owning module for a summary (amount and
//! JSON details), writes its own row from it, and then lets the module write
//! its own tables. Items of modules this build doesn't include are stored with
//! their kind and no amount or details. Adding the module to a build later
//! doesn't fill those in yet: core rows are written once (`ON CONFLICT DO
//! NOTHING`), so that needs the replay of a later step of issue #8.

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::{
    Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleInstanceId, ModuleKind,
};
use fedimint_core::module::registry::ModuleDecoderRegistry;
use fedimint_core::{PeerId, TransactionId};

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

/// Access a module gets while processing one session
pub struct ProcessCtx<'a> {
    pub dbtx: &'a DbTransaction<'a>,
    pub config: &'a ClientConfig,
    /// Esplora API of the configured mempool instance
    pub mempool_url: &'a str,
}

/// A Fedimint module the observer can decode and process.
///
/// `summarize_*` must not do I/O: core calls them before writing its own row.
/// `process_*` run afterwards in the same database transaction, so a module's
/// tables may reference the core rows of the item.
#[async_trait]
pub trait ObserverModule: Send + Sync + 'static {
    /// The module kind as it appears in client configs, e.g. `wallet`
    fn kind(&self) -> ModuleKind;

    fn decoder(&self) -> Decoder;

    fn summarize_input(&self, input: &DynInput) -> InputSummary;

    fn summarize_output(&self, output: &DynOutput) -> OutputSummary;

    /// JSON stored in `consensus_items`, or `None` to store nothing
    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value>;

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
#[derive(Clone, Default)]
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

    /// Decoders for the given module instances; instances of kinds this build
    /// doesn't include stay undecoded
    pub fn decoders(
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
