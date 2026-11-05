//! Lightning v2 guardians publish gateway URLs, not Lightning v1 announcements.
//! Registry membership is deliberately separate from gateway service
//! reachability.
use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::Context;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use fedimint_api_client::api::FederationApiExt;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::ApiRequestErased;
use fedimint_core::util::SafeUrl;
use fedimint_lnv2_common::endpoint_constants::GATEWAYS_ENDPOINT;
use fmo_api_types::{GatewayV2Info as Gateway, GatewayV2Registry as Registry};
use fmo_core::module::{federation_api, ApiError, ApiResult, Services};
use futures::{stream, StreamExt};
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct Params {
    module_instance_id: Option<u16>,
    after: Option<String>,
    limit: Option<u16>,
}

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
        .context("Federation not observed")?;
    let ids = instances(&federation.config);
    let instance = match params.module_instance_id {
        Some(id) if ids.contains(&id) => id,
        None if ids.len() == 1 => ids[0],
        _ => {
            return Err(ApiError::bad_request(
                "Select a configured lnv2 module_instance_id",
            ))
        }
    };
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
    let rows = connection.query(
        "SELECT api_endpoint,announcing_guardians,registered,first_seen,last_seen FROM fmo_lnv2.gateways
         WHERE federation_id=$1 AND module_instance_id=$2 AND ($3::text IS NULL OR api_endpoint > $3)
         ORDER BY api_endpoint LIMIT $4", &[&bytes,&i32::from(instance),&params.after,&(i64::from(limit)+1)],
    ).await?;
    let mut gateways = rows
        .iter()
        .map(|row| {
            Ok(Gateway {
                api_endpoint: row.get(0),
                announcing_guardians: serde_json::from_value(row.get(1))?,
                registered: row.get(2),
                first_seen: row.get(3),
                last_seen: row.get(4),
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
    Ok(Json(Registry {
        module_instance_id: instance,
        observed_at,
        stale: observed_at
            .is_none_or(|at| Utc::now().signed_duration_since(at).num_seconds() > 900),
        successful_guardians: poll.as_ref().map_or(0, |row| row.get(1)),
        total_guardians: poll
            .as_ref()
            .map_or(federation.config.global.api_endpoints.len() as i32, |row| {
                row.get(2)
            }),
        gateways,
        next_after,
    }))
}

pub(crate) fn complete_registry(successful: usize, total: usize) -> bool {
    total > 0 && successful == total
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
    futures::future::try_join_all(
        instances(&config)
            .into_iter()
            .map(|instance| poll_instance(services.clone(), fed, config.clone(), instance)),
    )
    .await?;
    Ok(())
}

async fn poll_instance(
    services: Services,
    fed: FederationId,
    config: ClientConfig,
    instance: u16,
) -> anyhow::Result<()> {
    let api = federation_api(services.connectors(), &config.global)?.with_module(instance);
    let bytes = fed.consensus_encode_to_vec();
    let jitter = (bytes.iter().map(|b| u64::from(*b)).sum::<u64>() + u64::from(instance)) % 31;
    tokio::time::sleep(Duration::from_secs(jitter)).await;
    let mut failures = 0u32;
    loop {
        let result = tokio::time::timeout(Duration::from_secs(40), async {
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
                let successful = responses.len();
                let registrations = merge_registrations(responses);
                let mut connection = services.connection().await?;
                let tx = connection.transaction().await?;
                tx.execute(
                    "INSERT INTO fmo_lnv2.gateway_polls(federation_id,module_instance_id,observed_at,successful_guardians,total_guardians)
                     VALUES($1,$2,now(),$3,$4) ON CONFLICT(federation_id,module_instance_id)
                     DO UPDATE SET observed_at=now(),successful_guardians=$3,total_guardians=$4",
                    &[&bytes,&i32::from(instance),&(successful as i32),&(config.global.api_endpoints.len() as i32)],
                ).await?;
                // A partial response cannot establish that an endpoint was removed.
                if complete_registry(successful, config.global.api_endpoints.len()) {
                    let urls = registrations.keys().cloned().collect::<Vec<_>>();
                    tx.execute("UPDATE fmo_lnv2.gateways SET registered=false WHERE federation_id=$1 AND module_instance_id=$2 AND registered AND NOT(api_endpoint=ANY($3::text[]))",&[&bytes,&i32::from(instance),&urls]).await?;
                }
                for (url, peers) in registrations {
                    tx.execute(
                        "INSERT INTO fmo_lnv2.gateways(federation_id,module_instance_id,api_endpoint,announcing_guardians)
                         VALUES($1,$2,$3,$4) ON CONFLICT(federation_id,module_instance_id,api_endpoint)
                         DO UPDATE SET last_seen=now(),registered=true,announcing_guardians=$4",
                        &[&bytes,&i32::from(instance),&url,&serde_json::to_value(peers)?],
                    ).await?;
                }
                // Bound retired registry storage; active registrations remain.
                tx.execute("DELETE FROM fmo_lnv2.gateways WHERE federation_id=$1 AND module_instance_id=$2 AND NOT registered AND last_seen<now()-interval '90 days'",&[&bytes,&i32::from(instance)]).await?;
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
