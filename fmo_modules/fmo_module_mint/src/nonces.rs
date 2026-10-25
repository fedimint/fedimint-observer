//! The nonce endpoint: when e-cash notes were spent

use std::collections::HashMap;

use anyhow::ensure;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fmo_api_types::{NonceSpendInfo, NoncesRequest};
use fmo_core::module::{query, ApiResult, Services};
use postgres_from_row::FromRow;

/// Each nonce is matched against the federation's mint inputs
const MAX_NONCES_PER_REQUEST: usize = 1000;

/// `POST /federations/:federation_id/nonces/spend`
pub(crate) fn federation_routes() -> Router<Services> {
    Router::new().route("/nonces/spend", post(get_nonces_spend_info))
}

async fn get_nonces_spend_info(
    Path(federation_id): Path<FederationId>,
    State(services): State<Services>,
    Json(request): Json<NoncesRequest>,
) -> ApiResult<Json<HashMap<String, NonceSpendInfo>>> {
    Ok(nonces_spend_info(&services, federation_id, &request.nonces)
        .await?
        .into())
}

pub(crate) async fn nonces_spend_info(
    services: &Services,
    federation_id: FederationId,
    nonces: &[String],
) -> anyhow::Result<std::collections::HashMap<String, NonceSpendInfo>> {
    use chrono::{DateTime, Utc};

    if nonces.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    ensure!(
        nonces.len() <= MAX_NONCES_PER_REQUEST,
        "At most {MAX_NONCES_PER_REQUEST} nonces can be checked per request"
    );

    services.ensure_observed(federation_id).await?;

    #[derive(Debug, FromRow)]
    struct NonceSpendRow {
        nonce: String,
        session_index: i32,
        estimated_session_timestamp: Option<chrono::NaiveDateTime>,
    }

    // Extract nonce from JSONB: {"V0": {"note": {"nonce": "..."}}}
    // language=postgresql
    let sql = "
        SELECT
            tid.details->'V0'->'note'->>'nonce' AS nonce,
            t.session_index,
            st.estimated_session_timestamp
        FROM transaction_input_details tid
        JOIN transactions t ON tid.federation_id = t.federation_id AND tid.txid = t.txid
        LEFT JOIN session_times st ON t.federation_id = st.federation_id AND t.session_index = st.session_index
        WHERE tid.federation_id = $1
          AND tid.kind = 'mint'
          AND tid.details->'V0'->'note'->>'nonce' = ANY($2)
    ";

    let rows = query::<NonceSpendRow>(
        &services.connection().await?,
        sql,
        &[&federation_id.consensus_encode_to_vec(), &nonces],
    )
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.nonce,
                NonceSpendInfo {
                    session_index: row.session_index as u64,
                    estimated_timestamp: row
                        .estimated_session_timestamp
                        .map(|ts| DateTime::<Utc>::from_naive_utc_and_offset(ts, Utc)),
                },
            )
        })
        .collect())
}
