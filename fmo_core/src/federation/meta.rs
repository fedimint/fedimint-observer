use axum::extract::{Path, State};
use axum::Json;
use fedimint_core::config::FederationId;

use crate::config::meta::MetaFields;
use crate::federation::observed_config_json;
use crate::meta::federation_meta;

// FIXME: cache meta in DB
pub(super) async fn get_federation_meta(
    Path(federation_id): Path<FederationId>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<MetaFields>> {
    let config = observed_config_json(&state.federation_observer, federation_id).await?;
    federation_meta(&config, &state).await
}
