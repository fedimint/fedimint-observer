//! Storage and reporting of Lightning probe results pushed by external probers
//! (see `fmo_prober`). Probes are keyed by the gateway's LN node public key.

use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_auth::AuthBearer;
use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fmo_api_types::{
    GatewayProbeReport, GatewayProbeResult, GatewayProbeSubmission, GatewayProbeSummary,
    GatewayProbeTrendPoint, ProbeOutcome,
};
use serde::Deserialize;
use tracing::info;

use crate::federation::gateways::GatewayMetricsWindow;
use crate::federation::observer::FederationObserver;
use crate::util::{query, query_opt, query_value};
use crate::AppState;

const MAX_PROBES_PER_SUBMISSION: usize = 1000;
const MAX_PROBER_ID_LEN: usize = 64;
const PROBE_RETENTION_DAYS: i64 = 90;
/// Gateways not seen in the registry for longer than this are not probed
const PROBE_TARGET_MAX_AGE_HOURS: i64 = 24;
const RECENT_PROBES_LIMIT: i64 = 50;
/// Tolerated clock skew between prober and observer
const MAX_FUTURE_SKEW_MINUTES: i64 = 5;

pub fn get_gateway_probe_routes() -> Router<AppState> {
    Router::new()
        .route("/probe-targets", get(get_probe_targets))
        .route("/probes", post(submit_probes))
}

fn inconclusive_outcomes() -> Vec<&'static str> {
    ProbeOutcome::ALL
        .into_iter()
        .filter(|outcome| !outcome.is_conclusive())
        .map(ProbeOutcome::as_str)
        .collect()
}

fn validate_node_pub_key(node_pub_key: &str) -> anyhow::Result<()> {
    let bytes = hex::decode(node_pub_key)
        .with_context(|| format!("Invalid node_pub_key '{node_pub_key}'"))?;
    ensure!(
        bytes.len() == 33 && matches!(bytes[0], 0x02 | 0x03),
        "Invalid node_pub_key '{node_pub_key}', expected 33 byte compressed public key"
    );
    Ok(())
}

fn validate_submission(
    submission: &GatewayProbeSubmission,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    ensure!(
        !submission.prober_id.is_empty() && submission.prober_id.len() <= MAX_PROBER_ID_LEN,
        "prober_id must be between 1 and {MAX_PROBER_ID_LEN} characters"
    );
    ensure!(
        submission.results.len() <= MAX_PROBES_PER_SUBMISSION,
        "Too many probe results, at most {MAX_PROBES_PER_SUBMISSION} per submission"
    );

    let max_probe_time = now + chrono::Duration::minutes(MAX_FUTURE_SKEW_MINUTES);
    for result in &submission.results {
        validate_node_pub_key(&result.node_pub_key)?;
        ensure!(result.amount_msat > 0, "Probe amount must be positive");
        ensure!(
            i64::try_from(result.amount_msat).is_ok(),
            "Probe amount out of range"
        );
        ensure!(
            result.probe_time <= max_probe_time,
            "Probe time {} lies in the future",
            result.probe_time
        );
    }
    Ok(())
}

fn to_i32(value: Option<u64>) -> Option<i32> {
    value.map(|v| i32::try_from(v).unwrap_or(i32::MAX))
}

fn to_i64(value: Option<u64>) -> Option<i64> {
    value.map(|v| i64::try_from(v).unwrap_or(i64::MAX))
}

fn success_rate_pct(successes: u64, conclusive: u64) -> Option<f64> {
    (conclusive > 0).then(|| successes as f64 / conclusive as f64 * 100.0)
}

