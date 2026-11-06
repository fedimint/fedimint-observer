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
                "INSERT INTO fmo_lnv2.gateway_keys VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
                &[&fed, &i32::from(instance), &url, &key],
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
        let deadline: Option<chrono::DateTime<chrono::Utc>> = services.connection().await?.query_one(
            "SELECT min(deadline) FROM (
                SELECT min(COALESCE(probed_at+interval '5 minutes',now())) AS deadline
                FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2 AND registered
                UNION ALL SELECT min(hour)+interval '90 days' FROM fmo_lnv2.gateway_health_hourly
             ) due", &[&bytes,&i32::from(instance)],
        ).await?.get(0);
        match deadline {
            Some(at) => tokio::select! {
                result = wakeup.changed() => { result?; }
                _ = tokio::time::sleep((at-chrono::Utc::now()).to_std().unwrap_or_default()) => {}
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
    let started = Instant::now();
    let result = match target {
        Some(target) => target.routing_info(&fed).await,
        None => super::bounded_iroh::routing_info(&endpoint, &fed).await,
    };
    let elapsed = i32::try_from(started.elapsed().as_millis())?;
    drop(permit);
    let observation = match result {
        Ok(info) => Observation::Reachable {
            latency: elapsed,
            routing: info.map(serde_json::to_value).transpose()?,
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
