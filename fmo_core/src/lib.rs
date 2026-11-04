//! Module-agnostic core of Fedimint Observer.
//!
//! An observer build picks its modules and runs:
//!
//! ```ignore
//! FedimintObserver::builder()
//!     .with_module(MintObserver)
//!     .with_module(WalletObserver::default())
//!     .with_module(LnObserver::default())
//!     .run()
//!     .await
//! ```

use std::sync::Arc;

use anyhow::Context;
use axum::routing::{get, put};
use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::config::meta::{ConsensusMetaCache, MetaOverrideCache};
use crate::config::{get_config_routes, FederationConfigCache};
use crate::federation::get_federations_routes;
use crate::federation::nostr::{get_nostr_federations, publish_federation_event};
use crate::federation::observer::FederationObserver;
use crate::module::{ModuleRegistry, ObserverModule, Services};

/// Fedimint config fetching service implementation
mod config;
/// HTTP error responses with sanitized internal errors.
mod error;
mod federation;
mod http;
mod meta;
pub mod module;
mod observations;
mod pagination;
mod runtime;
mod task;
mod util;

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
}

impl FedimintObserverBuilder {
    /// Decode and process this module's items. A module added to a build
    /// later fills its own tables from the stored sessions, and completes the
    /// core rows of its items, which were stored without amounts or details.
    pub fn with_module(mut self, module: impl ObserverModule) -> Self {
        self.modules.push(Arc::new(module));
        self
    }

    /// Parses the command line and environment, then runs the observer and its
    /// API server until it fails
    pub async fn run(self) -> anyhow::Result<()> {
        run(ModuleRegistry::new(self.modules)?).await
    }
}

async fn run(modules: ModuleRegistry) -> anyhow::Result<()> {
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

    info!(
        "Observing modules {:?}",
        modules.kinds().collect::<Vec<_>>()
    );
    if args.admin_auth.len() < 16 {
        warn!("FO_ADMIN_AUTH is shorter than 16 characters; use a long random secret");
    }
    info!("Starting API server on {}", args.bind);

    let federation_observer = FederationObserver::new(
        &args.database,
        &args.admin_auth,
        &args.mempool_url,
        modules.clone(),
        &args.runtime,
    )
    .await?;
    let services = Services::new(federation_observer.clone());
    let shutdown_observer = federation_observer.clone();
    let http_metrics = Arc::new(http::HttpMetrics::new(args.runtime.http_concurrency));

    let mut app = Router::new()
        .route("/health", get(|| async { "Server is up and running!" }))
        .route("/live", get(|| async { "alive" }))
        .route("/ready", get(http::ready))
        .route("/metrics", get(http::metrics))
        .nest("/config", get_config_routes())
        .nest("/federations", get_federations_routes())
        // TODO: move into nostr service/module
        .route("/nostr/federations", get(get_nostr_federations))
        .route("/nostr/federations", put(publish_federation_event));
    // The modules' endpoints, at the same paths core served them from
    if let Some(routes) = modules.federation_routes() {
        app = app.nest(
            "/federations/:federation_id",
            routes.with_state(services.clone()),
        );
    }
    if let Some(routes) = modules.config_routes() {
        app = app.nest("/config/:invite", routes.with_state(services));
    }
    let app = app
        .layer(axum::middleware::from_fn_with_state(
            http_metrics.clone(),
            http::admission,
        ))
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(AppState {
            federation_config_cache: federation_observer.federation_config_cache().clone(),
            meta_override_cache: Default::default(),
            consensus_meta_cache: Default::default(),
            federation_observer,
            http_metrics,
        });

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .context("Binding to port")?;

    axum::serve(listener, app)
        .with_graceful_shutdown(http::shutdown_signal(shutdown_observer))
        .await
        .context("Starting axum server")?;

    Ok(())
}
