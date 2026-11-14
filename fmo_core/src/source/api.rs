//! The HTTP side of sources: the push endpoint external services write
//! through, and the public read endpoint.

use std::sync::{Arc, LazyLock};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::routing::{get, put};
use axum::{Json, Router};
use axum_auth::AuthBearer;
use chrono::{DateTime, Utc};
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fmo_api_types::SourceObservation;
use serde::Deserialize;
use serde_json::Value;

use super::{
    ObservationSink, PushToken, Recorded, RegisteredSource, SourceRegistry, MAX_OBSERVATION_BYTES,
};
use crate::error::{AppError, ClientError};
use crate::module::Services;
use crate::AppState;

/// The largest push request body: the largest observation, its envelope and
/// some whitespace
const MAX_PUSH_BODY: usize = MAX_OBSERVATION_BYTES + 4 * 1024;

/// `PUT /sources/:source/federations/:federation_id/:kind`
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/sources/:source/federations/:federation_id/:kind",
            put(push_observation),
        )
        .layer(DefaultBodyLimit::max(MAX_PUSH_BODY))
}

/// `GET /federations/:federation_id/sources`
pub(crate) fn federations_routes() -> Router<AppState> {
    Router::new().route("/:federation_id/sources", get(federation_observations))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PushBody {
    /// When the source made the observation, RFC 3339
    observed_at: DateTime<Utc>,
    data: Value,
}

/// Stores an observation an external service pushes. The token is checked
/// before anything else about the request, so callers without one learn
/// nothing about sources, kinds or federations. Answers `{"stored": true}`,
/// or `false` when a newer observation was already stored.
async fn push_observation(
    Path((source_id, federation_id, kind)): Path<(String, String, String)>,
    AuthBearer(token): AuthBearer,
    State(state): State<AppState>,
    body: Bytes,
) -> crate::error::Result<Json<Value>> {
    let observer = &state.federation_observer;
    let Some(source) = authenticate(observer.sources(), &source_id, &token) else {
        return Err(AppError::unauthorized());
    };

    let federation_id = federation_id
        .parse::<FederationId>()
        .map_err(|_| ClientError::bad_request("Invalid federation id"))?;
    let body = serde_json::from_slice::<PushBody>(&body)
        .map_err(|error| ClientError::bad_request(format!("Invalid observation: {error}")))?;

    let sink = ObservationSink::new(source.clone(), Services::new(observer.clone()));
    let recorded = sink
        .try_record(federation_id, &kind, body.observed_at, &body.data)
        .await?;
    Ok(Json(
        serde_json::json!({ "stored": recorded == Recorded::Stored }),
    ))
}

/// The source a push is for, if the token is its push token. Unknown sources
/// and sources that don't accept pushes verify against a token nobody holds,
/// so their answer takes as long as a wrong token's and reads the same.
pub(super) fn authenticate(
    sources: &SourceRegistry,
    source_id: &str,
    token: &str,
) -> Option<Arc<RegisteredSource>> {
    static NOBODY: LazyLock<PushToken> =
        LazyLock::new(|| PushToken::new(&"\u{0}".repeat(32)).expect("valid length"));
    let source = sources.get(source_id);
    let expected = source
        .and_then(|source| source.source.push_token())
        .unwrap_or(&NOBODY);
    // Verify first, whatever the source, then decide
    let verified = expected.verify(token);
    source
        .filter(|source| verified && source.source.push_token().is_some())
        .cloned()
}

/// The newest public observations about an observed federation, of the
/// sources and kinds this build declares, within their retention
async fn federation_observations(
    Path(federation_id): Path<FederationId>,
    State(state): State<AppState>,
) -> crate::error::Result<Json<Vec<SourceObservation>>> {
    let observer = &state.federation_observer;
    observer.ensure_observed(federation_id).await?;
    let sources = observer.sources();
    let now = Utc::now();
    let rows = observer
        .connection()
        .await?
        .query(
            "SELECT source, kind, data, observed_at FROM source_observations
             WHERE federation_id = $1 ORDER BY source, kind",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;
    Ok(Json(
        rows.into_iter()
            .filter_map(|row| {
                let source: String = row.get(0);
                let kind: String = row.get(1);
                let observed_at: DateTime<Utc> = row.get(3);
                let declared = sources.kind(&source, &kind)?;
                (declared.is_public() && !declared.expired(observed_at, now)).then(|| {
                    SourceObservation {
                        source,
                        kind,
                        observed_at,
                        data: row.get(2),
                    }
                })
            })
            .collect(),
    ))
}
