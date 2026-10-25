use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Json;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use postgres_from_row::FromRow;
use serde_json::json;

use crate::federation::observer::FederationObserver;
use crate::util::{query, query_value};
use crate::AppState;

pub(super) async fn list_sessions(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<BTreeMap<i64, serde_json::Value>>> {
    Ok(state
        .federation_observer
        .federation_session_list(federation_id)
        .await?
        .into_iter()
        .map(|session| {
            (
                session.session_index,
                json!({ "transactions": session.transaction_count }),
            )
        })
        .collect::<BTreeMap<_, _>>()
        .into())
}

pub(super) async fn count_sessions(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<u64>> {
    Ok(state
        .federation_observer
        .federation_session_count(federation_id)
        .await?
        .into())
}

#[derive(FromRow)]
pub struct SessionData {
    pub session_index: i64,
    pub transaction_count: i64,
}

impl FederationObserver {
    pub async fn federation_session_list(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Vec<SessionData>> {
        self.ensure_observed(federation_id).await?;

        query::<SessionData>(&self.connection().await?, "
            SELECT s.session_index::bigint AS session_index, COUNT(t.txid) AS transaction_count
            FROM sessions AS s
            LEFT JOIN transactions AS t ON s.federation_id = t.federation_id AND s.session_index = t.session_index
            WHERE s.federation_id = $1
            GROUP BY s.session_index
            ORDER BY s.session_index ASC
        ", &[&federation_id.consensus_encode_to_vec()])
        .await
    }

    pub async fn federation_session_count(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<u64> {
        let session_count = query_value::<i64>(
            &self.connection().await?,
            "SELECT COUNT(*) FROM sessions WHERE federation_id = $1",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;
        Ok(session_count as u64)
    }

    /// The session to fetch next: one past the last stored session. Counting
    /// rows instead would refetch a session forever if one were ever missing.
    pub(crate) async fn next_session_index(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<u64> {
        let connection = self.connection().await?;
        let (next, count) = {
            let row = connection
                .query_one(
                    "SELECT COALESCE(MAX(session_index) + 1, 0)::bigint, COUNT(*)::bigint FROM sessions WHERE federation_id = $1",
                    &[&federation_id.consensus_encode_to_vec()],
                )
                .await?;
            (row.get::<_, i64>(0), row.get::<_, i64>(1))
        };
        if next != count {
            // Sync continues after the newest session, so these are not fetched again
            tracing::warn!(
                "Federation {federation_id} is missing {} of its first {next} sessions",
                next - count
            );
        }
        Ok(next as u64)
    }
}
