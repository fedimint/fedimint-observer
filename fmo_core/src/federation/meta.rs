use anyhow::Context;
use axum::extract::{Path, State};
use axum::Json;
use fedimint_core::config::FederationId;

use crate::config::meta::MetaFields;
use crate::meta::federation_meta;
use crate::util::config_to_json;

// FIXME: cache meta in DB
pub(super) async fn get_federation_meta(
    Path(federation_id): Path<FederationId>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<MetaFields>> {
    let config = state
        .federation_observer
        .get_federation(federation_id)
        .await?
        .context("Federation not observed, you might want to try /config/:federation_invite")?
        .config;

    federation_meta(
        &config_to_json(config, state.federation_observer.modules())?,
        &state,
    )
    .await
}
