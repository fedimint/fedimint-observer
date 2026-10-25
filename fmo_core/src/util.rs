use deadpool_postgres::GenericClient;
use fedimint_api_client::api::DynGlobalApi;
use fedimint_connectors::ConnectorRegistry;
use fedimint_core::config::{
    ClientConfig, ClientModuleConfig, GlobalClientConfig, JsonClientConfig, JsonWithKind,
};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::encoding::DynRawFallback;
use hex::ToHex;
use postgres_from_row::FromRow;
use serde_json::json;

use crate::config::meta::MetaFields;
use crate::module::ModuleRegistry;

/// Caches keyed by what callers send (invite codes, meta URLs) must stay
/// bounded: makes room for one more entry by evicting an arbitrary one
pub fn make_room<K, V>(map: &mut std::collections::HashMap<K, V>, max_entries: usize)
where
    K: Clone + Eq + std::hash::Hash,
{
    if map.len() >= max_entries {
        if let Some(key) = map.keys().next().cloned() {
            map.remove(&key);
        }
    }
}

/// A client for the API of a federation's guardians
pub fn federation_api(
    connectors: &ConnectorRegistry,
    global: &GlobalClientConfig,
) -> anyhow::Result<DynGlobalApi> {
    let peers = global
        .api_endpoints
        .iter()
        .map(|(&peer_id, peer_url)| (peer_id, peer_url.url.clone()))
        .collect();
    DynGlobalApi::new(connectors.clone(), peers, None)
}

/// The instance id of the federation's first module of `kind`
pub fn module_instance_id(config: &ClientConfig, kind: &str) -> Option<ModuleInstanceId> {
    config
        .modules
        .iter()
        .find_map(|(&instance_id, module)| (module.kind.as_str() == kind).then_some(instance_id))
}

pub fn config_to_json(
    cfg: ClientConfig,
    modules: &ModuleRegistry,
) -> anyhow::Result<JsonClientConfig> {
    let decoders = modules.decoders_for_config(&cfg);
    let config = cfg.redecode_raw(&decoders)?;

    Ok(JsonClientConfig {
        global: config.global,
        modules: config
            .modules
            .into_iter()
            .map(
                |(
                    instance_id,
                    ClientModuleConfig {
                        kind,
                        config: module_config,
                        ..
                    },
                )| {
                    (
                        instance_id,
                        JsonWithKind::new(
                            kind.clone(),
                            match module_config {
                                DynRawFallback::Raw { raw, .. } => {
                                    let raw: String = ToHex::encode_hex(&raw);
                                    json!({"raw": raw})
                                }
                                DynRawFallback::Decoded(decoded) => decoded.to_json().into(),
                            },
                        ),
                    )
                },
            )
            .collect(),
    })
}

pub async fn query_one<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<T>
where
    T: FromRow,
{
    let result = conn.query_one(sql, params).await?;
    Ok(T::try_from_row(&result)?)
}

pub async fn query_value<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<T>
where
    for<'a> T: tokio_postgres::types::FromSql<'a>,
{
    let result = conn.query_one(sql, params).await?;
    Ok(result.try_get(0)?)
}

pub async fn query_opt<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<Option<T>>
where
    T: FromRow,
{
    let result = conn.query_opt(sql, params).await?;
    Ok(result.map(|row| T::try_from_row(&row)).transpose()?)
}

pub async fn query<T>(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<Vec<T>>
where
    T: FromRow,
{
    let result = conn.query(sql, params).await?;
    Ok(result
        .iter()
        .map(T::try_from_row)
        .collect::<Result<_, _>>()?)
}

/// Merges meta fields from different sources
///
/// * `metas` - Meta fields from different sources highest to lowest priority
pub fn merge_metas(metas: &[MetaFields]) -> MetaFields {
    let key_set = metas
        .iter()
        .flat_map(|meta| meta.keys().cloned())
        .collect::<std::collections::BTreeSet<_>>();

    key_set
        .into_iter()
        .map(|key| {
            let value = metas
                .iter()
                .find_map(|meta_fields| meta_fields.get(&key).cloned())
                .expect("Key must exist in one meta source");
            (key, value)
        })
        .collect::<MetaFields>()
}
