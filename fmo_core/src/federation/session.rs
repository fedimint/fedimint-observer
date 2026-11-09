use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use postgres_from_row::FromRow;
use serde_json::json;

use crate::federation::observer::FederationObserver;
use crate::pagination::Page;
use crate::util::{query, query_value};
use crate::AppState;

pub(super) async fn list_sessions(
    Path(federation_id): Path<FederationId>,
    Query(page): Query<Page<i32>>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<BTreeMap<i64, serde_json::Value>>> {
    Ok(state
        .federation_observer
        .federation_session_list(federation_id, page.after.unwrap_or(-1), page.limit()?)
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
    // Unknown ids are a 404 like on the other routes, not a count of 0
    state
        .federation_observer
        .ensure_observed(federation_id)
        .await?;
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
        after: i32,
        limit: i64,
    ) -> anyhow::Result<Vec<SessionData>> {
        self.ensure_observed(federation_id).await?;

        query::<SessionData>(&self.connection().await?, "
            SELECT s.session_index::bigint AS session_index, COUNT(t.txid) AS transaction_count
            FROM (SELECT session_index, federation_id FROM sessions
                  WHERE federation_id = $1 AND session_index > $2
                  ORDER BY session_index LIMIT $3) AS s
            LEFT JOIN transactions AS t ON s.federation_id = t.federation_id AND s.session_index = t.session_index
            WHERE s.federation_id = $1
            GROUP BY s.session_index
            ORDER BY s.session_index ASC
        ", &[&federation_id.consensus_encode_to_vec(), &after, &limit])
        .await
    }

    pub async fn federation_session_count(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<u64> {
        let session_count = query_value::<i64>(
            &self.connection().await?,
            "SELECT COALESCE((SELECT session_count FROM federation_session_counts WHERE federation_id = $1), 0)",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;
        Ok(session_count as u64)
    }

    /// The session to fetch next: one past the last stored session. Counting
    /// rows instead would refetch a session forever if one were ever missing.
    ///
    /// Also checks the federation's stored session count against it. The
    /// count is derived from `sessions`, so where they differ it is recounted
    /// first: a counter left short is corrected here, and only sessions really
    /// missing are reported.
    pub(crate) async fn next_session_index(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<u64> {
        let federation_bytes = federation_id.consensus_encode_to_vec();
        let mut connection = self.connection().await?;
        let (next, count) = {
            let row = connection
                .query_one(
                    "SELECT COALESCE((SELECT MAX(session_index)::bigint + 1 FROM sessions WHERE federation_id = $1), 0),
                            COALESCE((SELECT session_count FROM federation_session_counts WHERE federation_id = $1), 0)",
                    &[&federation_bytes],
                )
                .await?;
            (row.get::<_, i64>(0), row.get::<_, i64>(1))
        };
        if next == count {
            return Ok(next as u64);
        }

        // A session stored meanwhile updates the count's row, so with that row
        // locked the count read after it is exact
        let dbtx = connection.transaction().await?;
        dbtx.execute(
            "SELECT 1 FROM federation_session_counts WHERE federation_id = $1 FOR UPDATE",
            &[&federation_bytes],
        )
        .await?;
        let stored: i64 = dbtx
            .query_one(
                "SELECT COUNT(*) FROM sessions WHERE federation_id = $1",
                &[&federation_bytes],
            )
            .await?
            .get(0);
        if stored != count {
            dbtx.execute(
                "INSERT INTO federation_session_counts (federation_id, session_count) VALUES ($1, $2)
                 ON CONFLICT (federation_id) DO UPDATE SET session_count = EXCLUDED.session_count",
                &[&federation_bytes, &stored],
            )
            .await?;
            tracing::warn!(
                "Federation {federation_id} had a stored session count of {count} for {stored} stored sessions; corrected it"
            );
        }
        dbtx.commit().await?;
        if next != stored {
            // Sync continues after the newest session, so these are not fetched again
            tracing::warn!(
                "Federation {federation_id} is missing {} of its first {next} sessions",
                next - stored
            );
        }
        Ok(next as u64)
    }
}
