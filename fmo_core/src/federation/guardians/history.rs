//! Guardian health over time and the latest polls, read from what
//! `monitor_health` has already stored: no request here talks to a guardian.

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, DurationRound, NaiveDateTime, TimeDelta, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fmo_api_types::{
    FederationLiveHealth, GuardianHealthBucket, GuardianHealthHistory, GuardianHealthSeries,
    GuardianHealthWindow, GuardianLiveStatus, GuardianPeerView,
};
use serde::Deserialize;
use serde_json::Value;

use crate::federation::observer::FederationObserver;

/// How far back `last_online_hour` looks, so a guardian that has been gone
/// for years costs a bounded index range, not the whole rollup
const LAST_ONLINE_LOOKBACK_DAYS: i64 = 90;

/// Enumerates the federation's guardian ids with one index descent each; see
/// `get_guardian_health` for why this replaces a GROUP BY over the samples
const GUARDIANS_CTE: &str = "WITH RECURSIVE guardians AS (
         SELECT (SELECT h.guardian_id
                 FROM guardian_health h
                 WHERE h.federation_id = $1
                 ORDER BY h.guardian_id
                 LIMIT 1) AS guardian_id
       UNION ALL
         SELECT (SELECT h.guardian_id
                 FROM guardian_health h
                 WHERE h.federation_id = $1
                   AND h.guardian_id > g.guardian_id
                 ORDER BY h.guardian_id
                 LIMIT 1)
         FROM guardians g
         WHERE g.guardian_id IS NOT NULL
     )";

/// Buckets in the window, the span of one and its `date_trunc` unit
fn window_buckets(window: GuardianHealthWindow) -> (u32, TimeDelta, &'static str) {
    match window {
        GuardianHealthWindow::Day => (24, TimeDelta::hours(1), "hour"),
        GuardianHealthWindow::Week => (7 * 24, TimeDelta::hours(1), "hour"),
        GuardianHealthWindow::Month => (30, TimeDelta::days(1), "day"),
    }
}

/// The first bucket's start, so the window ends with the current bucket
fn window_start(now: DateTime<Utc>, window: GuardianHealthWindow) -> DateTime<Utc> {
    let (count, bucket, _) = window_buckets(window);
    let current = now
        .duration_trunc(bucket)
        .expect("Hours and days divide the epoch evenly");
    current - bucket * (count as i32 - 1)
}

fn ratio(part: i64, whole: i64) -> Option<f32> {
    (whole > 0).then(|| part as f32 / whole as f32)
}

/// The mean latency over a window, from each bucket's `(latency_sum_ms,
/// latency_samples)`: every timed request weighs the same. Weighting the
/// buckets' means by their online samples instead biased it, since the
/// status poll and the separately timed request succeed independently.
fn window_latency_ms(buckets: impl IntoIterator<Item = (i64, i64)>) -> Option<f32> {
    let (sum, samples) = buckets
        .into_iter()
        .filter(|&(_, samples)| samples > 0)
        .fold(
            (0i64, 0i64),
            |(sum, samples), (bucket_sum, bucket_samples)| {
                (
                    sum.saturating_add(bucket_sum),
                    samples.saturating_add(bucket_samples),
                )
            },
        );
    ratio(sum, samples)
}

