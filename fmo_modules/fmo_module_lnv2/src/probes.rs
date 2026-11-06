use std::time::{Duration, Instant};

use anyhow::Context;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fedimint_core::util::SafeUrl;
use fmo_core::module::{DbTransaction, Services};
use futures::{stream, StreamExt};

pub(crate) const SAVE_PROBE_SQL:&str =
    "UPDATE fmo_lnv2.gateways SET probed_at=now(),reachable=$4,federation_supported=$5,latency_ms=$6,
     routing_info=COALESCE($7,routing_info),
     routing_observed_at=CASE WHEN $7::jsonb IS NOT NULL THEN now() ELSE routing_observed_at END
     WHERE federation_id=$1 AND module_instance_id=$2 AND api_endpoint=$3 AND registered";

/// Module keys an endpoint may hold for activity attribution. Keys are
/// self-reported, so an endpoint must not accumulate every key it ever named.
pub(crate) const MAX_KEYS_PER_ENDPOINT: i64 = 8;

pub(crate) const PRUNE_KEYS_SQL: &str = "DELETE FROM fmo_lnv2.gateway_keys
     WHERE federation_id=$1 AND module_instance_id=$2 AND api_endpoint=$3
       AND module_public_key NOT IN (
         SELECT module_public_key FROM fmo_lnv2.gateway_keys
         WHERE federation_id=$1 AND module_instance_id=$2 AND api_endpoint=$3
         ORDER BY last_claimed DESC,module_public_key LIMIT $4)";

/// Seconds until the next probe or retention deadline, by the database clock;
/// negative when overdue, NULL when there is none (wait for a notification).
pub(crate) const NEXT_PROBE_SQL: &str =
    "SELECT EXTRACT(EPOCH FROM min(deadline)-now())::float8 FROM (
        SELECT min(COALESCE(probed_at+interval '5 minutes',now())) AS deadline
        FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2 AND registered
        UNION ALL SELECT min(hour)+interval '90 days' FROM fmo_lnv2.gateway_health_hourly
     ) due";

/// Characters of a gateway's self-reported alias that are stored and served.
/// A Lightning node alias is at most 32 bytes; the reply may carry up to the
/// response limit, which the public API would otherwise return for every
/// gateway.
pub(crate) const MAX_ALIAS_CHARS: usize = 64;

/// The routing info to store: every other field has a fixed size, and the
/// alias is cut to `MAX_ALIAS_CHARS` without control characters (PostgreSQL
/// JSONB rejects NUL, which would fail the probe's write on every attempt).
pub(crate) fn bounded_routing(
    mut info: fedimint_lnv2_common::gateway_api::RoutingInfo,
) -> serde_json::Result<serde_json::Value> {
    if let Some(alias) = &mut info.lightning_alias {
        *alias = alias
            .chars()
            .filter(|character| !character.is_control())
            .take(MAX_ALIAS_CHARS)
            .collect();
    }
    serde_json::to_value(info)
}

pub(crate) enum Observation {
    Unreachable,
    Reachable {
        latency: i32,
        routing: Option<serde_json::Value>,
    },
}

pub(crate) async fn save(
    tx: &DbTransaction<'_>,
    fed: &[u8],
    instance: u16,
    url: &str,
    observation: Observation,
) -> anyhow::Result<()> {
    let (reachable, supported, latency, routing) = match observation {
        Observation::Unreachable => (false, None, None, None),
        Observation::Reachable { latency, routing } => {
            (true, Some(routing.is_some()), Some(latency), routing)
        }
    };
    let updated = tx
        .execute(
            SAVE_PROBE_SQL,
            &[
                &fed,
                &i32::from(instance),
                &url,
                &reachable,
                &supported,
                &latency,
                &routing,
            ],
        )
        .await?;
    if updated > 0 {
        if let Some(key) = routing
            .as_ref()
            .and_then(|info| info.get("module_public_key"))
            .and_then(serde_json::Value::as_str)
        {
            tx.execute(
                // clock_timestamp, not now(): claims order correctly even
                // within one transaction.
                "INSERT INTO fmo_lnv2.gateway_keys(federation_id,module_instance_id,api_endpoint,module_public_key,last_claimed)
                 VALUES($1,$2,$3,$4,clock_timestamp())
                 ON CONFLICT(federation_id,module_instance_id,api_endpoint,module_public_key)
                 DO UPDATE SET last_claimed=clock_timestamp()",
                &[&fed, &i32::from(instance), &url, &key],
            )
            .await?;
            // Rotation keeps the most recently claimed keys: the current one
            // always stays attributed and the least recently claimed drops off.
            tx.execute(
                PRUNE_KEYS_SQL,
                &[&fed, &i32::from(instance), &url, &MAX_KEYS_PER_ENDPOINT],
            )
            .await?;
        }
        let online = i64::from(reachable && supported == Some(true));
        let latency_count = i64::from(latency.is_some());
        tx.execute(
            "INSERT INTO fmo_lnv2.gateway_health_hourly AS h
             (federation_id,module_instance_id,api_endpoint,hour,samples,online_samples,latency_sum_ms,latency_samples)
             VALUES($1,$2,$3,date_trunc('hour',now()),1,$4,$5,$6)
             ON CONFLICT(federation_id,module_instance_id,api_endpoint,hour)
             DO UPDATE SET samples=h.samples+1,online_samples=h.online_samples+EXCLUDED.online_samples,
                 latency_sum_ms=h.latency_sum_ms+EXCLUDED.latency_sum_ms,latency_samples=h.latency_samples+EXCLUDED.latency_samples",
            &[&fed,&i32::from(instance),&url,&online,&i64::from(latency.unwrap_or(0)),&latency_count],
        ).await?;
    }
    Ok(())
}

