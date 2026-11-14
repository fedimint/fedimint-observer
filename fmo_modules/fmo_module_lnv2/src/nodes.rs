//! A federation's gateways grouped by Lightning node, over both protocols and
//! all lnv2 module instances. `/gateways/v2` can't group them: it covers one
//! instance, paged by endpoint.
use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fmo_api_types::{
    GatewayNode, GatewayNodeTotals, GatewayNodes, GatewayRegistration, GatewayV2Activity,
};
use fmo_core::module::{ApiError, ApiResult, ClientError, DbTransaction, Services};
use serde::Deserialize;

use crate::gateways::{attribution, window_buckets, window_start, REGISTRY_SQL};
use crate::lnv1_link::{node_key, LNV1_GATEWAYS_SQL};

/// The lnv2 module instances with registered endpoints (`$1` federation)
pub(crate) const INSTANCES_SQL: &str = "SELECT DISTINCT module_instance_id FROM fmo_lnv2.gateways
     WHERE federation_id=$1 AND registered ORDER BY module_instance_id";

/// As `/gateways/v2` reports it (`$1` federation)
const ACTIVITY_CAUGHT_UP_SQL: &str =
    "SELECT COALESCE((SELECT next_session FROM module_cursors WHERE federation_id=$1 AND module_kind='lnv2'),0)
     >=COALESCE((SELECT MAX(session_index)::bigint+1 FROM sessions WHERE federation_id=$1),0)";

/// Every read of one response sees the same committed state.
/// `REGISTRY_SQL` pages are separate statements, and a poll committing between
/// two of them could credit one contract both to the endpoint it retires and
/// to the one it registers. Must be the transaction's first statement.
pub(crate) const SNAPSHOT_SQL: &str = "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY";

/// Endpoints read per `REGISTRY_SQL` page, as `/gateways/v2` caps its pages
const PAGE: i64 = 100;

#[derive(Deserialize, Default)]
pub(crate) struct Params {
    window: Option<String>,
}

/// A `REGISTRY_SQL` health bucket, as far as the totals need it
#[derive(Deserialize)]
struct Health {
    samples: u64,
    online_samples: u64,
}

pub(crate) struct Lnv1Gateway {
    pub(crate) gateway_id: String,
    pub(crate) node_pub_key: String,
    pub(crate) api_endpoint: String,
    pub(crate) first_seen: DateTime<Utc>,
    pub(crate) last_seen: DateTime<Utc>,
}

pub(crate) struct Lnv2Gateway {
    pub(crate) module_instance_id: u16,
    pub(crate) api_endpoint: String,
    pub(crate) first_seen: DateTime<Utc>,
    pub(crate) last_seen: DateTime<Utc>,
    pub(crate) lightning_public_key: Option<String>,
    pub(crate) status: String,
    pub(crate) samples: u64,
    pub(crate) online_samples: u64,
    pub(crate) activity: GatewayV2Activity,
    pub(crate) attribution: String,
}

pub(crate) async fn nodes(
    Path(fed): Path<FederationId>,
    Query(params): Query<Params>,
    State(services): State<Services>,
) -> ApiResult<Json<GatewayNodes>> {
    services
        .federation(fed)
        .await?
        .ok_or_else(|| ClientError::not_found(format!("Federation {fed} is not observed")))?;
    let window = params.window.as_deref().unwrap_or("7d");
    let (bucket_count, bucket_hours) = window_buckets(window)
        .ok_or_else(|| ApiError::bad_request("window must be 24h, 7d, 30d or 90d"))?;
    let since = window_start(Utc::now(), bucket_count, bucket_hours);
    let mut connection = services.connection().await?;
    let tx = connection.transaction().await?;
    tx.batch_execute(SNAPSHOT_SQL).await?;
    let bytes = fed.consensus_encode_to_vec();
    let lnv1 = tx
        .query(LNV1_GATEWAYS_SQL, &[&bytes])
        .await?
        .iter()
        .map(|row| Lnv1Gateway {
            gateway_id: row.get(0),
            node_pub_key: row.get(1),
            api_endpoint: row.get(2),
            first_seen: row.get(3),
            last_seen: row.get(4),
        })
        .collect();
    let lnv2 = lnv2_gateways(&tx, &bytes, since, bucket_count, bucket_hours, PAGE).await?;
    let activity_caught_up = tx
        .query_one(ACTIVITY_CAUGHT_UP_SQL, &[&bytes])
        .await?
        .get(0);
    Ok(Json(GatewayNodes {
        activity_caught_up,
        window: window.to_owned(),
        ..group(lnv1, lnv2)
    }))
}

