use std::str::FromStr;

use axum::Json;
use fedimint_core::config::{FederationId, JsonClientConfig};
use tracing::debug;
use tracing::log::warn;

use crate::config::meta::{parse_meta_lenient, MetaFields};
use crate::util::merge_metas;
use crate::AppState;

pub async fn federation_meta(
    cfg: &JsonClientConfig,
    state: &AppState,
) -> crate::error::Result<Json<MetaFields>> {
    let maybe_consensus_meta = state.consensus_meta_cache.fetch_meta_cached(cfg).await;

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
        let meta_override = match state
            .meta_override_cache
            .fetch_meta_cached(&override_url, cfg.global.calculate_federation_id())
            .await
        {
            Ok(meta) => meta,
            Err(e) => {
                warn!("Failed to fetch meta fields from {override_url}: {e:?}");
                return Ok(meta_fields_config.into());
            }
        };
        Some(meta_override)
    } else {
        None
    };

    let mut meta_fields = merge_metas(&[
        maybe_consensus_meta.unwrap_or_default(),
        maybe_meta_override.unwrap_or_default(),
        meta_fields_config,
    ]);

    if cfg.global.calculate_federation_id()
        == FederationId::from_str(
            "1bcb64e68ef0b3de3ad96cb98b43a2fd972a9ffa0fb6f0e26aaee69d1d463b97",
        )
        .expect("valid")
    {
        meta_fields.insert(
            "federation_name".into(),
            serde_json::Value::String("Global Bitcoin Federation".into()),
        );
    }

    Ok(Json(meta_fields))
}