impl FederationObserver {
    async fn store_gateway_probes(
        &self,
        submission: GatewayProbeSubmission,
    ) -> anyhow::Result<u64> {
        let now = Utc::now();
        validate_submission(&submission, now)?;

        let len = submission.results.len();
        let mut node_pub_keys = Vec::with_capacity(len);
        let mut probe_times = Vec::with_capacity(len);
        let mut amounts_msat = Vec::with_capacity(len);
        let mut outcomes = Vec::with_capacity(len);
        let mut latencies_ms = Vec::with_capacity(len);
        let mut failure_codes = Vec::with_capacity(len);
        let mut failure_source_indices = Vec::with_capacity(len);
        let mut route_hops = Vec::with_capacity(len);
        let mut route_fees_msat = Vec::with_capacity(len);

        for result in submission.results {
            node_pub_keys.push(result.node_pub_key.to_lowercase());
            probe_times.push(result.probe_time);
            amounts_msat.push(result.amount_msat as i64);
            outcomes.push(result.outcome.as_str());
            latencies_ms.push(to_i32(result.latency_ms));
            failure_codes.push(result.failure_code);
            failure_source_indices.push(to_i32(result.failure_source_index.map(u64::from)));
            route_hops.push(to_i32(result.route_hops.map(u64::from)));
            route_fees_msat.push(to_i64(result.route_fee_msat));
        }

        let mut conn = self.connection().await?;
        let dbtx = conn.transaction().await?;

        // ON CONFLICT DO NOTHING makes resubmitting a batch after a failed
        // request idempotent.
        let inserted = dbtx
            .execute(
                "INSERT INTO gateway_probes
                     (prober_id, node_pub_key, probe_time, amount_msat, outcome, latency_ms,
                      failure_code, failure_source_index, route_hops, route_fee_msat)
                 SELECT $1, p.*
                 FROM UNNEST(
                     $2::text[],
                     $3::timestamptz[],
                     $4::bigint[],
                     $5::text[],
                     $6::integer[],
                     $7::text[],
                     $8::integer[],
                     $9::integer[],
                     $10::bigint[]
                 ) AS p(node_pub_key, probe_time, amount_msat, outcome, latency_ms,
                        failure_code, failure_source_index, route_hops, route_fee_msat)
                 ON CONFLICT DO NOTHING",
                &[
                    &submission.prober_id,
                    &node_pub_keys,
                    &probe_times,
                    &amounts_msat,
                    &outcomes,
                    &latencies_ms,
                    &failure_codes,
                    &failure_source_indices,
                    &route_hops,
                    &route_fees_msat,
                ],
            )
            .await?;

        let retention_cutoff = now - chrono::Duration::days(PROBE_RETENTION_DAYS);
        let pruned = dbtx
            .execute(
                "DELETE FROM gateway_probes WHERE probe_time < $1",
                &[&retention_cutoff],
            )
            .await?;
        dbtx.commit().await?;

        info!(
            "Stored {inserted} probe result(s) from prober '{}', pruned {pruned} old probe(s)",
            submission.prober_id
        );
        Ok(inserted)
    }

    async fn list_probe_targets(&self) -> anyhow::Result<Vec<String>> {
        #[derive(postgres_from_row::FromRow)]
        struct TargetRow {
            node_pub_key: String,
        }

        let conn = self.connection().await?;
        let cutoff = Utc::now() - chrono::Duration::hours(PROBE_TARGET_MAX_AGE_HOURS);
        Ok(query::<TargetRow>(
            &conn,
            "SELECT DISTINCT node_pub_key
             FROM gateways
             WHERE last_seen >= $1
             ORDER BY node_pub_key",
            &[&cutoff],
        )
        .await?
        .into_iter()
        .map(|row| row.node_pub_key)
        .collect())
    }