/// A federation's registered Lightning v2 endpoints in every module instance,
/// with the node key each last reported and their numbers over the window
/// from `since`, read through `REGISTRY_SQL` so they match `/gateways/v2`.
/// Probes keep the last routing info when they get none, so the key may be
/// stale.
pub(crate) async fn lnv2_gateways(
    tx: &DbTransaction<'_>,
    fed: &[u8],
    since: DateTime<Utc>,
    bucket_count: i32,
    bucket_hours: i64,
    page: i64,
) -> anyhow::Result<Vec<Lnv2Gateway>> {
    let mut gateways = Vec::new();
    for instance in tx.query(INSTANCES_SQL, &[&fed]).await? {
        let instance: i32 = instance.get(0);
        let mut after: Option<String> = None;
        loop {
            let rows = tx
                .query(
                    REGISTRY_SQL,
                    &[
                        &fed,
                        &instance,
                        &after,
                        &page,
                        &since,
                        &((bucket_hours * 3600) as f64),
                        &bucket_count,
                    ],
                )
                .await?;
            for row in rows.iter().filter(|row| row.get::<_, bool>(2)) {
                let health: Vec<Health> = serde_json::from_value(row.get(11))?;
                let routing_info: Option<serde_json::Value> = row.get(5);
                gateways.push(Lnv2Gateway {
                    module_instance_id: u16::try_from(instance)?,
                    api_endpoint: row.get(0),
                    first_seen: row.get(3),
                    last_seen: row.get(4),
                    lightning_public_key: routing_info
                        .as_ref()
                        .and_then(|info| info.get("lightning_public_key"))
                        .and_then(|key| key.as_str())
                        .map(ToOwned::to_owned),
                    status: super::probes::status(true, row.get(7), row.get(8), row.get(9))
                        .to_owned(),
                    samples: health.iter().map(|bucket| bucket.samples).sum(),
                    online_samples: health.iter().map(|bucket| bucket.online_samples).sum(),
                    activity: serde_json::from_value(row.get(12))?,
                    attribution: attribution(row.get(13), row.get(14)).to_owned(),
                });
            }
            if i64::try_from(rows.len())? < page {
                break;
            }
            after = rows.last().map(|row| row.get(0));
        }
    }
    Ok(gateways)
}

/// Groups registrations by node key. A Lightning v2 endpoint's key is
/// self-reported, so it links to a node unverified unless a Lightning v1
/// registration of that node announces the same endpoint, and only verified
/// registrations count towards the node's totals.
pub(crate) fn group(lnv1: Vec<Lnv1Gateway>, lnv2: Vec<Lnv2Gateway>) -> GatewayNodes {
    let mut nodes: BTreeMap<String, Vec<GatewayRegistration>> = BTreeMap::new();
    let mut unlinked = Vec::new();
    for gateway in lnv1 {
        let registration = GatewayRegistration::Lnv1 {
            gateway_id: gateway.gateway_id,
            api_endpoint: gateway.api_endpoint,
            first_seen: gateway.first_seen,
            last_seen: gateway.last_seen,
        };
        match node_key(&gateway.node_pub_key) {
            Some(key) => nodes.entry(key).or_default().push(registration),
            None => unlinked.push(registration),
        }
    }
    for gateway in lnv2 {
        let key = gateway.lightning_public_key.as_deref().and_then(node_key);
        // Every Lightning v1 registration is grouped by now
        let registrations = key.as_ref().and_then(|key| nodes.get(key));
        let url_matches_lnv1 = registrations.into_iter().flatten().any(|registration| {
            matches!(registration, GatewayRegistration::Lnv1 { api_endpoint, .. }
                if *api_endpoint == gateway.api_endpoint)
        });
        let registration = GatewayRegistration::Lnv2 {
            module_instance_id: gateway.module_instance_id,
            api_endpoint: gateway.api_endpoint,
            first_seen: gateway.first_seen,
            last_seen: gateway.last_seen,
            status: gateway.status,
            url_matches_lnv1,
            samples: gateway.samples,
            online_samples: gateway.online_samples,
            activity: gateway.activity,
            attribution: gateway.attribution,
        };
        match key {
            Some(key) => nodes.entry(key).or_default().push(registration),
            None => unlinked.push(registration),
        }
    }
    GatewayNodes {
        activity_caught_up: false,
        window: String::new(),
        nodes: nodes
            .into_iter()
            .map(|(node_pub_key, registrations)| GatewayNode {
                node_pub_key,
                totals: totals(&registrations),
                registrations,
            })
            .collect(),
        unlinked,
    }
}

fn totals(registrations: &[GatewayRegistration]) -> GatewayNodeTotals {
    let mut totals = GatewayNodeTotals::default();
    for registration in registrations {
        if let GatewayRegistration::Lnv2 {
            url_matches_lnv1: true,
            samples,
            online_samples,
            activity,
            ..
        } = registration
        {
            totals.samples += samples;
            totals.online_samples += online_samples;
            let total = &mut totals.activity;
            total.incoming_contracts += activity.incoming_contracts;
            total.outgoing_contracts += activity.outgoing_contracts;
            total.incoming_msat += activity.incoming_msat;
            total.outgoing_msat += activity.outgoing_msat;
            total.outgoing_claimed += activity.outgoing_claimed;
            total.outgoing_refunded += activity.outgoing_refunded;
            total.outgoing_cancelled += activity.outgoing_cancelled;
        }
    }
    totals
}
