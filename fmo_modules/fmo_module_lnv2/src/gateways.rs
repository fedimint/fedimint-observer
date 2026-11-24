use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Context as _;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, DurationRound, Utc};
use fedimint_api_client::api::FederationApiExt;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::ApiRequestErased;
use fedimint_core::util::SafeUrl;
use fedimint_lnv2_common::endpoint_constants::GATEWAYS_ENDPOINT;
use fmo_api_types::{GatewayV2Info as Gateway, GatewayV2Registry as Registry, GatewayV2Totals};
use fmo_core::module::{ApiError, ApiResult, ClientError, DbTransaction, Services};
use futures::{stream, StreamExt};
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct Params {
    module_instance_id: Option<u16>,
    after: Option<String>,
    limit: Option<u16>,
    window: Option<String>,
}

#[derive(Deserialize)]
struct HealthTotals {
    bucket: i32,
    samples: u64,
    online_samples: u64,
    latency_sum_ms: u64,
    latency_samples: u64,
}

/// Bucket count and whole hours per bucket.
pub(crate) fn window_buckets(window: &str) -> Option<(i32, i64)> {
    match window {
        "24h" => Some((24, 1)),
        "7d" => Some((28, 6)),
        "30d" => Some((30, 24)),
        "90d" => Some((30, 72)),
        _ => None,
    }
}

/// Start of the first bucket.
pub(crate) fn window_start(now: DateTime<Utc>, buckets: i32, hours: i64) -> DateTime<Utc> {
    let hour = now
        .duration_trunc(chrono::Duration::hours(1))
        .expect("truncating the current time to an hour cannot overflow");
    hour - chrono::Duration::hours(i64::from(buckets) * hours - 1)
}

pub(crate) const REGISTRY_SQL: &str = include_str!("../schema/gateway_overview.sql");

pub(crate) const TOTALS_SQL: &str = "WITH g AS (
         SELECT api_endpoint,registered,latency_ms,
             CASE
                 WHEN NOT registered THEN 'removed'
                 WHEN probed_at IS NULL OR $3::timestamptz-probed_at>interval '900 seconds' THEN 'unknown'
                 WHEN reachable IS TRUE AND federation_supported IS TRUE THEN 'online'
                 WHEN reachable IS TRUE THEN 'degraded'
                 WHEN reachable IS FALSE THEN 'offline'
                 ELSE 'unknown'
             END AS status
         FROM fmo_lnv2.gateways
         WHERE federation_id=$1 AND module_instance_id=$2
     )
     SELECT
         COUNT(*) FILTER(WHERE registered)::bigint AS registered,
         COUNT(*) FILTER(WHERE status='online')::bigint AS online,
         COUNT(*) FILTER(WHERE status='degraded')::bigint AS degraded,
         COUNT(*) FILTER(WHERE status='offline')::bigint AS offline,
         COUNT(*) FILTER(WHERE status='unknown')::bigint AS unknown,
         COUNT(*) FILTER(WHERE status='removed')::bigint AS removed,
         percentile_cont(0.5) WITHIN GROUP (ORDER BY latency_ms::double precision)
             FILTER(WHERE status='online' AND latency_ms IS NOT NULL) AS median_latency_ms,
         (SELECT COALESCE(SUM(samples),0)::bigint FROM fmo_lnv2.gateway_health_hourly
          WHERE federation_id=$1 AND module_instance_id=$2 AND hour>=$4) AS samples,
         (SELECT COALESCE(SUM(online_samples),0)::bigint FROM fmo_lnv2.gateway_health_hourly
          WHERE federation_id=$1 AND module_instance_id=$2 AND hour>=$4) AS online_samples
     FROM g";

pub(crate) const AVAILABILITY_TREND_SQL: &str =
    "SELECT LEAST($5::integer-1,GREATEST(0,floor(extract(epoch FROM (hour-$3::timestamptz))/$4::double precision)::integer)) AS bucket,
         SUM(samples)::bigint AS samples,SUM(online_samples)::bigint AS online_samples
     FROM fmo_lnv2.gateway_health_hourly
     WHERE federation_id=$1 AND module_instance_id=$2 AND hour>=$3
     GROUP BY 1";

pub(crate) fn instances(config: &ClientConfig) -> Vec<u16> {
    config
        .modules
        .iter()
        .filter_map(|(&id, module)| (module.kind.as_str() == "lnv2").then_some(id))
        .collect()
}

