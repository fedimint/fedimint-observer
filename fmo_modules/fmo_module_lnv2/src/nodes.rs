//! A federation's gateways grouped by Lightning node, over both protocols and
//! all lnv2 module instances. `/gateways/v2` can't group them: it covers one
//! instance, paged by endpoint.
use std::collections::BTreeMap;

use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fmo_api_types::{GatewayNode, GatewayNodes, GatewayRegistration};
use fmo_core::module::{ApiResult, ClientError, Services};

use crate::lnv1_link::{node_key, LNV1_GATEWAYS_SQL};

/// A federation's registered Lightning v2 endpoints in every module instance
/// (`$1` federation), with the node key each last reported. Probes keep the
/// last routing info when they get none, so the key may be stale.
pub(crate) const LNV2_GATEWAYS_SQL: &str =
    "SELECT module_instance_id,api_endpoint,first_seen,last_seen,
         routing_info->>'lightning_public_key',probed_at,reachable,federation_supported
     FROM fmo_lnv2.gateways WHERE federation_id=$1 AND registered
     ORDER BY module_instance_id,api_endpoint";

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
}

pub(crate) async fn nodes(
    Path(fed): Path<FederationId>,
    State(services): State<Services>,
) -> ApiResult<Json<GatewayNodes>> {
    services
        .federation(fed)
        .await?
        .ok_or_else(|| ClientError::not_found(format!("Federation {fed} is not observed")))?;
    let connection = services.connection().await?;
    let bytes = fed.consensus_encode_to_vec();
    let lnv1 = connection
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
    let lnv2 = connection
        .query(LNV2_GATEWAYS_SQL, &[&bytes])
        .await?
        .iter()
        .map(|row| {
            Ok(Lnv2Gateway {
                module_instance_id: u16::try_from(row.get::<_, i32>(0))?,
                api_endpoint: row.get(1),
                first_seen: row.get(2),
                last_seen: row.get(3),
                lightning_public_key: row.get(4),
                status: super::probes::status(true, row.get(5), row.get(6), row.get(7)).to_owned(),
            })
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(Json(group(lnv1, lnv2)))
}

/// Groups registrations by node key. A Lightning v2 endpoint's key is
/// self-reported, so it links to a node unverified unless a Lightning v1
/// registration of that node announces the same endpoint.
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
        };
        match key {
            Some(key) => nodes.entry(key).or_default().push(registration),
            None => unlinked.push(registration),
        }
    }
    GatewayNodes {
        nodes: nodes
            .into_iter()
            .map(|(node_pub_key, registrations)| GatewayNode {
                node_pub_key,
                registrations,
            })
            .collect(),
        unlinked,
    }
}
