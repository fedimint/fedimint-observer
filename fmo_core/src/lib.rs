//! Module-agnostic core of Fedimint Observer.
//!
//! An observer build picks its modules and runs:
//!
//! ```ignore
//! FedimintObserver::builder()
//!     .with_module(MintObserver)
//!     .with_module(WalletObserver)
//!     .with_module(LnObserver)
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
use crate::module::{ModuleRegistry, ObserverModule};

/// Fedimint config fetching service implementation
mod config;
mod db;
/// `anyhow`-based error handling for axum
mod error;
mod federation;
mod meta;
pub mod module;
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
}

#[derive(Debug, Clone)]
struct AppState {
    federation_config_cache: FederationConfigCache,
    meta_override_cache: MetaOverrideCache,
    consensus_meta_cache: ConsensusMetaCache,
    federation_observer: FederationObserver,
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
    /// Decode and process this module's items. Items of modules a build
    /// doesn't include are stored without amounts or details, and adding the
    /// module later doesn't fill those in yet.
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

    let app = Router::new()
        .route("/health", get(|| async { "Server is up and running!" }))
        .nest("/config", get_config_routes())
        .nest("/federations", get_federations_routes())
        // TODO: move into nostr service/module
        .route("/nostr/federations", get(get_nostr_federations))
        .route("/nostr/federations", put(publish_federation_event))
        .layer(CorsLayer::permissive())
        .with_state({
            let federation_observer = FederationObserver::new(
                &args.database,
                &args.admin_auth,
                &args.mempool_url,
                modules,
            )
            .await?;
            AppState {
                federation_config_cache: Default::default(),
                meta_override_cache: Default::default(),
                consensus_meta_cache: federation_observer.consensus_meta_cache().clone(),
                federation_observer,
            }
        });

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .context("Binding to port")?;

    axum::serve(listener, app)
        .await
        .context("Starting axum server")?;

    Ok(())
}
