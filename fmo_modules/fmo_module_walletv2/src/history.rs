use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fmo_api_types::{WalletV2History, WalletV2OnChainTransaction};
use fmo_core::module::{ApiError, ApiResult, ClientError, Services};
use serde::Deserialize;

pub(crate) const HISTORY_PAGE_SQL: &str = "SELECT data FROM fmo_walletv2.chain WHERE federation_id=$1 AND module_instance_id=$2
         AND tx_index <= (SELECT history_end_index FROM fmo_walletv2.wallets WHERE federation_id=$1 AND module_instance_id=$2)
         AND ($3::bigint IS NULL OR tx_index < $3) ORDER BY tx_index DESC LIMIT $4";

#[derive(Default, Deserialize)]
pub(crate) struct Params {
    pub module_instance_id: Option<u16>,
    pub before: Option<i64>,
    pub limit: Option<u16>,
}

pub(crate) fn routes() -> Router<Services> {
    Router::new().route("/wallet-v2/history", get(history))
}

pub(crate) fn page_limit(params: &Params) -> ApiResult<i64> {
    let limit = params.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) || params.before.is_some_and(|before| before < 0) {
        return Err(ApiError::bad_request(
            "limit must be 1..100 and before must be nonnegative",
        ));
    }
    Ok(i64::from(limit))
}

/// The walletv2 instance a request is about: 404 if the federation has none,
/// 400 if the requested one isn't a walletv2 instance or the choice is
/// ambiguous
pub(crate) fn select_instance(ids: &[u16], requested: Option<u16>) -> ApiResult<u16> {
    match (requested, ids) {
        (_, []) => Err(ApiError::not_found("Federation has no Wallet v2 module")),
        (Some(id), _) if ids.contains(&id) => Ok(id),
        (Some(id), _) => Err(ApiError::bad_request(format!(
            "Module instance {id} is not a Wallet v2 module of this federation"
        ))),
        (None, [id]) => Ok(*id),
        (None, _) => Err(ApiError::bad_request(
            "Select a configured walletv2 module_instance_id",
        )),
    }
}

async fn history(
    Path(federation_id): Path<FederationId>,
    Query(params): Query<Params>,
    State(services): State<Services>,
) -> ApiResult<Json<WalletV2History>> {
    let limit = page_limit(&params)?;
    let federation = services.federation(federation_id).await?.ok_or_else(|| {
        ClientError::not_found(format!("Federation {federation_id} is not observed"))
    })?;
    let instance = select_instance(
        &super::collector::instances(&federation.config),
        params.module_instance_id,
    )?;
    let connection = services.connection().await?;
    let bytes = federation_id.consensus_encode_to_vec();
    let state = connection
        .query_opt(
            "SELECT balance_msat, balance_observed_at, history_observed_at
         FROM fmo_walletv2.wallets WHERE federation_id=$1 AND module_instance_id=$2",
            &[&bytes, &i32::from(instance)],
        )
        .await?;
    let balance: Option<i64> = state.as_ref().and_then(|row| row.get(0));
    let balance_observed_at: Option<DateTime<Utc>> = state.as_ref().and_then(|row| row.get(1));
    let observed_at: Option<DateTime<Utc>> = state.as_ref().and_then(|row| row.get(2));
    let rows = connection
        .query(
            HISTORY_PAGE_SQL,
            &[&bytes, &i32::from(instance), &params.before, &(limit + 1)],
        )
        .await?;
    let mut transactions = rows
        .iter()
        .map(|row| serde_json::from_value::<WalletV2OnChainTransaction>(row.get(0)))
        .collect::<Result<Vec<_>, _>>()?;
    let more = transactions.len() > limit as usize;
    transactions.truncate(limit as usize);
    let next_before = if more {
        transactions.last().map(|tx| tx.index)
    } else {
        None
    };
    let accounting_pending: bool = connection.query_one(
        "SELECT EXISTS(SELECT 1 FROM fmo_walletv2.deposit_jobs WHERE federation_id=$1 AND module_instance_id=$2)",
        &[&bytes,&i32::from(instance)],
    ).await?.get(0);
    Ok(Json(WalletV2History {
        module_instance_id: instance,
        accounting_pending,
        transactions,
        next_before,
        observed_at,
        stale: observed_at
            .is_none_or(|at| Utc::now().signed_duration_since(at).num_seconds() > 1800),
        balance_msat: balance.map(|value| value as u64),
        balance_observed_at,
        balance_stale: balance_observed_at
            .is_none_or(|at| Utc::now().signed_duration_since(at).num_seconds() > 300),
    }))
}
