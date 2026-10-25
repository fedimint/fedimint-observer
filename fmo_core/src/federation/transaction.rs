use std::collections::BTreeMap;

use anyhow::Context;
use axum::extract::{Path, State};
use axum::Json;
use chrono::NaiveDate;
use fedimint_core::config::FederationId;
use fedimint_core::core::{DynUnknown, ModuleInstanceId};
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::{Amount, TransactionId};
use fmo_api_types::FederationActivity;
use postgres_from_row::FromRow;
use serde::Serialize;

use crate::federation::observer::FederationObserver;
use crate::util::{query, query_value};
use crate::AppState;

pub(super) async fn list_transactions(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<Vec<TransactionId>>> {
    Ok(state
        .federation_observer
        .federation_transaction_ids(federation_id)
        .await?
        .into())
}

pub(super) async fn count_transactions(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<u64>> {
    Ok(state
        .federation_observer
        .federation_transaction_count(federation_id)
        .await?
        .into())
}

pub(super) async fn transaction(
    Path((federation_id, transaction_id)): Path<(FederationId, TransactionId)>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<DebugTransaction>> {
    Ok(state
        .federation_observer
        .transaction_details(federation_id, transaction_id)
        .await?
        .into())
}

pub(super) async fn transaction_histogram(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<BTreeMap<NaiveDate, FederationActivity>>> {
    Ok(state
        .federation_observer
        .transaction_histogram(federation_id)
        .await?
        .into_iter()
        .map(|histogram_entry| {
            (
                histogram_entry.date,
                FederationActivity {
                    num_transactions: histogram_entry.count as u64,
                    amount_transferred: Amount::from_msats(histogram_entry.amount as u64),
                },
            )
        })
        .collect::<BTreeMap<_, _>>()
        .into())
}

impl FederationObserver {
    pub async fn federation_transaction_ids(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Vec<TransactionId>> {
        self.ensure_observed(federation_id).await?;

        // Only the ids: decoding every transaction blob of a large federation
        // (hundreds of MB) just to drop it was most of this endpoint's cost
        self.connection()
            .await?
            .query(
                "SELECT txid FROM transactions WHERE federation_id = $1",
                &[&federation_id.consensus_encode_to_vec()],
            )
            .await?
            .iter()
            .map(|row| {
                Ok(TransactionId::consensus_decode_whole(
                    &row.get::<_, Vec<u8>>("txid"),
                    &Default::default(),
                )?)
            })
            .collect()
    }

    pub async fn federation_transaction_count(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<u64> {
        self.ensure_observed(federation_id).await?;

        Ok(query_value::<i64>(
            &self.connection().await?,
            "SELECT COUNT(txid) FROM transactions WHERE federation_id = $1",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await? as u64)
    }

    pub async fn transaction_details(
        &self,
        federation_id: FederationId,
        transaction_id: TransactionId,
    ) -> anyhow::Result<DebugTransaction> {
        let cfg = self
            .get_federation(federation_id)
            .await?
            .context("Federation doesn't exist")?
            .config;

        let data = query_value::<Vec<u8>>(
            &self.connection().await?,
            "SELECT data FROM transactions WHERE federation_id = $1 AND txid = $2",
            &[
                &federation_id.consensus_encode_to_vec(),
                &transaction_id.consensus_encode_to_vec(),
            ],
        )
        .await
        .context("Transaction not found")?;
        // Items of modules this build doesn't include decode as `DynUnknown`
        let tx = fedimint_core::transaction::Transaction::consensus_decode_whole(
            &data,
            &self.modules().decoders_for_config(&cfg),
        )?;
        let describe = |item: &dyn std::any::Any, instance: ModuleInstanceId, debug: String| {
            if item.is::<DynUnknown>() {
                format!("Unknown module, instance id={instance}")
            } else {
                debug
            }
        };
        let inputs = tx
            .inputs
            .iter()
            .map(|input| {
                describe(
                    input.as_any(),
                    input.module_instance_id(),
                    format!("{input:?}"),
                )
            })
            .collect();
        let outputs = tx
            .outputs
            .iter()
            .map(|output| {
                describe(
                    output.as_any(),
                    output.module_instance_id(),
                    format!("{output:?}"),
                )
            })
            .collect();

        Ok(DebugTransaction { inputs, outputs })
    }

    pub async fn transaction_histogram(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Vec<HistogramEntry>> {
        // Days with only output-only transactions (Lightning offers and
        // such) are not shown, as before the daily activity table existed
        // language=postgresql
        const QUERY: &str = "
            SELECT day AS date, funded_tx_count AS count, volume_msat AS amount
            FROM federation_daily_activity
            WHERE federation_id = $1 AND funded_tx_count > 0
            ORDER BY day
        ";

        self.ensure_observed(federation_id).await?;

        let histogram = query::<HistogramEntry>(
            &self.connection().await?,
            QUERY,
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        Ok(histogram)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugTransaction {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct HistogramEntry {
    date: NaiveDate,
    count: i64,
    amount: i64,
}