    async fn gateway_probe_report(
        &self,
        federation_id: FederationId,
        gateway_id: &str,
        window: GatewayMetricsWindow,
    ) -> anyhow::Result<GatewayProbeReport> {
        #[derive(postgres_from_row::FromRow)]
        struct NodeRow {
            node_pub_key: String,
        }

        #[derive(postgres_from_row::FromRow)]
        struct OutcomeCountRow {
            outcome: String,
            is_base_amount: bool,
            count: i64,
        }

        #[derive(postgres_from_row::FromRow)]
        struct LatencyRow {
            p50: Option<f64>,
            p95: Option<f64>,
        }

        #[derive(postgres_from_row::FromRow)]
        struct TrendRow {
            bucket: DateTime<Utc>,
            conclusive_probes: i64,
            successes: i64,
            latency_p50_ms: Option<f64>,
        }

        #[derive(postgres_from_row::FromRow)]
        struct ProbeRow {
            node_pub_key: String,
            probe_time: DateTime<Utc>,
            amount_msat: i64,
            outcome: String,
            latency_ms: Option<i32>,
            failure_code: Option<String>,
            failure_source_index: Option<i32>,
            route_hops: Option<i32>,
            route_fee_msat: Option<i64>,
        }

        let conn = self.connection().await?;
        let federation_id_bytes = federation_id.consensus_encode_to_vec();

        let Some(NodeRow { node_pub_key }) = query_opt::<NodeRow>(
            &conn,
            "SELECT node_pub_key FROM gateways WHERE federation_id = $1 AND gateway_id = $2",
            &[&federation_id_bytes, &gateway_id],
        )
        .await?
        else {
            bail!("Gateway {gateway_id} not found in federation {federation_id}");
        };

        let window_start = Utc::now() - window.duration();
        let inconclusive = inconclusive_outcomes();

        // Probers send increasing amounts until one fails, so the larger
        // amounts failing is expected and only tells us about liquidity. Routing
        // success is therefore measured on the smallest (base) amount only.
        let base_amount_msat = query_value::<Option<i64>>(
            &conn,
            "SELECT MIN(amount_msat)
             FROM gateway_probes
             WHERE node_pub_key = $1
               AND probe_time >= $2",
            &[&node_pub_key, &window_start],
        )
        .await?
        .unwrap_or(0);

        let outcome_rows = query::<OutcomeCountRow>(
            &conn,
            "SELECT outcome, amount_msat = $3 AS is_base_amount, COUNT(*)::bigint AS count
             FROM gateway_probes
             WHERE node_pub_key = $1
               AND probe_time >= $2
             GROUP BY 1, 2",
            &[&node_pub_key, &window_start, &base_amount_msat],
        )
        .await?
        .into_iter()
        .map(|row| {
            let outcome = row
                .outcome
                .parse::<ProbeOutcome>()
                .map_err(anyhow::Error::msg)?;
            Ok((outcome, row.is_base_amount, row.count.max(0) as u64))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

        let mut outcome_counts = BTreeMap::new();
        let (mut conclusive_probes, mut successes, mut inconclusive_probes) = (0, 0, 0);
        for (outcome, is_base_amount, count) in outcome_rows {
            *outcome_counts.entry(outcome).or_default() += count;
            if !outcome.is_conclusive() {
                inconclusive_probes += count;
            } else if is_base_amount {
                conclusive_probes += count;
                if outcome == ProbeOutcome::Success {
                    successes += count;
                }
            }
        }

        // Latency of probes that reached the gateway, i.e. full round trips
        let latency = query_opt::<LatencyRow>(
            &conn,
            "SELECT
                 percentile_cont(0.5) WITHIN GROUP (ORDER BY latency_ms)::float8 AS p50,
                 percentile_cont(0.95) WITHIN GROUP (ORDER BY latency_ms)::float8 AS p95
             FROM gateway_probes
             WHERE node_pub_key = $1
               AND probe_time >= $2
               AND outcome = 'success'
               AND latency_ms IS NOT NULL",
            &[&node_pub_key, &window_start],
        )
        .await?;

        let max_successful_amount_msat = query_value::<Option<i64>>(
            &conn,
            "SELECT MAX(amount_msat)
             FROM gateway_probes
             WHERE node_pub_key = $1
               AND probe_time >= $2
               AND outcome = 'success'",
            &[&node_pub_key, &(Utc::now() - chrono::Duration::hours(24))],
        )
        .await?;

        let base_amount_last_outcome = query_value::<Option<String>>(
            &conn,
            "SELECT (
                 SELECT outcome
                 FROM gateway_probes
                 WHERE node_pub_key = $1
                   AND probe_time >= $2
                   AND amount_msat = $3
                   AND outcome <> ALL($4::text[])
                 ORDER BY probe_time DESC
                 LIMIT 1
             )",
            &[
                &node_pub_key,
                &window_start,
                &base_amount_msat,
                &inconclusive,
            ],
        )
        .await?
        .map(|outcome| outcome.parse::<ProbeOutcome>().map_err(anyhow::Error::msg))
        .transpose()?;

        let bucket_unit = match window {
            GatewayMetricsWindow::H1 | GatewayMetricsWindow::H24 => "hour",
            GatewayMetricsWindow::D7 | GatewayMetricsWindow::D30 | GatewayMetricsWindow::D90 => {
                "day"
            }
        };
        let trend = query::<TrendRow>(
            &conn,
            "SELECT
                 date_trunc($3::text, probe_time) AS bucket,
                 COUNT(*) FILTER (
                     WHERE amount_msat = $5 AND outcome <> ALL($4::text[])
                 )::bigint AS conclusive_probes,
                 COUNT(*) FILTER (
                     WHERE amount_msat = $5 AND outcome = 'success'
                 )::bigint AS successes,
                 (percentile_cont(0.5) WITHIN GROUP (ORDER BY latency_ms)
                     FILTER (WHERE outcome = 'success'))::float8 AS latency_p50_ms
             FROM gateway_probes
             WHERE node_pub_key = $1
               AND probe_time >= $2
             GROUP BY 1
             ORDER BY 1 ASC",
            &[
                &node_pub_key,
                &window_start,
                &bucket_unit,
                &inconclusive,
                &base_amount_msat,
            ],
        )
        .await?
        .into_iter()
        .map(|row| {
            let conclusive_probes = row.conclusive_probes.max(0) as u64;
            let successes = row.successes.max(0) as u64;
            GatewayProbeTrendPoint {
                bucket: row.bucket,
                conclusive_probes,
                successes,
                success_rate_pct: success_rate_pct(successes, conclusive_probes),
                latency_p50_ms: row.latency_p50_ms,
            }
        })
        .collect();

        let recent = query::<ProbeRow>(
            &conn,
            "SELECT node_pub_key, probe_time, amount_msat, outcome, latency_ms, failure_code,
                    failure_source_index, route_hops, route_fee_msat
             FROM gateway_probes
             WHERE node_pub_key = $1
             ORDER BY probe_time DESC, amount_msat DESC
             LIMIT $2",
            &[&node_pub_key, &RECENT_PROBES_LIMIT],
        )
        .await?
        .into_iter()
        .map(|row| {
            Ok(GatewayProbeResult {
                node_pub_key: row.node_pub_key,
                probe_time: row.probe_time,
                amount_msat: row.amount_msat.max(0) as u64,
                outcome: row.outcome.parse().map_err(anyhow::Error::msg)?,
                latency_ms: row.latency_ms.map(|v| v.max(0) as u64),
                failure_code: row.failure_code,
                failure_source_index: row.failure_source_index.map(|v| v.max(0) as u32),
                route_hops: row.route_hops.map(|v| v.max(0) as u32),
                route_fee_msat: row.route_fee_msat.map(|v| v.max(0) as u64),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

        let last_probe_time = recent
            .first()
            .map(|probe| probe.probe_time)
            .filter(|time| *time >= window_start);

        Ok(GatewayProbeReport {
            node_pub_key,
            summary: GatewayProbeSummary {
                window: window.label().to_owned(),
                conclusive_probes,
                successes,
                success_rate_pct: success_rate_pct(successes, conclusive_probes),
                inconclusive_probes,
                outcome_counts,
                last_probe_time,
                base_amount_msat: (base_amount_msat > 0).then_some(base_amount_msat as u64),
                base_amount_last_outcome,
                latency_p50_ms: latency.as_ref().and_then(|row| row.p50),
                latency_p95_ms: latency.and_then(|row| row.p95),
                max_successful_amount_msat: max_successful_amount_msat.map(|v| v.max(0) as u64),
            },
            trend,
            recent,
        })
    }
}

async fn get_probe_targets(
    State(state): State<AppState>,
) -> crate::error::Result<Json<Vec<String>>> {
    Ok(state.federation_observer.list_probe_targets().await?.into())
}

async fn submit_probes(
    AuthBearer(auth): AuthBearer,
    State(state): State<AppState>,
    Json(submission): Json<GatewayProbeSubmission>,
) -> crate::error::Result<Json<u64>> {
    // Probers get their own token so the admin token never has to live on the
    // probing host. Ingestion is disabled unless one is configured.
    let Some(prober_auth) = state.prober_auth.as_deref() else {
        return Err(anyhow::anyhow!("Probe ingestion is disabled (FO_PROBER_AUTH not set)").into());
    };
    ensure_auth(prober_auth, &auth)?;

    Ok(state
        .federation_observer
        .store_gateway_probes(submission)
        .await?
        .into())
}

fn ensure_auth(expected: &str, provided: &str) -> anyhow::Result<()> {
    ensure!(expected == provided, "Invalid bearer token");
    Ok(())
}

#[derive(Debug, Deserialize)]
pub(super) struct GatewayProbeReportParams {
    window: Option<String>,
}

pub(super) async fn get_gateway_probe_report(
    Path((federation_id, gateway_id)): Path<(FederationId, String)>,
    Query(params): Query<GatewayProbeReportParams>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<GatewayProbeReport>> {
    let window = GatewayMetricsWindow::parse(params.window.as_deref())?;
    Ok(state
        .federation_observer
        .gateway_probe_report(federation_id, &gateway_id, window)
        .await?
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(node_pub_key: &str, amount_msat: u64) -> GatewayProbeResult {
        GatewayProbeResult {
            node_pub_key: node_pub_key.to_owned(),
            probe_time: Utc::now(),
            amount_msat,
            outcome: ProbeOutcome::Success,
            latency_ms: Some(1200),
            failure_code: None,
            failure_source_index: None,
            route_hops: Some(3),
            route_fee_msat: Some(1000),
        }
    }

    const NODE: &str = "02eadbd9e7557375161df8b646776a547c5cbc2e95b3071ec81553f8ec2cea3b8c";

    #[test]
    fn accepts_valid_submission() {
        let submission = GatewayProbeSubmission {
            prober_id: "prober-1".to_owned(),
            results: vec![result(NODE, 10_000_000)],
        };
        validate_submission(&submission, Utc::now()).unwrap();
    }

    #[test]
    fn rejects_invalid_submissions() {
        let now = Utc::now();
        let check = |prober_id: &str, results: Vec<GatewayProbeResult>| {
            validate_submission(
                &GatewayProbeSubmission {
                    prober_id: prober_id.to_owned(),
                    results,
                },
                now,
            )
        };

        assert!(check("", vec![]).is_err());
        assert!(check(&"x".repeat(MAX_PROBER_ID_LEN + 1), vec![]).is_err());
        assert!(check("p", vec![result("not-hex", 1)]).is_err());
        assert!(check("p", vec![result(&NODE[2..], 1)]).is_err());
        assert!(check("p", vec![result(&format!("04{}", &NODE[2..]), 1)]).is_err());
        assert!(check("p", vec![result(NODE, 0)]).is_err());
        assert!(check("p", vec![result(NODE, u64::MAX)]).is_err());

        let mut future = result(NODE, 1);
        future.probe_time = now + chrono::Duration::hours(1);
        assert!(check("p", vec![future]).is_err());

        assert!(check("p", vec![result(NODE, 1); MAX_PROBES_PER_SUBMISSION + 1]).is_err());
    }

    #[test]
    fn inconclusive_outcomes_match_schema() {
        assert_eq!(
            inconclusive_outcomes(),
            vec!["local_failure", "timeout", "error"]
        );
        for outcome in ProbeOutcome::ALL {
            assert_eq!(outcome.as_str().parse::<ProbeOutcome>().unwrap(), outcome);
            assert_eq!(
                serde_json::to_value(outcome).unwrap(),
                serde_json::Value::String(outcome.as_str().to_owned())
            );
        }
    }
}
