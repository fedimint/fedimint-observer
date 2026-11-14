//! Presentation: the HTTP API. Assembles the routes of every layer and the
//! middleware they share. Handlers live next to the read queries they serve
//! (`federation/`, `config/`, `recommend`, and each module crate's
//! `federation_routes()`/`config_routes()`).
//!
//! - `/config/*`: stable API for inspecting federations by invite code
//! - `/federations/*`: unstable API for observed federations, including
//!   modules' endpoints under `/federations/:federation_id`
//! - `/nostr/*`, `/federations/nostr/*`: recommendations (`crate::recommend`)
//! - `/health`, `/live`, `/ready`, `/metrics`: probes, outside admission
//!   control

use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use tower_http::cors::CorsLayer;

use crate::config::get_config_routes;
use crate::federation::get_federations_routes;
use crate::http::{self, HttpMetrics};
use crate::module::{ModuleRegistry, Services};
use crate::AppState;

pub(crate) fn router(
    modules: &ModuleRegistry,
    services: Services,
    http_metrics: Arc<HttpMetrics>,
    state: AppState,
) -> Router {
    let mut app = Router::new()
        .route("/health", get(|| async { "Server is up and running!" }))
        .route("/live", get(|| async { "alive" }))
        .route("/ready", get(http::ready))
        .route("/metrics", get(http::metrics))
        .nest("/config", get_config_routes())
        .nest("/federations", get_federations_routes())
        .merge(crate::recommend::routes());
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
    app.layer(axum::middleware::from_fn_with_state(
        http_metrics,
        http::admission,
    ))
    .layer(tower_http::compression::CompressionLayer::new())
    .layer(tower_http::trace::TraceLayer::new_for_http())
    .layer(CorsLayer::permissive())
    .with_state(state)
}