fn non_negative(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn clamp_u32(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// The parts of a stored `StatusResponse` the live view shows. Read
/// leniently: rows come from every Fedimint version the observer has polled,
/// so a missing or odd field leaves that field empty instead of failing.
struct ReportedStatus {
    session_count: Option<u64>,
    peers_online: Option<u64>,
    peers_offline: Option<u64>,
    peers_flagged: Option<u64>,
    scheduled_shutdown: Option<u64>,
    peers: Vec<GuardianPeerView>,
}

fn reported_status(status: &Value) -> ReportedStatus {
    let federation = status.get("federation");
    let number = |key: &str| federation.and_then(|f| f.get(key)).and_then(Value::as_u64);
    let mut peers: Vec<GuardianPeerView> = federation
        .and_then(|f| f.get("status_by_peer"))
        .and_then(Value::as_object)
        .map(|by_peer| {
            by_peer
                .iter()
                .filter_map(|(peer, view)| {
                    Some(GuardianPeerView {
                        peer_id: peer.parse().ok()?,
                        connected: view.get("connection_status")?.as_str()? == "connected",
                        flagged: view
                            .get("flagged")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        last_contribution: view.get("last_contribution").and_then(Value::as_u64),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    peers.sort_by_key(|peer| peer.peer_id);
    ReportedStatus {
        session_count: number("session_count"),
        peers_online: number("peers_online"),
        peers_offline: number("peers_offline"),
        peers_flagged: number("peers_flagged"),
        scheduled_shutdown: number("scheduled_shutdown"),
        peers,
    }
}

impl FederationObserver {
    pub async fn guardian_health_history(
        &self,
        federation_id: FederationId,
        window: GuardianHealthWindow,
    ) -> anyhow::Result<GuardianHealthHistory> {
        self.ensure_observed(federation_id).await?;
        let now = Utc::now();
        let start = window_start(now, window);
        let (bucket_count, bucket, unit) = window_buckets(window);
        let id = federation_id.consensus_encode_to_vec();
        let connection = self.connection().await?;

        // At most one row per guardian and bucket: 168 per guardian for 7d,
        // each from a range of the rollup's primary key
        let rows = connection
            .query(
                &format!(
                    "{GUARDIANS_CTE}
                     SELECT g.guardian_id,
                            b.bucket,
                            b.samples,
                            b.online_samples,
                            b.polled_s,
                            b.online_s,
                            b.latency_samples,
                            b.latency_sum_ms
                     FROM guardians g
                     CROSS JOIN LATERAL (
                         SELECT date_trunc($3, h.hour) AS bucket,
                                SUM(h.samples)::bigint AS samples,
                                SUM(h.online_samples)::bigint AS online_samples,
                                SUM(h.polled_s)::bigint AS polled_s,
                                SUM(h.online_s)::bigint AS online_s,
                                SUM(h.latency_samples)::bigint AS latency_samples,
                                SUM(h.latency_sum_ms)::bigint AS latency_sum_ms
                         FROM guardian_health_hourly h
                         WHERE h.federation_id = $1
                           AND h.guardian_id = g.guardian_id
                           AND h.hour >= $2
                         GROUP BY 1
                     ) b
                     WHERE g.guardian_id IS NOT NULL
                     ORDER BY g.guardian_id, b.bucket"
                ),
                &[&id, &start.naive_utc(), &unit],
            )
            .await?;

        let last_online = connection
            .query(
                &format!(
                    "{GUARDIANS_CTE}
                     SELECT g.guardian_id,
                            (SELECT MAX(h.hour)
                             FROM guardian_health_hourly h
                             WHERE h.federation_id = $1
                               AND h.guardian_id = g.guardian_id
                               AND h.hour >= $2
                               AND h.online_samples > 0) AS last_online_hour
                     FROM guardians g
                     WHERE g.guardian_id IS NOT NULL
                     ORDER BY g.guardian_id"
                ),
                &[
                    &id,
                    &(now - TimeDelta::days(LAST_ONLINE_LOOKBACK_DAYS)).naive_utc(),
                ],
            )
            .await?;

        let mut guardians: Vec<GuardianHealthSeries> = last_online
            .iter()
            .map(|row| GuardianHealthSeries {
                guardian_id: row.get::<_, i32>("guardian_id") as u16,
                uptime_pct: None,
                avg_latency_ms: None,
                last_online_hour: row
                    .get::<_, Option<NaiveDateTime>>("last_online_hour")
                    .map(|hour| hour.and_utc()),
                buckets: Vec::new(),
            })
            .collect();

        // Per guardian, as in `guardians`: each bucket's (latency sum, timed samples)
        let mut latency: Vec<Vec<(i64, i64)>> = vec![Vec::new(); guardians.len()];
        for row in &rows {
            let guardian_id = row.get::<_, i32>("guardian_id") as u16;
            let Some(index) = guardians
                .iter()
                .position(|series| series.guardian_id == guardian_id)
            else {
                continue;
            };
            let series = &mut guardians[index];
            let latency_samples: i64 = row.get("latency_samples");
            let latency_sum_ms: i64 = row.get("latency_sum_ms");
            latency[index].push((latency_sum_ms, latency_samples));
            series.buckets.push(GuardianHealthBucket {
                start: row.get::<_, NaiveDateTime>("bucket").and_utc(),
                samples: clamp_u32(row.get("samples")),
                online_samples: clamp_u32(row.get("online_samples")),
                polled_s: non_negative(row.get("polled_s")),
                online_s: non_negative(row.get("online_s")),
                avg_latency_ms: ratio(latency_sum_ms, latency_samples),
            });
        }

        for (series, latency) in guardians.iter_mut().zip(latency) {
            let polled: u64 = series.buckets.iter().map(|b| b.polled_s).sum();
            let online: u64 = series.buckets.iter().map(|b| b.online_s).sum();
            series.uptime_pct = ratio(online as i64, polled as i64).map(|share| share * 100.0);
            series.avg_latency_ms = window_latency_ms(latency);
        }

        Ok(GuardianHealthHistory {
            window,
            bucket_seconds: bucket.num_seconds() as u32,
            start,
            bucket_count,
            generated_at: now,
            guardians,
        })
    }

    pub async fn guardian_live_health(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<FederationLiveHealth> {
        self.ensure_observed(federation_id).await?;
        let now = Utc::now();
        // Polls are a minute apart while any guardian answers and at most 30
        // minutes apart otherwise, so the last hour holds the latest poll
        let since = (now - TimeDelta::hours(1)).naive_utc();
        let rows = self
            .connection()
            .await?
            .query(
                &format!(
                    "{GUARDIANS_CTE}
                     SELECT g.guardian_id,
                            polled.time AS polled_at,
                            answered.time AS answered_at,
                            answered.latency_ms,
                            answered.status
                     FROM guardians g
                     LEFT JOIN LATERAL (
                         SELECT h.time
                         FROM guardian_health h
                         WHERE h.federation_id = $1
                           AND h.guardian_id = g.guardian_id
                           AND h.time >= $2
                         ORDER BY h.time DESC
                         LIMIT 1
                     ) polled ON true
                     LEFT JOIN LATERAL (
                         SELECT h.time, h.latency_ms, h.status
                         FROM guardian_health h
                         WHERE h.federation_id = $1
                           AND h.guardian_id = g.guardian_id
                           AND h.time >= $2
                           AND h.status IS NOT NULL
                         ORDER BY h.time DESC
                         LIMIT 1
                     ) answered ON true
                     WHERE g.guardian_id IS NOT NULL
                     ORDER BY g.guardian_id"
                ),
                &[&federation_id.consensus_encode_to_vec(), &since],
            )
            .await?;

        let guardians = rows
            .iter()
            .map(|row| {
                let status = row
                    .get::<_, Option<Value>>("status")
                    .map(|status| reported_status(&status));
                GuardianLiveStatus {
                    guardian_id: row.get::<_, i32>("guardian_id") as u16,
                    polled_at: row
                        .get::<_, Option<NaiveDateTime>>("polled_at")
                        .map(|time| time.and_utc()),
                    answered_at: row
                        .get::<_, Option<NaiveDateTime>>("answered_at")
                        .map(|time| time.and_utc()),
                    latency_ms: row
                        .get::<_, Option<i32>>("latency_ms")
                        .and_then(|ms| u32::try_from(ms).ok()),
                    session_count: status.as_ref().and_then(|s| s.session_count),
                    peers_online: status.as_ref().and_then(|s| s.peers_online),
                    peers_offline: status.as_ref().and_then(|s| s.peers_offline),
                    peers_flagged: status.as_ref().and_then(|s| s.peers_flagged),
                    scheduled_shutdown: status.as_ref().and_then(|s| s.scheduled_shutdown),
                    peers: status.map(|s| s.peers).unwrap_or_default(),
                }
            })
            .collect();

        Ok(FederationLiveHealth {
            generated_at: now,
            guardians,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(in crate::federation) struct HistoryParams {
    window: Option<GuardianHealthWindow>,
}

pub(in crate::federation) async fn get_guardian_health_history(
    Path(federation_id): Path<FederationId>,
    Query(params): Query<HistoryParams>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<GuardianHealthHistory>> {
    Ok(Json(
        state
            .federation_observer
            .guardian_health_history(
                federation_id,
                params.window.unwrap_or(GuardianHealthWindow::Week),
            )
            .await?,
    ))
}

pub(in crate::federation) async fn get_guardian_live_health(
    Path(federation_id): Path<FederationId>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<FederationLiveHealth>> {
    Ok(Json(
        state
            .federation_observer
            .guardian_live_health(federation_id)
            .await?,
    ))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn windows_end_with_the_current_bucket() {
        let now = Utc.with_ymd_and_hms(2026, 11, 11, 15, 42, 7).unwrap();
        assert_eq!(
            window_start(now, GuardianHealthWindow::Day),
            Utc.with_ymd_and_hms(2026, 11, 10, 16, 0, 0).unwrap()
        );
        assert_eq!(
            window_start(now, GuardianHealthWindow::Week),
            Utc.with_ymd_and_hms(2026, 11, 4, 16, 0, 0).unwrap()
        );
        assert_eq!(
            window_start(now, GuardianHealthWindow::Month),
            Utc.with_ymd_and_hms(2026, 10, 13, 0, 0, 0).unwrap()
        );
    }

    #[test]
    fn window_latency_weighs_every_timed_request_alike() {
        // Hour 1: 10 polls answered but only 1 request timed, at 1000 ms.
        // Hour 2: 1 poll answered but 9 requests timed, at 100 ms each.
        // Weighting the hours' means by answered polls gave
        // (1000·10 + 100·1) / 11 ≈ 918 ms; the 10 timed requests average 190 ms.
        assert_eq!(window_latency_ms([(1000, 1), (900, 9)]), Some(190.0));
        // Timed requests in a bucket without answered polls still count
        assert_eq!(window_latency_ms([(0, 0), (300, 3)]), Some(100.0));
        assert_eq!(window_latency_ms([(0, 0)]), None);
        assert_eq!(window_latency_ms([]), None);
    }

    #[test]
    fn windows_parse_from_their_query_names() {
        let parse = |text: &str| serde_json::from_value::<GuardianHealthWindow>(Value::from(text));
        assert_eq!(parse("24h").unwrap(), GuardianHealthWindow::Day);
        assert_eq!(parse("7d").unwrap(), GuardianHealthWindow::Week);
        assert_eq!(parse("30d").unwrap(), GuardianHealthWindow::Month);
        assert!(parse("90d").is_err());
    }

    #[test]
    fn peer_views_are_read_leniently() {
        let status = serde_json::json!({
            "server": "consensus_running",
            "federation": {
                "session_count": 4021,
                "status_by_peer": {
                    "2": { "last_contribution": 4020, "connection_status": "disconnected", "flagged": true },
                    "1": { "last_contribution": 4021, "connection_status": "connected", "flagged": false },
                    "x": { "connection_status": "connected" },
                    "3": { "flagged": false }
                },
                "peers_online": 1,
                "peers_offline": 1,
                "peers_flagged": 1,
                "scheduled_shutdown": null
            }
        });
        let reported = reported_status(&status);
        assert_eq!(reported.session_count, Some(4021));
        assert_eq!(reported.peers_flagged, Some(1));
        assert_eq!(reported.scheduled_shutdown, None);
        let peers: Vec<_> = reported
            .peers
            .iter()
            .map(|p| (p.peer_id, p.connected, p.flagged, p.last_contribution))
            .collect();
        assert_eq!(
            peers,
            vec![(1, true, false, Some(4021)), (2, false, true, Some(4020))]
        );

        // A guardian still in setup reports no federation status
        let setup = reported_status(
            &serde_json::json!({ "server": "awaiting_password", "federation": null }),
        );
        assert!(setup.peers.is_empty());
        assert_eq!(setup.session_count, None);
    }
}
