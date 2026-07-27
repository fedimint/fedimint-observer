pub mod db;
pub(crate) mod gateways;
mod guardians;
mod meta;
pub(crate) mod nostr;
pub mod observer;
mod session;
mod transaction;

use std::collections::{HashMap, HashSet};

use anyhow::Context;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use axum_auth::AuthBearer;
use bitcoin::OutPoint;
use fedimint_core::config::{ClientConfig, FederationId, JsonClientConfig};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::module::registry::ModuleDecoderRegistry;
use fmo_api_types::{
    FederationSummary, FederationUtxo, FederationUtxosResponse, FedimintTotals,
    GuardianClaimedUtxo, GuardianUtxoClaim, GuardianUtxoClaimStatus, GuardianUtxoDisagreement,
    NonceSpendInfo, NoncesRequest,
};
use serde::Deserialize;
use serde_json::json;

use crate::federation::gateways::get_federation_gateways;
use crate::federation::guardians::get_federation_health;
use crate::federation::meta::get_federation_meta;
use crate::federation::session::{count_sessions, list_sessions};
use crate::federation::transaction::{
    count_transactions, list_transactions, transaction, transaction_histogram,
};
use crate::util::{config_to_json, get_decoders};
use crate::{federation, AppState};

pub fn get_federations_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_observed_federations))
        .route("/", put(add_observed_federation))
        .route("/totals", get(get_federation_totals))
        // TODO: move to nostr module
        .route("/nostr/rating", put(publish_rating_event))
        .route("/:federation_id", get(get_federation_overview))
        .route(
            "/:federation_id/config",
            get(federation::get_federation_config),
        )
        .route("/:federation_id/meta", get(get_federation_meta))
        .route("/:federation_id/health", get(get_federation_health))
        .route("/:federation_id/transactions", get(list_transactions))
        .route(
            "/:federation_id/transactions/:transaction_id",
            get(transaction),
        )
        .route(
            "/:federation_id/transactions/count",
            get(count_transactions),
        )
        .route(
            "/:federation_id/transactions/histogram",
            get(transaction_histogram),
        )
        .route("/:federation_id/gateways", get(get_federation_gateways))
        .route("/:federation_id/utxos", get(get_federation_utxos))
        .route("/:federation_id/sessions", get(list_sessions))
        .route("/:federation_id/sessions/count", get(count_sessions))
        .route("/:federation_id/backfill", post(backfill_federation))
        .route("/:federation_id/nonces/spend", post(get_nonces_spend_info))
}

pub async fn list_observed_federations(
    State(state): State<AppState>,
) -> crate::error::Result<Json<Vec<FederationSummary>>> {
    Ok(state
        .federation_observer
        .list_federation_summaries()
        .await?
        .into())
}

