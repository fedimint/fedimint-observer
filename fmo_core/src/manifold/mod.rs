//! Read-only Manifold data the observer can establish itself: which Manifold
//! Guardian (MG) operates each guardian seat, from the federation's own
//! consensus meta. Discovery data (Nostr advertisements), trust credentials
//! and MG telemetry are not collected here.

mod api;
mod collector;
mod directory;
pub mod discovery;
pub mod seat_bindings;

pub(crate) use api::routes;
use axum::extract::{Path, State};
use axum::Json;
use bitcoin::hashes::{sha256, Hash, HashEngine};
use chrono::{DateTime, Utc};
pub(crate) use collector::{check_trusted_issuers, configured_relays};
use fedimint_core::config::FederationId;
use fmo_api_types::{ManifoldSeatBinding, ManifoldSeatBindingsReport, SeatBindingsStatus};
use serde_json::{json, Map, Value};

use self::seat_bindings::{verify_for_federation, FederationSeats, SEAT_BINDINGS_META_KEY};
use crate::error::ClientError;
use crate::module::Services;

/// Observation kind holding the raw directory string: `{"value": "<field>"}`,
/// or `{"value": null}` when the consensus meta has no such field
pub(crate) const SEAT_BINDINGS_OBSERVATION: &str = "manifold-seat-bindings";
const MG_NAME_DOMAIN: &[u8] = b"fman-name/v1\0";

/// Manifold's deterministic display name for an MG key (`MgName` in
/// fedibtc/manifold): two words picked by a domain-separated hash of the
/// x-only key. Presentation only; names can collide.
pub fn mg_display_name(xonly_key: &[u8; 32]) -> String {
    let mut engine = sha256::Hash::engine();
    engine.input(MG_NAME_DOMAIN);
    engine.input(xonly_key);
    let digest = sha256::Hash::from_engine(engine).to_byte_array();
    let words = petname::Petnames::medium();
    let pick = |bytes: &[u8], count: usize| {
        let value = u64::from_be_bytes(bytes.try_into().expect("eight digest bytes"));
        (value % count as u64) as usize
    };
    format!(
        "{}-{}",
        words.adjectives[pick(&digest[..8], words.adjectives.len())],
        words.nouns[pick(&digest[8..16], words.nouns.len())]
    )
}

/// Records the directory field of a freshly read consensus meta. Called by
/// the metadata refresh; failing here never fails that refresh.
pub(crate) async fn save_seat_bindings_observation(
    services: &Services,
    federation_id: FederationId,
    consensus_meta: Option<&Map<String, Value>>,
) {
    // Only strings are meta values; anything else is no directory
    let value = consensus_meta
        .and_then(|meta| meta.get(SEAT_BINDINGS_META_KEY))
        .and_then(Value::as_str);
    if let Err(error) = services
        .save_observation(
            federation_id,
            SEAT_BINDINGS_OBSERVATION,
            &json!({ "value": value }),
        )
        .await
    {
        tracing::warn!(%error, "Could not persist Manifold seat bindings");
    }
}

/// `GET /federations/:id/manifold/seat-bindings`: verifies the stored
/// directory against the stored config. No guardian is asked here; the work
/// is bounded by the directory's 64 binding cap.
pub(crate) async fn get_seat_bindings(
    Path(federation_id): Path<FederationId>,
    State(state): State<crate::AppState>,
) -> crate::error::Result<Json<ManifoldSeatBindingsReport>> {
    let observer = &state.federation_observer;
    let federation = observer
        .get_federation(federation_id)
        .await?
        .ok_or_else(|| {
            ClientError::not_found(format!("Federation {federation_id} is not observed"))
        })?;
    let has_meta_module = federation
        .config
        .modules
        .values()
        .any(|module| module.kind().as_str() == "meta");
    let observation = Services::new(observer.clone())
        .observation(federation_id, SEAT_BINDINGS_OBSERVATION)
        .await?;

    let report = |observed_at: Option<DateTime<Utc>>,
                  status,
                  error: Option<String>,
                  bindings: Vec<ManifoldSeatBinding>| {
        let distinct_guardians = bindings
            .iter()
            .map(|binding| binding.fman_pubkey.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len() as u32;
        Json(ManifoldSeatBindingsReport {
            has_meta_module,
            observed_at,
            status,
            error,
            bindings,
            distinct_guardians,
        })
    };

    let Some((data, observed_at)) = observation.filter(|_| has_meta_module) else {
        return Ok(report(
            None,
            SeatBindingsStatus::NotObserved,
            None,
            Vec::new(),
        ));
    };
    let Some(value) = data.get("value").and_then(Value::as_str) else {
        return Ok(report(
            Some(observed_at),
            SeatBindingsStatus::NotPublished,
            None,
            Vec::new(),
        ));
    };

    let verified = FederationSeats::from_config(&federation.config)
        .and_then(|seats| verify_for_federation(value, &seats));
    Ok(match verified {
        Ok(bindings) => report(
            Some(observed_at),
            SeatBindingsStatus::Verified,
            None,
            bindings
                .into_iter()
                .map(|binding| {
                    let key: [u8; 32] = hex::decode(&binding.fman_pubkey)
                        .ok()
                        .and_then(|bytes| bytes.try_into().ok())
                        .expect("verified keys are 32 bytes of hex");
                    ManifoldSeatBinding {
                        peer_id: binding.peer_id,
                        display_name: mg_display_name(&key),
                        fman_pubkey: binding.fman_pubkey,
                        guardian_identity: binding.guardian_identity,
                        guardian_fee_pubkey: binding.guardian_fee_pubkey,
                        issued_at: binding.issued_at,
                    }
                })
                .collect(),
        ),
        Err(error) => report(
            Some(observed_at),
            SeatBindingsStatus::Invalid,
            Some(error.to_string()),
            Vec::new(),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mg_names_match_manifolds_golden_vector() {
        let key: [u8; 32] =
            hex::decode("9384d5bef4d90f491d316a2b786cd1477ea1dde563ae10d42f8e3e270f249b85")
                .unwrap()
                .try_into()
                .unwrap();
        assert_eq!(mg_display_name(&key), "blissful-chiffchaff");
    }
}
