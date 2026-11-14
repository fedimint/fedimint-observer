//! Module-agnostic core of Fedimint Observer.
//!
//! An observer build picks its modules and data sources and runs:
//!
//! ```ignore
//! FedimintObserver::builder()
//!     .with_module(MintObserver)
//!     .with_module(WalletObserver::default())
//!     .with_module(LnObserver::default())
//!     .with_source(MyProbe::default())
//!     .run()
//!     .await
//! ```
//!
//! Core is layered: collection (`federation/ingest.rs`, `guardians.rs`,
//! `metadata.rs`, [`source`]), processing (`federation/projector.rs`),
//! storage (`store`), recommendations (`recommend`) and presentation (`api`,
//! and the handlers next to their read queries). `ARCHITECTURE.md` at the
//! repository root describes the layers, the data flow and the extension
//! points.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::config::meta::{ConsensusMetaCache, MetaOverrideCache};
use crate::config::FederationConfigCache;
use crate::federation::observer::FederationObserver;
use crate::module::{ModuleRegistry, ObserverModule, Services};
use crate::source::{ObservationSource, SourceRegistry};

/// The HTTP API: assembles every layer's routes
mod api;
/// Fedimint config fetching service implementation
mod config;
/// HTTP error responses with sanitized internal errors.
mod error;
mod federation;
mod http;
mod meta;
pub mod module;
mod pagination;
mod recommend;
mod runtime;
pub mod source;
mod store;
mod task;
mod util;
mod work;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, env = "FO_BIND", default_value = "127.0.0.1:3000")]
    bind: String,

    #[arg(long, env = "FO_DATABASE")]
    database: String,

    #[arg(long, env = "FO_ADMIN_AUTH")]
    admin_auth: String,

    #[arg(
        long,
        env = "FO_MEMPOOL_URL",
        default_value = "https://mempool.space/api"
    )]
    mempool_url: String,

    /// JSON document declaring the sources that push observations over HTTP;
    /// each source's token is read from `FO_SOURCE_TOKEN_<ID>`
    #[arg(long, env = "FO_SOURCES_FILE")]
    sources_file: Option<PathBuf>,

    #[command(flatten)]
    runtime: runtime::RuntimeOptions,
}

#[derive(Debug, Clone)]
struct AppState {
    federation_config_cache: FederationConfigCache,
    meta_override_cache: MetaOverrideCache,
    consensus_meta_cache: ConsensusMetaCache,
    federation_observer: FederationObserver,
    http_metrics: Arc<http::HttpMetrics>,
}

/// Entry point of an observer build
pub struct FedimintObserver;

impl FedimintObserver {
    pub fn builder() -> FedimintObserverBuilder {
        FedimintObserverBuilder::default()
    }
}

#[derive(Default)]
pub struct FedimintObserverBuilder {
    modules: Vec<Arc<dyn ObserverModule>>,
    sources: Vec<Arc<dyn ObservationSource>>,
}

impl FedimintObserverBuilder {
    /// Decode and process this module's items. A module added to a build
    /// later fills its own tables from the stored sessions, and completes the
    /// core rows of its items, which were stored without amounts or details.
    pub fn with_module(mut self, module: impl ObserverModule) -> Self {
        self.modules.push(Arc::new(module));
        self
    }

    /// Collect observations from a source other than the guardians, such as
    /// an external observability service. Sources that push over HTTP are
    /// usually declared in `FO_SOURCES_FILE` instead. See [`source`].
    pub fn with_source(mut self, source: impl ObservationSource) -> Self {
        self.sources.push(Arc::new(source));
        self
    }

    /// Parses the command line and environment, then runs the observer and its
    /// API server until it fails
    pub async fn run(self) -> anyhow::Result<()> {
        run(ModuleRegistry::new(self.modules)?, self.sources).await
    }
}

async fn run(
    modules: ModuleRegistry,
    mut sources: Vec<Arc<dyn ObservationSource>>,
) -> anyhow::Result<()> {
    // Dependencies enable both Rustls providers. Select one before any
    // background HTTPS/WebSocket task can initialize a TLS client.
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let args = Args::parse();
    args.runtime.validate()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive("info".parse().unwrap())
                .from_env()
                .unwrap(),
        )
        .init();

    if let Some(path) = &args.sources_file {
        sources.extend(
            source::load_push_sources(path, |name| std::env::var(name).ok())?
                .into_iter()
                .map(|source| Arc::new(source) as Arc<dyn ObservationSource>),
        );
    }
    let sources = SourceRegistry::new(sources)?;

    info!(
        "Observing modules {:?}",
        modules.kinds().collect::<Vec<_>>()
    );
    info!("Data sources {:?}", sources.ids().collect::<Vec<_>>());
    if args.admin_auth.len() < 16 {
        warn!("FO_ADMIN_AUTH is shorter than 16 characters; use a long random secret");
    }
    info!("Starting API server on {}", args.bind);

    let federation_observer = FederationObserver::new(
        &args.database,
        &args.admin_auth,
        &args.mempool_url,
        modules.clone(),
        sources,
        &args.runtime,
    )
    .await?;
    let services = Services::new(federation_observer.clone());
    let shutdown_observer = federation_observer.clone();
    let http_metrics = Arc::new(http::HttpMetrics::new(args.runtime.http_concurrency));

    let app = api::router(
        &modules,
        services,
        http_metrics.clone(),
        AppState {
            federation_config_cache: federation_observer.federation_config_cache().clone(),
            meta_override_cache: Default::default(),
            consensus_meta_cache: Default::default(),
            federation_observer,
            http_metrics,
        },
    );

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .context("Binding to port")?;

    axum::serve(listener, app)
        .with_graceful_shutdown(http::shutdown_signal(shutdown_observer))
        .await
        .context("Starting axum server")?;

    // A shutdown caused by losing the worker lease, not by a signal, is a
    // failure: exiting with an error lets a process manager restart us
    anyhow::ensure!(
        !runtime::worker_lease_lost(),
        "Lost the worker lease and stopped the collectors"
    );
    Ok(())
}
