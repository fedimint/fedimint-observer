use anyhow::Context;
use axum::routing::{get, put};
use axum::Router;
use clap::Parser;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::config::meta::{ConsensusMetaCache, MetaOverrideCache};
use crate::config::{get_config_routes, FederationConfigCache};
use crate::federation::get_federations_routes;
use crate::federation::nostr::{get_nostr_federations, publish_federation_event};
use crate::federation::observer::FederationObserver;
use crate::federation::probes::get_gateway_probe_routes;

/// Fedimint config fetching service implementation
mod config;
mod db;
/// `anyhow`-based error handling for axum
mod error;
mod federation;
mod meta;
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

    /// Bearer token for LN probers submitting gateway probe results. Probe
    /// ingestion is disabled if unset.
    #[arg(long, env = "FO_PROBER_AUTH")]
    prober_auth: Option<String>,
}

#[derive(Debug, Clone)]
struct AppState {
    federation_config_cache: FederationConfigCache,
    meta_override_cache: MetaOverrideCache,
    consensus_meta_cache: ConsensusMetaCache,
    federation_observer: FederationObserver,
    prober_auth: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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

    info!("Starting API server on {}", args.bind);

    let app = Router::new()
        .route("/health", get(|| async { "Server is up and running!" }))
        .nest("/config", get_config_routes())
        .nest("/federations", get_federations_routes())
        .nest("/gateways", get_gateway_probe_routes())
        // TODO: move into nostr service/module
        .route("/nostr/federations", get(get_nostr_federations))
        .route("/nostr/federations", put(publish_federation_event))
        .layer(CorsLayer::permissive())
        .with_state(AppState {
            federation_config_cache: Default::default(),
            meta_override_cache: Default::default(),
            consensus_meta_cache: Default::default(),
            federation_observer: FederationObserver::new(
                &args.database,
                &args.admin_auth,
                &args.mempool_url,
            )
            .await?,
            prober_auth: args.prober_auth.filter(|auth| !auth.is_empty()),
        });

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .context("Binding to port")?;

    axum::serve(listener, app)
        .await
        .context("Starting axum server")?;

    Ok(())
}
