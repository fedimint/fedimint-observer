use deadpool_postgres::GenericClient;
use fedimint_core::config::{ClientConfig, ClientModuleConfig, JsonClientConfig, JsonWithKind};
use fedimint_core::encoding::DynRawFallback;
use hex::ToHex;
use postgres_from_row::FromRow;
use serde_json::json;

use crate::config::meta::MetaFields;
use crate::module::ModuleRegistry;

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

pub async fn execute(
    conn: &impl GenericClient,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> anyhow::Result<u64> {
    let num_rows = conn.execute(sql, params).await?;
    Ok(num_rows)
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
