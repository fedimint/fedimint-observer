use axum::Json;
use fedimint_core::config::JsonClientConfig;
use tracing::{debug, warn};

use crate::config::meta::{parse_meta_lenient, MetaFields};
use crate::util::merge_metas;
use crate::AppState;

pub async fn federation_meta(
    cfg: &JsonClientConfig,
    state: &AppState,
) -> crate::error::Result<Json<MetaFields>> {
    let maybe_consensus_meta = state
        .consensus_meta_cache
        .fetch_meta_cached(cfg, state.federation_observer.connectors())
        .await;

    let meta_fields_config = parse_meta_lenient(
        cfg.global
            .meta
            .iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned().into())),
    );

    let maybe_meta_override = if let Some(override_url) = meta_fields_config
        .get("meta_override_url")
        .or_else(|| meta_fields_config.get("meta_external_url")) // Fedi legacy field
        .and_then(|url| url.as_str().map(ToOwned::to_owned))
    {
        debug!("fetching {override_url}");
        // Without the override, the consensus and config meta still apply
        state
            .meta_override_cache
            .fetch_meta_cached(&override_url, cfg.global.calculate_federation_id())
            .await
            .inspect_err(|e| warn!("Failed to fetch meta fields from {override_url}: {e:?}"))
            .ok()
    } else {
        None
    };

    Ok(Json(merge_metas(&[
        maybe_consensus_meta.unwrap_or_default(),
        maybe_meta_override.unwrap_or_default(),
        meta_fields_config,
    ])))
}