pub(crate) fn routes() -> Router<Services> {
    Router::new().route("/gateways/v2", get(registry))
}

async fn registry(
    Path(fed): Path<FederationId>,
    Query(params): Query<Params>,
    State(services): State<Services>,
) -> ApiResult<Json<Registry>> {
    let federation = services
        .federation(fed)
        .await?
        .ok_or_else(|| ClientError::not_found(format!("Federation {fed} is not observed")))?;
    let instance = select_instance(&instances(&federation.config), params.module_instance_id)?;
    let window = params.window.as_deref().unwrap_or("7d");
    let (bucket_count, bucket_hours) = window_buckets(window)
        .ok_or_else(|| ApiError::bad_request("window must be 24h, 7d, 30d or 90d"))?;
    let now = Utc::now();
    let since = window_start(now, bucket_count, bucket_hours);
    let limit = params.limit.unwrap_or(100);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::bad_request("limit must be 1..100"));
    }
    let connection = services.connection().await?;
    let bytes = fed.consensus_encode_to_vec();
    let poll = connection
        .query_opt(
            "SELECT observed_at, successful_guardians, total_guardians FROM fmo_lnv2.gateway_polls
         WHERE federation_id=$1 AND module_instance_id=$2",
            &[&bytes, &i32::from(instance)],
        )
        .await?;
    let observed_at: Option<DateTime<Utc>> = poll.as_ref().map(|row| row.get(0));
    let totals_row = connection
        .query_one(TOTALS_SQL, &[&bytes, &i32::from(instance), &now, &since])
        .await?;
    let count = |column: &str| u64::try_from(totals_row.get::<_, i64>(column));
    let totals = GatewayV2Totals {
        registered: count("registered")?,
        online: count("online")?,
        degraded: count("degraded")?,
        offline: count("offline")?,
        unknown: count("unknown")?,
        removed: count("removed")?,
        samples: count("samples")?,
        online_samples: count("online_samples")?,
        median_latency_ms: totals_row.get("median_latency_ms"),
    };
    let trend_rows = connection
        .query(
            AVAILABILITY_TREND_SQL,
            &[
                &bytes,
                &i32::from(instance),
                &since,
                &((bucket_hours * 3600) as f64),
                &bucket_count,
            ],
        )
        .await?;
    let mut availability_trend: Vec<_> = (0..bucket_count)
        .map(|index| fmo_api_types::GatewayV2AvailabilityBucket {
            start: since + chrono::Duration::hours(bucket_hours * i64::from(index)),
            samples: 0,
            online_samples: 0,
        })
        .collect();
    for row in trend_rows {
        if let Some(bucket) = usize::try_from(row.get::<_, i32>("bucket"))
            .ok()
            .and_then(|index| availability_trend.get_mut(index))
        {
            bucket.samples = u64::try_from(row.get::<_, i64>("samples"))?;
            bucket.online_samples = u64::try_from(row.get::<_, i64>("online_samples"))?;
        }
    }
    let rows = connection
        .query(
            REGISTRY_SQL,
            &[
                &bytes,
                &i32::from(instance),
                &params.after,
                &(i64::from(limit) + 1),
                &since,
                &((bucket_hours * 3600) as f64),
                &bucket_count,
            ],
        )
        .await?;
    let mut gateways = rows
        .iter()
        .map(|row| {
            let totals: Vec<HealthTotals> = serde_json::from_value(row.get(11))?;
            let samples = totals.iter().map(|bucket| bucket.samples).sum();
            let online_samples = totals.iter().map(|bucket| bucket.online_samples).sum();
            let latency_sum: u64 = totals.iter().map(|bucket| bucket.latency_sum_ms).sum();
            let latency_count: u64 = totals.iter().map(|bucket| bucket.latency_samples).sum();
            let availability = (0..bucket_count)
                .map(|index| {
                    let bucket = totals.iter().find(|bucket| bucket.bucket == index);
                    fmo_api_types::GatewayV2AvailabilityBucket {
                        start: since + chrono::Duration::hours(bucket_hours * i64::from(index)),
                        samples: bucket.map_or(0, |bucket| bucket.samples),
                        online_samples: bucket.map_or(0, |bucket| bucket.online_samples),
                    }
                })
                .collect();
            let registered = row.get(2);
            let probed_at = row.get(7);
            let reachable = row.get(8);
            let federation_supported = row.get(9);
            Ok(Gateway {
                api_endpoint: row.get(0),
                announcing_guardians: serde_json::from_value(row.get(1))?,
                registered,
                first_seen: row.get(3),
                last_seen: row.get(4),
                routing_info: row.get(5),
                routing_observed_at: row.get(6),
                probed_at,
                reachable,
                federation_supported,
                latency_ms: row.get(10),
                status: super::probes::status(
                    registered,
                    probed_at,
                    reachable,
                    federation_supported,
                )
                .to_owned(),
                samples,
                online_samples,
                avg_latency_ms: if latency_count > 0 {
                    Some(latency_sum as f64 / latency_count as f64)
                } else {
                    None
                },
                availability,
                activity: serde_json::from_value(row.get(12))?,
                attribution: attribution(row.get(13), row.get(14)).to_owned(),
                contested_keys: u64::try_from(row.get::<_, i64>(14))?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let more = gateways.len() > usize::from(limit);
    gateways.truncate(usize::from(limit));
    let next_after = if more {
        gateways.last().map(|g| g.api_endpoint.clone())
    } else {
        None
    };
    let activity_caught_up:bool=connection.query_one(
        "SELECT COALESCE((SELECT next_session FROM module_cursors WHERE federation_id=$1 AND module_kind='lnv2'),0)
         >=COALESCE((SELECT MAX(session_index)::bigint+1 FROM sessions WHERE federation_id=$1),0)", &[&bytes],
    ).await?.get(0);
    Ok(Json(Registry {
        activity_caught_up,
        window: window.to_owned(),
        module_instance_id: instance,
        observed_at,
        stale: observed_at
            .is_none_or(|at| Utc::now().signed_duration_since(at).num_seconds() > 900),
        successful_guardians: poll.as_ref().map_or(0, |row| row.get(1)),
        totals,
        availability_trend,
        total_guardians: poll
            .as_ref()
            .map_or(federation.config.global.api_endpoints.len() as i32, |row| {
                row.get(2)
            }),
        gateways,
        next_after,
    }))
}

/// The lnv2 module instance a request is about.
pub(crate) fn select_instance(
    instances: &[u16],
    requested: Option<u16>,
) -> Result<u16, ClientError> {
    match (requested, instances) {
        (_, []) => Err(ClientError::not_found(
            "This federation has no Lightning v2 (lnv2) module",
        )),
        (Some(id), _) if instances.contains(&id) => Ok(id),
        (Some(id), _) => Err(ClientError::not_found(format!(
            "Module instance {id} is not an lnv2 module of this federation"
        ))),
        (None, [only]) => Ok(*only),
        (None, _) => Err(ClientError::bad_request(
            "This federation has several lnv2 modules; select one with module_instance_id",
        )),
    }
}

pub(crate) fn attribution(claimed_keys: i64, contested_keys: i64) -> &'static str {
    if contested_keys > 0 {
        "contested"
    } else if claimed_keys > 0 {
        "self_reported"
    } else {
        "none"
    }
}

pub(crate) fn complete_registry(successful: usize, total: usize) -> bool {
    total > 0 && successful == total
}

pub(crate) const MAX_ENDPOINT_BYTES: usize = 1024;

/// Distinct gateways one guardian may announce per poll.
pub(crate) const MAX_URLS_PER_GUARDIAN: usize = 256;
pub(crate) const MAX_ACTIVE_GATEWAYS: i64 = 1024;
pub(crate) const MAX_RETIRED_GATEWAYS: i64 = 1024;

pub(crate) fn bounded_registrations(
    mut responses: Vec<(u16, Vec<String>)>,
) -> (BTreeMap<String, Vec<u16>>, usize) {
    let mut complete = 0;
    for (peer, urls) in &mut responses {
        let original = urls.len();
        urls.retain(|url| url.len() <= MAX_ENDPOINT_BYTES);
        let oversized = original - urls.len();
        urls.sort_unstable();
        urls.dedup();
        let excess = urls.len().saturating_sub(MAX_URLS_PER_GUARDIAN);
        urls.truncate(MAX_URLS_PER_GUARDIAN);
        if oversized > 0 || excess > 0 {
            tracing::warn!(
                peer,
                oversized,
                excess,
                "Gateway identities skipped; registry coverage is incomplete"
            );
        }
        complete += usize::from(oversized == 0 && excess == 0);
    }
    (merge_registrations(responses), complete)
}

/// Announced endpoints in admission order: more announcing guardians first.
pub(crate) fn admission_order(
    registrations: BTreeMap<String, Vec<u16>>,
) -> Vec<(String, Vec<u16>)> {
    let mut ordered: Vec<_> = registrations.into_iter().collect();
    ordered.sort_by_key(|(_, peers)| std::cmp::Reverse(peers.len()));
    ordered
}

pub(crate) const EVICT_SQL: &str = "UPDATE fmo_lnv2.gateways SET registered=false
     WHERE (federation_id,module_instance_id,api_endpoint) IN (
         SELECT federation_id,module_instance_id,api_endpoint FROM fmo_lnv2.gateways
         WHERE federation_id=$1 AND module_instance_id=$2 AND registered
           AND NOT(api_endpoint=ANY($3::text[]))
           AND jsonb_array_length(announcing_guardians)<$4
         ORDER BY jsonb_array_length(announcing_guardians),last_seen,api_endpoint
         LIMIT 1)";

pub(crate) const TRIM_ACTIVE_SQL: &str =
    "UPDATE fmo_lnv2.gateways g SET registered=false FROM (
         SELECT api_endpoint,row_number() OVER (
             ORDER BY jsonb_array_length(announcing_guardians) DESC,last_seen DESC,api_endpoint) AS rank
         FROM fmo_lnv2.gateways
         WHERE federation_id=$1 AND module_instance_id=$2 AND registered
     ) r
     WHERE g.federation_id=$1 AND g.module_instance_id=$2 AND g.api_endpoint=r.api_endpoint AND r.rank>$3";

pub(crate) const PRUNE_RETIRED_SQL: &str =
    "DELETE FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2 AND NOT registered
       AND api_endpoint NOT IN (
         SELECT api_endpoint FROM fmo_lnv2.gateways
         WHERE federation_id=$1 AND module_instance_id=$2 AND NOT registered
         ORDER BY last_seen DESC,api_endpoint LIMIT $3)";

pub(crate) async fn admit(
    tx: &DbTransaction<'_>,
    fed: &[u8],
    instance: u16,
    announced: &[String],
    registrations: BTreeMap<String, Vec<u16>>,
) -> anyhow::Result<usize> {
    let instance = i32::from(instance);
    tx.execute(TRIM_ACTIVE_SQL, &[&fed, &instance, &MAX_ACTIVE_GATEWAYS])
        .await?;
    let mut active: i64 = tx
        .query_one(
            "SELECT count(*) FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2 AND registered",
            &[&fed, &instance],
        )
        .await?
        .get(0);
    let mut skipped = 0;
    for (url, peers) in admission_order(registrations) {
        let registered: Option<bool> = tx
            .query_opt(
                "SELECT registered FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2 AND api_endpoint=$3",
                &[&fed, &instance, &url],
            )
            .await?
            .map(|row| row.get(0));
        if registered != Some(true) {
            if active < MAX_ACTIVE_GATEWAYS {
                active += 1;
            } else if tx
                .execute(
                    EVICT_SQL,
                    &[&fed, &instance, &announced, &i32::try_from(peers.len())?],
                )
                .await?
                == 0
            {
                skipped += 1;
                continue;
            }
        }
        tx.execute(
            "INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians)
             VALUES($1,$2,$3,$4) ON CONFLICT(federation_id,module_instance_id,api_endpoint)
             DO UPDATE SET last_seen=now(),registered=true,announcing_guardians=$4",
            &[&fed, &instance, &url, &serde_json::to_value(peers)?],
        )
        .await?;
    }
    Ok(skipped)
}

pub(crate) fn merge_registrations(
    responses: Vec<(u16, Vec<String>)>,
) -> BTreeMap<String, Vec<u16>> {
    let mut registrations: BTreeMap<String, Vec<u16>> = BTreeMap::new();
    for (peer, urls) in responses {
        for url in urls {
            let peers = registrations.entry(url).or_default();
            if !peers.contains(&peer) {
                peers.push(peer);
            }
        }
    }
    for peers in registrations.values_mut() {
        peers.sort_unstable();
    }
    registrations
}

pub(crate) async fn run(
    services: Services,
    fed: FederationId,
    config: ClientConfig,
) -> anyhow::Result<()> {
    futures::future::try_join_all(instances(&config).into_iter().map(|instance| {
        let services = services.clone();
        let config = config.clone();
        async move {
            tokio::try_join!(
                poll_instance(services.clone(), fed, config, instance),
                super::probes::run(services, fed, instance)
            )?;
            anyhow::Ok(())
        }
    }))
    .await?;
    Ok(())
}

async fn poll_instance(
    services: Services,
    fed: FederationId,
    config: ClientConfig,
    instance: u16,
) -> anyhow::Result<()> {
    let bytes = fed.consensus_encode_to_vec();
    let jitter = (bytes.iter().map(|b| u64::from(*b)).sum::<u64>() + u64::from(instance)) % 31;
    tokio::time::sleep(Duration::from_secs(jitter)).await;
    let mut failures = 0u32;
    loop {
        let result = tokio::time::timeout(Duration::from_secs(40), async {
            let api = services
                .federation_api(&config.global)
                .context("No guardian API client for gateway registry poll")?
                .with_module(instance);
            let _permit = services.external_io_permit().await?;
            let responses = stream::iter(config.global.api_endpoints.keys().copied())
                .map(|peer| {
                    let api = api.clone();
                    async move {
                        let value = tokio::time::timeout(
                            Duration::from_secs(10),
                            api.request_single_peer::<Vec<SafeUrl>>(
                                GATEWAYS_ENDPOINT.to_owned(),
                                ApiRequestErased::default(),
                                peer,
                            ),
                        )
                        .await;
                        match value {
                            Ok(Ok(urls)) => Some((
                                peer.to_usize() as u16,
                                urls.into_iter()
                                    .map(|url| url.to_string())
                                    .collect::<Vec<_>>(),
                            )),
                            _ => None,
                        }
                    }
                })
                .buffer_unordered(4)
                .filter_map(|item| async move { item })
                .collect::<Vec<_>>()
                .await;
            anyhow::ensure!(
                !responses.is_empty(),
                "No guardian gateway registry responses"
            );
            anyhow::Ok(responses)
        })
        .await;
        match result {
            Ok(Ok(responses)) => {
                // Validate before opening a transaction.
                let (registrations, successful) = bounded_registrations(responses);
                let mut connection = services.connection().await?;
                let tx = connection.transaction().await?;
                tx.execute(
                    "INSERT INTO fmo_lnv2.gateway_polls(federation_id,module_instance_id,observed_at,successful_guardians,total_guardians)
                     VALUES($1,$2,now(),$3,$4) ON CONFLICT(federation_id,module_instance_id)
                     DO UPDATE SET observed_at=now(),successful_guardians=$3,total_guardians=$4",
                    &[&bytes,&i32::from(instance),&(successful as i32),&(config.global.api_endpoints.len() as i32)],
                ).await?;
                let urls = registrations.keys().cloned().collect::<Vec<_>>();
                if complete_registry(successful, config.global.api_endpoints.len()) {
                    tx.execute("UPDATE fmo_lnv2.gateways SET registered=false WHERE federation_id=$1 AND module_instance_id=$2 AND registered AND NOT(api_endpoint=ANY($3::text[]))",&[&bytes,&i32::from(instance),&urls]).await?;
                }
                let skipped = admit(&tx, &bytes, instance, &urls, registrations).await?;
                if skipped > 0 {
                    tracing::warn!(%fed,instance,skipped,"Gateway registry is at capacity; new endpoints announced by fewer guardians were not stored");
                }
                tx.execute("DELETE FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2 AND NOT registered AND last_seen<now()-interval '90 days'",&[&bytes,&i32::from(instance)]).await?;
                tx.execute(
                    PRUNE_RETIRED_SQL,
                    &[&bytes, &i32::from(instance), &MAX_RETIRED_GATEWAYS],
                )
                .await?;
                tx.execute(
                    "SELECT pg_notify('fmo_work',$1)",
                    &[&format!("lnv2:{fed}:{instance}")],
                )
                .await?;
                tx.commit().await?;
                failures = 0;
            }
            error => {
                failures = failures.saturating_add(1);
                tracing::warn!(%fed,instance,?error,"Lightning v2 gateway collection failed; retaining registrations");
            }
        }
        tokio::time::sleep(Duration::from_secs(
            (300 * (1u64 << failures.min(4))).min(3600) + jitter,
        ))
        .await;
    }
}