pub(crate) async fn run(
    services: Services,
    fed: FederationId,
    instance: u16,
) -> anyhow::Result<()> {
    let mut wakeup = services.work_notifications(&format!("lnv2:{fed}:{instance}"));
    let bytes = fed.consensus_encode_to_vec();
    loop {
        wakeup.borrow_and_update();
        let rows=services.connection().await?.query(
            "SELECT api_endpoint FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2
             AND registered AND (probed_at IS NULL OR probed_at<now()-interval '5 minutes')
             ORDER BY probed_at NULLS FIRST,api_endpoint LIMIT 100",&[&bytes,&i32::from(instance)],
        ).await?;
        let errors = stream::iter(rows).map(|row| {
            let services=services.clone();
            async move {
                let url:String=row.get(0);
                if let Err(error)=probe(&services,fed,instance,&url).await {
                    tracing::warn!(%fed,instance,%error,"Gateway probe could not be recorded; keeping previous observation");
                    return true;
                }
                false
            }
        }).buffer_unordered(4).collect::<Vec<_>>().await;
        services.connection().await?.execute(
            "DELETE FROM fmo_lnv2.gateway_health_hourly WHERE (federation_id,module_instance_id,api_endpoint,hour) IN
             (SELECT federation_id,module_instance_id,api_endpoint,hour FROM fmo_lnv2.gateway_health_hourly
              WHERE hour<now()-interval '90 days' ORDER BY hour LIMIT 1000 FOR UPDATE SKIP LOCKED)",&[],
        ).await?;
        if errors.iter().any(|error| *error) {
            tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        }
        // The wait is measured against the database clock that the due-work
        // queries use; an app clock ahead of it would otherwise wake early and spin.
        let wait: Option<f64> = services
            .connection()
            .await?
            .query_one(NEXT_PROBE_SQL, &[&bytes, &i32::from(instance)])
            .await?
            .get(0);
        match wait {
            Some(seconds) => tokio::select! {
                result = wakeup.changed() => { result?; }
                _ = tokio::time::sleep(Services::wait_for_deadline(seconds)) => {}
            },
            None => {
                wakeup.changed().await?;
            }
        }
    }
}

async fn probe(
    services: &Services,
    fed: FederationId,
    instance: u16,
    url: &str,
) -> anyhow::Result<()> {
    // Queue overload is unknown observation coverage, not evidence of gateway
    // downtime.
    let permit = tokio::time::timeout(Duration::from_secs(5), services.external_io_permit())
        .await
        .context("Gateway probe queue is full")??;
    let endpoint: SafeUrl = url.parse()?;
    let target = if endpoint.scheme() == "iroh" {
        None
    } else {
        match super::public_http::Target::prepare(&endpoint).await {
            Ok(target) => Some(target),
            Err(error) => {
                drop(permit);
                // Policy/DNS failures have no remote probe sample. Record an
                // unknown state and a retry timestamp without claiming downtime.
                services.connection().await?.execute(
                    "UPDATE fmo_lnv2.gateways SET probed_at=now(),reachable=NULL,federation_supported=NULL,latency_ms=NULL
                     WHERE federation_id=$1 AND module_instance_id=$2 AND api_endpoint=$3",
                    &[&fed.consensus_encode_to_vec(),&i32::from(instance),&url],
                ).await?;
                return Err(error);
            }
        }
    };
    // Latency starts at the request on both transports: DNS for HTTPS and
    // local endpoint setup for Iroh happen before it. Connection handshakes count.
    let result = match target {
        Some(target) => {
            let started = Instant::now();
            target
                .routing_info(&fed)
                .await
                .map(|info| (info, started.elapsed()))
        }
        None => super::bounded_iroh::routing_info(&endpoint, &fed).await,
    };
    drop(permit);
    let observation = match result {
        Ok((info, elapsed)) => Observation::Reachable {
            latency: i32::try_from(elapsed.as_millis())?,
            routing: info.map(bounded_routing).transpose()?,
        },
        Err(_) => Observation::Unreachable,
    };
    let mut db = services.connection().await?;
    let tx = db.transaction().await?;
    save(
        &tx,
        &fed.consensus_encode_to_vec(),
        instance,
        url,
        observation,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) fn status(
    registered: bool,
    probed_at: Option<chrono::DateTime<chrono::Utc>>,
    reachable: Option<bool>,
    supported: Option<bool>,
) -> &'static str {
    if !registered {
        return "removed";
    }
    if probed_at.is_none_or(|at| chrono::Utc::now().signed_duration_since(at).num_seconds() > 900) {
        return "unknown";
    }
    match (reachable, supported) {
        (Some(true), Some(true)) => "online",
        (Some(true), _) => "degraded",
        (Some(false), _) => "offline",
        _ => "unknown",
    }
}
