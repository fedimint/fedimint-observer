use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use fedimint_core::config::FederationId;
use fedimint_core::core::{DynUnknown, ModuleInstanceId};
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::TransactionId;
use fmo_api_types::{TransactionHistogram, TransactionHistogramDay};
use postgres_from_row::FromRow;
use serde::{Deserialize, Serialize};

use crate::error::ClientError;
use crate::federation::observer::FederationObserver;
use crate::pagination::Page;
use crate::util::{query, query_value};
use crate::AppState;

pub(super) async fn list_transactions(
    Path(federation_id): Path<FederationId>,
    Query(page): Query<Page<TransactionId>>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<Vec<TransactionId>>> {
    Ok(state
        .federation_observer
        .federation_transaction_ids(federation_id, page.after, page.limit()?)
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

/// `?average=0|7|30&outliers=include|exclude`
#[derive(Debug, Default, Deserialize)]
pub(super) struct HistogramParams {
    average: Option<String>,
    outliers: Option<String>,
}

/// What a histogram request asked for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistogramOptions {
    /// Days the moving averages span; 0 for none
    average_days: usize,
    /// Flag outlier days and leave them out of averages and totals
    exclude_outliers: bool,
}

impl HistogramParams {
    fn options(&self) -> Result<HistogramOptions, ClientError> {
        let average_days = match self.average.as_deref() {
            None | Some("0") => 0,
            Some("7") => 7,
            Some("30") => 30,
            Some(_) => return Err(ClientError::bad_request("average must be 0, 7 or 30")),
        };
        let exclude_outliers = match self.outliers.as_deref() {
            None | Some("include") => false,
            Some("exclude") => true,
            Some(_) => {
                return Err(ClientError::bad_request(
                    "outliers must be include or exclude",
                ))
            }
        };
        Ok(HistogramOptions {
            average_days,
            exclude_outliers,
        })
    }
}

pub(super) async fn transaction_histogram(
    Path(federation_id): Path<FederationId>,
    Query(params): Query<HistogramParams>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<TransactionHistogram>> {
    let options = params.options()?;
    Ok(state
        .federation_observer
        .transaction_histogram(federation_id, options)
        .await?
        .into())
}

impl FederationObserver {
    pub async fn federation_transaction_ids(
        &self,
        federation_id: FederationId,
        after: Option<TransactionId>,
        limit: i64,
    ) -> anyhow::Result<Vec<TransactionId>> {
        self.ensure_observed(federation_id).await?;

        // Only the ids: decoding every transaction blob of a large federation
        // (hundreds of MB) just to drop it was most of this endpoint's cost
        self.connection()
            .await?
            .query(
                "SELECT txid FROM transactions WHERE federation_id = $1 AND txid > $2
                 ORDER BY txid LIMIT $3",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &after
                        .map(|id| id.consensus_encode_to_vec())
                        .unwrap_or_default(),
                    &limit,
                ],
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
            "SELECT COALESCE((SELECT tx_count FROM federation_totals WHERE federation_id = $1), 0)",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await? as u64)
    }

    pub async fn transaction_details(
        &self,
        federation_id: FederationId,
        transaction_id: TransactionId,
    ) -> anyhow::Result<DebugTransaction> {
        let cfg = self.observed_federation(federation_id).await?.config;

        let data = self
            .connection()
            .await?
            .query_opt(
                "SELECT data FROM transactions WHERE federation_id = $1 AND txid = $2",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &transaction_id.consensus_encode_to_vec(),
                ],
            )
            .await?
            .ok_or_else(|| {
                ClientError::not_found(format!("Transaction {transaction_id} not found"))
            })?
            .get::<_, Vec<u8>>(0);
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

    /// Funded transactions and volume per UTC day, with the averages and
    /// outliers asked for. The stored days with activity are few (one row
    /// per day), so filling in the others and averaging happens here.
    pub async fn transaction_histogram(
        &self,
        federation_id: FederationId,
        options: HistogramOptions,
    ) -> anyhow::Result<TransactionHistogram> {
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

        let active_days = query::<HistogramEntry>(
            &self.connection().await?,
            QUERY,
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        Ok(build_histogram(
            &active_days,
            chrono::Utc::now().date_naive(),
            options,
        ))
    }
}

/// Every day from the first in `active_days` through `today` (or the last
/// active day, if later), with averages and outliers as asked for.
///
/// Ports what the frontend did: a day is an outlier if its volume is at least
/// 10 times the 95th percentile of the days with activity (the element at
/// `floor(n * 0.95)` of their sorted volumes); days without activity never are.
/// A moving average over `n` days averages the day and the `n - 1` before it,
/// over fewer days at the start of the range, and leaves excluded outliers out
/// of both the sum and the number of days.
fn build_histogram(
    active_days: &[HistogramEntry],
    today: NaiveDate,
    options: HistogramOptions,
) -> TransactionHistogram {
    let active = active_days
        .iter()
        .map(|entry| {
            (
                entry.date,
                (
                    u64::try_from(entry.count).unwrap_or(0),
                    u64::try_from(entry.amount).unwrap_or(0),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let (Some((&first, _)), Some((&last, _))) = (active.first_key_value(), active.last_key_value())
    else {
        return TransactionHistogram {
            days: vec![],
            total_count: 0,
            total_volume_msat: 0,
        };
    };

    let outlier_threshold = options.exclude_outliers.then(|| {
        let mut volumes = active
            .values()
            .map(|(_, volume)| *volume)
            .collect::<Vec<_>>();
        volumes.sort_unstable();
        let index = ((volumes.len() as f64) * 0.95).floor() as usize;
        u128::from(volumes[index.min(volumes.len() - 1)]) * 10
    });

    let mut days = first
        .iter_days()
        .take_while(|day| *day <= today.max(last))
        .map(|date| {
            let (count, volume_msat) = active.get(&date).copied().unwrap_or_default();
            TransactionHistogramDay {
                date,
                count,
                volume_msat,
                avg_count: None,
                avg_volume_msat: None,
                outlier: active.contains_key(&date)
                    && outlier_threshold
                        .is_some_and(|threshold| u128::from(volume_msat) >= threshold),
            }
        })
        .collect::<Vec<_>>();

    let window = options.average_days;
    if window > 0 {
        let (mut count_sum, mut volume_sum, mut included) = (0u128, 0u128, 0usize);
        for index in 0..days.len() {
            let day = &days[index];
            if !day.outlier {
                count_sum += u128::from(day.count);
                volume_sum += u128::from(day.volume_msat);
                included += 1;
            }
            if index >= window {
                let left = &days[index - window];
                if !left.outlier {
                    count_sum -= u128::from(left.count);
                    volume_sum -= u128::from(left.volume_msat);
                    included -= 1;
                }
            }
            if included > 0 {
                days[index].avg_count = Some(count_sum as f64 / included as f64);
                days[index].avg_volume_msat = Some(volume_sum as f64 / included as f64);
            }
        }
    }

    let included = || days.iter().filter(|day| !day.outlier);
    let total_count = included().map(|day| day.count).sum();
    let total_volume_msat = included().fold(0u64, |sum, day| sum.saturating_add(day.volume_msat));
    TransactionHistogram {
        days,
        total_count,
        total_volume_msat,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugTransaction {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
struct HistogramEntry {
    date: NaiveDate,
    count: i64,
    amount: i64,
}

#[cfg(test)]
mod tests;
