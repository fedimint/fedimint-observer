use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use fedimint_core::config::{FederationId, JsonClientConfig};
use fedimint_core::invite_code::InviteCode;
use tracing::warn;

use crate::config::id::fetch_federation_id;
use crate::config::meta::fetch_federation_meta;
use crate::config::modules::fetch_federation_module_kinds;
use crate::error::Result;
use crate::federation::observer::FederationObserver;
use crate::util::{config_to_json, make_room};
use crate::AppState;

/// Helper API that exposes the federation id
pub mod id;
/// Helper API that unifies config meta and override meta, applying lenient
/// parsing
pub mod meta;

/// Helper API that exposes the federation modules
pub mod modules;
/// CORS for these routes comes from the permissive layer on the whole app.
/// `/:invite/gateways` is the Lightning module's.
pub fn get_config_routes() -> Router<AppState> {
    Router::new()
        .route("/:invite", get(fetch_federation_config))
        .route("/:invite/meta", get(fetch_federation_meta))
        .route("/:invite/id", get(fetch_federation_id))
        .route("/:invite/module_kinds", get(fetch_federation_module_kinds))
}

pub async fn fetch_federation_config(
    Path(invite): Path<InviteCode>,
    State(state): State<AppState>,
) -> Result<Json<JsonClientConfig>> {
    Ok(state
        .federation_config_cache
        .fetch_config_cached(&invite, &state.federation_observer)
        .await?
        .into())
}

#[derive(Default, Debug, Clone)]
pub struct FederationConfigCache {
    federations: Arc<tokio::sync::RwLock<HashMap<FederationId, JsonClientConfig>>>,
}

impl FederationConfigCache {
    pub async fn fetch_config_cached(
        &self,
        invite: &InviteCode,
        observer: &FederationObserver,
    ) -> anyhow::Result<JsonClientConfig> {
        let federation_id = invite.federation_id();

        if let Some(config) = self.federations.read().await.get(&federation_id).cloned() {
            return Ok(config);
        }

        let config = fetch_config_inner(invite, observer).await?;
        let mut cache = self.federations.write().await;
        if !cache.contains_key(&federation_id) {
            make_room(&mut cache, MAX_CACHED_CONFIGS);
        }
        if let Some(replaced) = cache.insert(federation_id, config.clone()) {
            if replaced != config {
                warn!("Config for federation {federation_id} changed");
            }
        }

        Ok(config)
    }
}

/// Federations anyone can ask about through an invite code
const MAX_CACHED_CONFIGS: usize = 256;
/// Invite codes name the servers to contact, so they can name unreachable
/// ones, which the client keeps retrying
const INVITE_FETCH_TIMEOUT: Duration = Duration::from_secs(20);

async fn fetch_config_inner(
    invite: &InviteCode,
    observer: &FederationObserver,
) -> anyhow::Result<JsonClientConfig> {
    // Reuse the observer's registry: a registry built per request keeps its
    // guardian connections open after the request ends
    let (raw_config, _api) = tokio::time::timeout(
        INVITE_FETCH_TIMEOUT,
        fedimint_api_client::download_from_invite_code(observer.connectors(), invite),
    )
    .await
    .context("Timed out downloading the federation config")??;
    config_to_json(raw_config, observer.modules())
}
