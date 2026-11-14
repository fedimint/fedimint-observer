//! Observed federations. Files by layer:
//!
//! - collection: `ingest.rs` (sessions, block times, backfill), `guardians.rs`
//!   (health polling and retention), `metadata.rs`
//! - processing: `projector.rs` (module tables, read-model rebuilds)
//! - presentation: this file's routes, and the read queries of `summary.rs`,
//!   `session.rs`, `transaction.rs`, `meta.rs` and `guardians.rs`
//! - wiring: `observer.rs`, which owns the clients, pools and tasks
//!
//! See `ARCHITECTURE.md` at the repository root.

mod guardians;
mod ingest;
mod meta;
mod metadata;
pub mod observer;
mod projector;
mod session;
mod summary;
mod transaction;

use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use axum_auth::AuthBearer;
use fedimint_core::config::{ClientConfig, FederationId, JsonClientConfig};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::invite_code::InviteCode;
use fmo_api_types::{FederationOverview, FederationSummary, FedimintTotals};
use serde::Deserialize;

use crate::error::ClientError;
use crate::federation::guardians::get_federation_health;
use crate::federation::meta::get_federation_meta;
use crate::federation::observer::FederationObserver;
use crate::federation::session::{count_sessions, list_sessions};
use crate::federation::transaction::{
    count_transactions, list_transactions, transaction, transaction_histogram,
};
use crate::util::config_to_json;
use crate::{federation, AppState};

pub fn get_federations_routes() -> Router<AppState> {
    Router::new()
        .merge(crate::recommend::federations_routes())
        .merge(crate::source::federations_routes())
        .route("/", get(list_observed_federations))
        .route("/", put(add_observed_federation))
        .route("/totals", get(get_federation_totals))
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
        .route("/:federation_id/sessions", get(list_sessions))
        .route("/:federation_id/sessions/count", get(count_sessions))
        .route("/:federation_id/backfill", post(backfill_federation))
        .route(
            "/:federation_id/modules/:kind/reset",
            post(reset_module_cursor),
        )
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

    let invite = body
        .get("invite")
        .ok_or_else(|| ClientError::bad_request("Request did not contain invite field"))?;
    let invite = serde_json::from_value::<InviteCode>(invite.clone())
        .map_err(|_| ClientError::bad_request("Invalid invite code"))?;
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
    Ok(
        observed_config_json(&state.federation_observer, federation_id)
            .await?
            .into(),
    )
}

/// The config of an observed federation, decoded with this build's modules
pub(crate) async fn observed_config_json(
    observer: &FederationObserver,
    federation_id: FederationId,
) -> anyhow::Result<JsonClientConfig> {
    let config = observer.observed_federation(federation_id).await?.config;
    config_to_json(config, observer.modules())
}

async fn get_federation_overview(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<FederationOverview>> {
    let observer = &state.federation_observer;
    observer.ensure_observed(federation_id).await?;
    Ok(FederationOverview {
        session_count: observer.federation_session_count(federation_id).await?,
        total_assets_msat: observer.get_federation_assets(federation_id).await?,
        module_sessions: observer.module_positions(federation_id).await?,
    }
    .into())
}

async fn get_federation_totals(
    State(state): State<AppState>,
) -> crate::error::Result<Json<FedimintTotals>> {
    Ok(state.federation_observer.totals().await?.into())
}

#[derive(Deserialize, Debug)]
struct BackfillParams {
    session_start: Option<i32>,
    session_end: Option<i32>,
}

/// Admin: reprocesses stored sessions in the background, answering 202
/// Accepted with the sessions it covers once it has started
async fn backfill_federation(
    Path(federation_id): Path<FederationId>,
    AuthBearer(auth): AuthBearer,
    State(state): State<AppState>,
    Json(params): Json<BackfillParams>,
) -> crate::error::Result<(axum::http::StatusCode, Json<serde_json::Value>)> {
    state.federation_observer.check_auth(&auth)?;

    let (session_start, session_end) = state
        .federation_observer
        .start_backfill(federation_id, params.session_start, params.session_end)
        .await?;
    Ok((
        axum::http::StatusCode::ACCEPTED,
        Json(serde_json::json!({ "session_start": session_start, "session_end": session_end })),
    ))
}

#[derive(Deserialize, Debug)]
struct ResetModuleParams {
    /// The session the module processes next; 0 replays everything
    #[serde(default)]
    session_start: u64,
    /// Delete what the module derived from the federation first, so the replay
    /// rebuilds it; only from session 0
    #[serde(default)]
    clean: bool,
}

/// Admin: makes a module process a federation's stored sessions again from
/// `session_start`, by moving its cursor
async fn reset_module_cursor(
    Path((federation_id, kind)): Path<(FederationId, String)>,
    AuthBearer(auth): AuthBearer,
    State(state): State<AppState>,
    Json(params): Json<ResetModuleParams>,
) -> crate::error::Result<()> {
    state.federation_observer.check_auth(&auth)?;

    Ok(state
        .federation_observer
        .worker()
        .reset_module_cursor(federation_id, &kind, params.session_start, params.clean)
        .await?)
}

fn instance_to_kind(config: &ClientConfig, module_instance_id: ModuleInstanceId) -> String {
    config
        .modules
        .get(&module_instance_id)
        .map(|module_config| module_config.kind.to_string())
        .unwrap_or_else(|| "not-in-config".to_owned())
}
