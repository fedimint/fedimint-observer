use std::collections::BTreeSet;

use axum::extract::{Path, State};
use axum::Json;
use fedimint_core::core::ModuleKind;
use fedimint_core::invite_code::InviteCode;

use crate::AppState;

pub async fn fetch_federation_module_kinds(
    Path(invite): Path<InviteCode>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<BTreeSet<ModuleKind>>> {
    let config = state
        .federation_config_cache
        .fetch_config_cached(&invite, state.federation_observer.modules())
        .await?;
    let module_kinds = config
        .modules
        .into_values()
        .map(|module_config| module_config.kind().to_owned())
        .collect::<BTreeSet<_>>();

    Ok(module_kinds.into())
}
