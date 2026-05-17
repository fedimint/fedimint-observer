use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use fedimint_core::config::{FederationId, JsonClientConfig};
use fedimint_core::invite_code::InviteCode;
use fmo_api_types::GatewayInfo;
use reqwest::Method;
use tower_http::cors::{Any, CorsLayer};
use tracing::warn;

use crate::config::id::fetch_federation_id;
use crate::config::meta::fetch_federation_meta;
use crate::config::modules::fetch_federation_module_kinds;
use crate::error::Result;
use crate::federation::gateways::fetch_gateways_for_config;
use crate::util::config_to_json;
use crate::AppState;

/// Helper API that exposes the federation id
pub mod id;
/// Helper API that unifies config meta and override meta, applying lenient
/// parsing
pub mod meta;

/// Helper API that exposes the federation modules
pub mod modules;
pub fn get_config_routes() -> Router<AppState> {
    let router = Router::new()
        .route("/:invite", get(fetch_federation_config))
        .route("/:invite/gateways", get(fetch_federation_gateways))
        .route("/:invite/meta", get(fetch_federation_meta))
        .route("/:invite/id", get(fetch_federation_id))
        .route("/:invite/module_kinds", get(fetch_federation_module_kinds));

    let cors_enabled = dotenv::var("ALLOW_CONFIG_CORS").is_ok_and(|v| v == "true");

    if cors_enabled {
        router.layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET]),
        )
    } else {
        router
    }
}

pub async fn fetch_federation_config(
    Path(invite): Path<InviteCode>,
    State(state): State<AppState>,
) -> Result<Json<JsonClientConfig>> {
    Ok(state
        .federation_config_cache
        .fetch_config_cached(&invite)
        .await?
        .into())
}

pub async fn fetch_federation_gateways(
    Path(invite): Path<InviteCode>,
) -> Result<Json<Vec<GatewayInfo>>> {
    let config = fedimint_api_client::api::net::Connector::default()
        .download_from_invite_code(&invite)
        .await?;
    let gateways = fetch_gateways_for_config(&config).await?;
    Ok(gateways.into())
}

#[derive(Default, Debug, Clone)]
pub struct FederationConfigCache {
    federations: Arc<tokio::sync::RwLock<HashMap<FederationId, JsonClientConfig>>>,
}

impl FederationConfigCache {
    pub async fn fetch_config_cached(
        &self,
        invite: &InviteCode,
    ) -> anyhow::Result<JsonClientConfig> {
        let federation_id = invite.federation_id();

        if let Some(config) = self.federations.read().await.get(&federation_id).cloned() {
            return Ok(config);
        }

        let config = fetch_config_inner(invite).await?;
        let mut cache = self.federations.write().await;
        if let Some(replaced) = cache.insert(federation_id, config.clone()) {
            if replaced != config {
                // TODO: use tracing
                warn!("Config for federation {federation_id} changed");
            }
        }

        Ok(config)
    }
}

async fn fetch_config_inner(invite: &InviteCode) -> anyhow::Result<JsonClientConfig> {
    let raw_config = fedimint_api_client::api::net::Connector::default()
        .download_from_invite_code(invite)
        .await?;
    config_to_json(raw_config)
}