pub async fn add_observed_federation(
    AuthBearer(auth): AuthBearer,
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> crate::error::Result<Json<FederationId>> {
    state.federation_observer.check_auth(&auth)?;

    let invite: InviteCode = serde_json::from_value(
        body.get("invite")
            .context("Request did not contain invite field")?
            .clone(),
    )
    .context("Invalid invite code")?;
    Ok(state
        .federation_observer
        .add_federation(&invite)
        .await?
        .into())
}

pub(crate) async fn get_federation_config(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<JsonClientConfig>> {
    Ok(config_to_json(
        state
            .federation_observer
            .get_federation(federation_id)
            .await?
            .context("Federation not observed, you might want to try /config/:federation_invite")?
            .config,
    )?
    .into())
}

async fn get_federation_overview(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<serde_json::Value>> {
    let session_count = state
        .federation_observer
        .federation_session_count(federation_id)
        .await?;
    let total_assets_msat = state
        .federation_observer
        .get_federation_assets(federation_id)
        .await?;

    Ok(json!({
        "session_count": session_count,
        "total_assets_msat": total_assets_msat
    })
    .into())
}

async fn get_federation_utxos(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<FederationUtxosResponse>> {
    let utxos = state
        .federation_observer
        .federation_utxos(federation_id)
        .await?;
    let mut guardian_claims = state
        .federation_observer
        .guardian_utxo_claims(federation_id)
        .await?;
    state
        .federation_observer
        .enrich_guardian_claims_onchain(&mut guardian_claims)
        .await;
    let disagreements = guardian_utxo_disagreements(&utxos, &guardian_claims);
    Ok(FederationUtxosResponse {
        observed: utxos,
        guardian_claims,
        disagreements,
    }
    .into())
}

fn guardian_utxo_disagreements(
    observed: &[FederationUtxo],
    guardian_claims: &[GuardianUtxoClaim],
) -> Vec<GuardianUtxoDisagreement> {
    let observed_by_outpoint = observed
        .iter()
        .map(|utxo| (utxo.out_point, utxo))
        .collect::<HashMap<_, _>>();
    let successful_claims = guardian_claims
        .iter()
        .filter(|claim| matches!(claim.status, GuardianUtxoClaimStatus::Ok))
        .collect::<Vec<_>>();

    if successful_claims.is_empty() {
        return if guardian_claims.is_empty() {
            Vec::new()
        } else {
            vec![GuardianUtxoDisagreement {
                out_point: OutPoint::null(),
                description: "no guardian wallet summaries could be fetched".to_owned(),
            }]
        };
    }

    let claimed_by_outpoint = successful_claims
        .iter()
        .flat_map(|claim| {
            claim
                .utxos
                .iter()
                .map(|utxo| (utxo.out_point, (claim.guardian_id, utxo)))
        })
        .fold(
            HashMap::<OutPoint, Vec<(u16, &GuardianClaimedUtxo)>>::new(),
            |mut acc, (out_point, claim)| {
                acc.entry(out_point).or_default().push(claim);
                acc
            },
        );

    let mut disagreements = Vec::new();

    for claim in &successful_claims {
        for utxo in &claim.utxos {
            if let Some(onchain) = &utxo.onchain {
                if onchain.amount != utxo.amount {
                    disagreements.push(GuardianUtxoDisagreement {
                        out_point: utxo.out_point,
                        description: format!(
                            "guardian {} reports {} msat, but resolved Bitcoin output has {} msat",
                            claim.guardian_id, utxo.amount.msats, onchain.amount.msats
                        ),
                    });
                }
            } else if let Some(error) = &utxo.resolution_error {
                disagreements.push(GuardianUtxoDisagreement {
                    out_point: utxo.out_point,
                    description: format!(
                        "could not resolve guardian {} claimed outpoint from Bitcoin data: {error}",
                        claim.guardian_id
                    ),
                });
            }
        }
    }

    for observed_utxo in observed {
        let Some(claims) = claimed_by_outpoint.get(&observed_utxo.out_point) else {
            disagreements.push(GuardianUtxoDisagreement {
                out_point: observed_utxo.out_point,
                description: "observer has UTXO but no successful guardian claims it".to_owned(),
            });
            continue;
        };

        let mismatched_guardians = claims
            .iter()
            .filter(|(_, claim)| claim.amount != observed_utxo.amount)
            .map(|(guardian_id, claim)| {
                format!("guardian {guardian_id} reports {} msat", claim.amount.msats)
            })
            .collect::<Vec<_>>();

        if !mismatched_guardians.is_empty() {
            disagreements.push(GuardianUtxoDisagreement {
                out_point: observed_utxo.out_point,
                description: format!(
                    "observer reports {} msat, but {}",
                    observed_utxo.amount.msats,
                    mismatched_guardians.join(", ")
                ),
            });
        }

        let observed_address = observed_utxo.address.clone().assume_checked().to_string();
        let mismatched_addresses = claims
            .iter()
            .filter_map(|(guardian_id, claim)| {
                claim
                    .onchain
                    .as_ref()
                    .and_then(|onchain| onchain.address.as_ref())
                    .filter(|address| *address != &observed_address)
                    .map(|address| format!("guardian {guardian_id} resolves to address {address}"))
            })
            .collect::<Vec<_>>();

        if !mismatched_addresses.is_empty() {
            disagreements.push(GuardianUtxoDisagreement {
                out_point: observed_utxo.out_point,
                description: format!(
                    "observer reconstructs address {}, but {}",
                    observed_address,
                    mismatched_addresses.join(", ")
                ),
            });
        }
    }

    for (out_point, claims) in &claimed_by_outpoint {
        if !observed_by_outpoint.contains_key(out_point) {
            let guardian_ids = claims
                .iter()
                .map(|(guardian_id, _)| guardian_id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let onchain_hint = claims
                .iter()
                .find_map(|(_, claim)| claim.onchain.as_ref())
                .map(|onchain| {
                    format!(
                        "; resolved script_pubkey: {}; address: {}",
                        onchain.script_pubkey,
                        onchain.address.as_deref().unwrap_or("non-standard")
                    )
                })
                .unwrap_or_default();
            disagreements.push(GuardianUtxoDisagreement {
                out_point: *out_point,
                description: format!(
                    "guardian wallet summary claims UTXO, but observer reconstruction does not; guardians: {guardian_ids}{onchain_hint}"
                ),
            });
        }
    }

    for claim in successful_claims {
        let guardian_outpoints = claim
            .utxos
            .iter()
            .map(|utxo| utxo.out_point)
            .collect::<HashSet<_>>();
        for out_point in claimed_by_outpoint.keys() {
            if !guardian_outpoints.contains(out_point) {
                disagreements.push(GuardianUtxoDisagreement {
                    out_point: *out_point,
                    description: format!(
                        "guardian {} did not claim UTXO claimed by another successful guardian",
                        claim.guardian_id
                    ),
                });
            }
        }
    }

    disagreements
}

async fn get_federation_totals(
    State(state): State<AppState>,
) -> crate::error::Result<Json<FedimintTotals>> {
    Ok(state.federation_observer.totals().await?.into())
}

async fn publish_rating_event(
    State(state): State<AppState>,
    Json(event): Json<nostr_sdk::Event>,
) -> crate::error::Result<()> {
    Ok(state.federation_observer.submit_rating(event).await?)
}

#[derive(Deserialize, Debug)]
struct BackfillParams {
    session_start: Option<i32>,
    session_end: Option<i32>,
}

async fn backfill_federation(
    Path(federation_id): Path<FederationId>,
    AuthBearer(auth): AuthBearer,
    State(state): State<AppState>,
    Json(params): Json<BackfillParams>,
) -> crate::error::Result<()> {
    state.federation_observer.check_auth(&auth)?;

    Ok(state
        .federation_observer
        .backfill_federation(federation_id, params.session_start, params.session_end)
        .await?)
}

fn decoders_from_config(config: &ClientConfig) -> ModuleDecoderRegistry {
    get_decoders(
        config
            .modules
            .iter()
            .map(|(module_instance_id, module_config)| {
                (*module_instance_id, module_config.kind.clone())
            }),
    )
    .with_fallback()
}

fn instance_to_kind(config: &ClientConfig, module_instance_id: ModuleInstanceId) -> String {
    config
        .modules
        .get(&module_instance_id)
        .map(|module_config| module_config.kind.to_string())
        .unwrap_or_else(|| "not-in-config".to_owned())
}

async fn get_nonces_spend_info(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
    Json(request): Json<NoncesRequest>,
) -> crate::error::Result<Json<std::collections::HashMap<String, NonceSpendInfo>>> {
    Ok(state
        .federation_observer
        .get_nonces_spend_info(federation_id, &request.nonces)
        .await?
        .into())
}
