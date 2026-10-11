pub mod db;
pub(crate) mod gateways;
mod guardians;
mod meta;
pub(crate) mod nostr;
pub mod observer;
mod session;
mod transaction;

use std::collections::{BTreeMap, BTreeSet, HashMap};

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
use fedimint_core::Amount;
use fmo_api_types::{
    FederationSummary, FederationUtxo, FederationUtxosResponse, FedimintTotals,
    GuardianClaimedUtxo, GuardianClaimedUtxoState, GuardianUtxoClaim, GuardianUtxoClaimStatus,
    GuardianUtxoDisagreementKind, GuardianUtxoReport, NonceSpendInfo, NoncesRequest,
    UtxoComparisonRow, UtxoComparisonStatus,
};
use serde::Deserialize;
use serde_json::json;

use crate::federation::gateways::get_federation_gateway_overview;
use crate::federation::guardians::get_federation_health;
use crate::federation::meta::get_federation_meta;
use crate::federation::observer::OnChainOutput;
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
        .route(
            "/:federation_id/gateways/overview",
            get(get_federation_gateway_overview),
        )
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
    let observer = &state.federation_observer;
    let (observed, guardian_claims) = tokio::try_join!(
        observer.federation_utxos(federation_id),
        observer.guardian_utxo_claims(federation_id),
    )?;
    let mut comparison = compare_utxos(&observed, &guardian_claims);
    let onchain = observer
        .onchain_outputs(comparison.onchain_check_requests())
        .await;
    comparison.apply_onchain_evidence(&onchain);
    Ok(comparison.into_response().into())
}

/// Every output judged against what a threshold of guardians agrees on, and
/// how each guardian deviates from that agreement
struct UtxoComparison {
    threshold: usize,
    any_responding: bool,
    rows: Vec<UtxoComparisonRow>,
    guardians: Vec<GuardianUtxoReport>,
}

/// What one responding guardian says about one output
enum GuardianView {
    /// Lists it as held by the federation; `None` if it lists several amounts
    Holds(Option<Amount>),
    /// Lists it only as part of a peg-out still in flight, so it neither
    /// confirms nor disputes that the federation holds it
    InFlight,
    Absent,
}

/// Whether a guardian listing an output in this state says the federation
/// holds it. Guardians keep peg-out change in `UnconfirmedChange` until the
/// finality delay has passed, while the observer records it at threshold
/// signatures.
fn is_held(state: GuardianClaimedUtxoState) -> bool {
    matches!(
        state,
        GuardianClaimedUtxoState::Spendable | GuardianClaimedUtxoState::UnconfirmedChange
    )
}

fn compare_utxos(
    observed: &[FederationUtxo],
    guardian_claims: &[GuardianUtxoClaim],
) -> UtxoComparison {
    // Fedimint tolerates f = (n - 1) / 3 faulty guardians and acts on what
    // n - f of them agree on, so that agreement is what honest guardians hold
    let num_guardians = guardian_claims.len();
    let threshold = num_guardians - num_guardians.saturating_sub(1) / 3;
    let responding = guardian_claims
        .iter()
        .filter(|claim| matches!(claim.status, GuardianUtxoClaimStatus::Ok))
        .map(|claim| claim.guardian_id)
        .collect::<Vec<_>>();

    let mut outputs = BTreeMap::<OutPoint, OutputReports>::new();
    for utxo in observed {
        outputs.entry(utxo.out_point).or_default().observed = Some(utxo);
    }
    for claim in guardian_claims
        .iter()
        .filter(|claim| matches!(claim.status, GuardianUtxoClaimStatus::Ok))
    {
        for utxo in &claim.utxos {
            outputs
                .entry(utxo.out_point)
                .or_default()
                .listed
                .entry(claim.guardian_id)
                .or_default()
                .push(utxo);
        }
    }

    let mut findings = BTreeMap::<u16, GuardianFindings>::new();
    let rows = outputs
        .into_iter()
        .map(|(out_point, reports)| {
            judge_output(
                out_point,
                &reports,
                &responding,
                num_guardians,
                threshold,
                &mut findings,
            )
        })
        .collect();

    UtxoComparison {
        threshold,
        any_responding: !responding.is_empty(),
        rows,
        guardians: guardian_claims
            .iter()
            .map(|claim| {
                let findings = findings
                    .get(&claim.guardian_id)
                    .copied()
                    .unwrap_or_default();
                GuardianUtxoReport {
                    guardian_id: claim.guardian_id,
                    status: claim.status,
                    session_count: claim.session_count,
                    error: claim.error.clone(),
                    missing_outputs: findings.missing_outputs,
                    extra_outputs: findings.extra_outputs,
                    wrong_amounts: findings.wrong_amounts,
                }
            })
            .collect(),
    }
}

#[derive(Default)]
struct OutputReports<'a> {
    observed: Option<&'a FederationUtxo>,
    /// Entries of responding guardians, usually one each
    listed: BTreeMap<u16, Vec<&'a GuardianClaimedUtxo>>,
}

#[derive(Default, Clone, Copy)]
struct GuardianFindings {
    missing_outputs: u32,
    extra_outputs: u32,
    wrong_amounts: u32,
}

fn judge_output(
    out_point: OutPoint,
    reports: &OutputReports,
    responding: &[u16],
    num_guardians: usize,
    threshold: usize,
    findings: &mut BTreeMap<u16, GuardianFindings>,
) -> UtxoComparisonRow {
    let views = responding
        .iter()
        .map(|guardian_id| {
            let view = match reports.listed.get(guardian_id) {
                None => GuardianView::Absent,
                Some(entries) => {
                    let amounts = entries
                        .iter()
                        .filter(|utxo| is_held(utxo.state))
                        .map(|utxo| utxo.amount)
                        .collect::<BTreeSet<_>>();
                    match amounts.len() {
                        0 => GuardianView::InFlight,
                        1 => GuardianView::Holds(amounts.first().copied()),
                        _ => GuardianView::Holds(None),
                    }
                }
            };
            (*guardian_id, view)
        })
        .collect::<Vec<_>>();
    let holders = views
        .iter()
        .filter_map(|(guardian_id, view)| match view {
            GuardianView::Holds(amount) => Some((*guardian_id, *amount)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let absent = views
        .iter()
        .filter(|(_, view)| matches!(view, GuardianView::Absent))
        .map(|(guardian_id, _)| *guardian_id)
        .collect::<Vec<_>>();
    let pending_states = reports
        .listed
        .values()
        .flatten()
        .filter_map(|utxo| pending_state_label(utxo.state))
        .collect::<BTreeSet<_>>();

    let mut row = UtxoComparisonRow {
        out_point,
        amount: reports
            .observed
            .map(|utxo| utxo.amount)
            .or_else(|| holders.iter().find_map(|(_, amount)| *amount))
            .or_else(|| {
                reports
                    .listed
                    .values()
                    .flatten()
                    .next()
                    .map(|utxo| utxo.amount)
            })
            .unwrap_or(Amount::from_msats(0)),
        address: reports.observed.map(|utxo| utxo.address.clone()),
        // Shown per guardian, preferring the entry that says it is held
        guardian_states: reports
            .listed
            .iter()
            .filter_map(|(guardian_id, entries)| {
                let entry = entries
                    .iter()
                    .find(|utxo| is_held(utxo.state))
                    .or(entries.first())?;
                Some((*guardian_id, entry.state))
            })
            .collect(),
        status: UtxoComparisonStatus::Pending,
        disagreement: None,
        detail: None,
    };
    if holders.len() >= threshold && !responding.is_empty() {
        for guardian_id in &absent {
            findings.entry(*guardian_id).or_default().missing_outputs += 1;
        }
        let mut votes = BTreeMap::<Amount, usize>::new();
        for amount in holders.iter().filter_map(|(_, amount)| *amount) {
            *votes.entry(amount).or_default() += 1;
        }
        let Some(agreed) = votes
            .into_iter()
            .find_map(|(amount, count)| (count >= threshold).then_some(amount))
        else {
            mark(
                &mut row,
                UtxoComparisonStatus::Mismatch,
                Some(GuardianUtxoDisagreementKind::EvidenceMismatch),
                format!(
                    "Guardians list this output but cannot agree on its amount: {}",
                    amount_reports(reports.observed, &holders)
                ),
            );
            return row;
        };

        let wrong = holders
            .iter()
            .filter(|(_, amount)| *amount != Some(agreed))
            .map(|(guardian_id, _)| *guardian_id)
            .collect::<Vec<_>>();
        for guardian_id in &wrong {
            findings.entry(*guardian_id).or_default().wrong_amounts += 1;
        }
        let wrong_note = (!wrong.is_empty()).then(|| {
            format!(
                "{} a different amount.",
                capitalized(&guardians_verb(&wrong, "reports", "report"))
            )
        });

        match reports.observed {
            Some(utxo) if utxo.amount != agreed => mark(
                &mut row,
                UtxoComparisonStatus::Mismatch,
                Some(GuardianUtxoDisagreementKind::EvidenceMismatch),
                format!(
                    "Observer history reports {} msat, but guardians agree on {} msat",
                    utxo.amount.msats, agreed.msats
                ),
            ),
            Some(_) => {
                let final_holders = holders
                    .iter()
                    .filter(|(guardian_id, _)| {
                        reports.listed[guardian_id]
                            .iter()
                            .all(|utxo| utxo.state == GuardianClaimedUtxoState::Spendable)
                    })
                    .count();
                if final_holders >= threshold {
                    row.status = UtxoComparisonStatus::Verified;
                    row.detail = wrong_note;
                } else {
                    row.detail = Some(
                        [Some(still_pending(&pending_states)), wrong_note]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" "),
                    );
                }
            }
            None => {
                row.amount = agreed;
                mark(
                    &mut row,
                    UtxoComparisonStatus::Pending,
                    Some(GuardianUtxoDisagreementKind::ObserverDifference),
                    "Guardians agree the federation holds this output, but observer history does not have it yet".to_owned(),
                );
            }
        }
    } else if absent.len() >= threshold {
        for (guardian_id, _) in &holders {
            findings.entry(*guardian_id).or_default().extra_outputs += 1;
        }
        let holder_ids = holders
            .iter()
            .map(|(guardian_id, _)| *guardian_id)
            .collect::<Vec<_>>();
        if reports.observed.is_some() {
            // Guardians stop listing an output once it is an input of a peg-out,
            // while the observer only records the spend at threshold signatures
            let still_listed = if holder_ids.is_empty() {
                String::new()
            } else {
                format!(
                    " {} it.",
                    capitalized(&guardians_verb(&holder_ids, "still lists", "still list"))
                )
            };
            mark(
                &mut row,
                UtxoComparisonStatus::Pending,
                Some(GuardianUtxoDisagreementKind::ObserverDifference),
                format!("Guardians agree the federation no longer holds this output, but observer history still lists it; it may be an input of a peg-out still collecting signatures.{still_listed}"),
            );
        } else if !holder_ids.is_empty() {
            mark(
                &mut row,
                UtxoComparisonStatus::Mismatch,
                Some(GuardianUtxoDisagreementKind::InventoryDifference),
                format!(
                    "Only {} this output; the other guardians agree the federation does not hold it",
                    guardians_verb(&holder_ids, "lists", "list")
                ),
            );
        } else if !pending_states.is_empty() {
            row.detail = Some(still_pending(&pending_states));
        }
    } else if !holders.is_empty() && !absent.is_empty() {
        let holder_ids = holders
            .iter()
            .map(|(guardian_id, _)| *guardian_id)
            .collect::<Vec<_>>();
        let detail = format!(
            "Guardians are split: {} it, {} not; {threshold} of {num_guardians} must agree",
            guardians_verb(&holder_ids, "lists", "list"),
            guardians_verb(&absent, "does", "do"),
        );
        if responding.len() >= threshold {
            // More guardians deviate than the federation can tolerate
            mark(
                &mut row,
                UtxoComparisonStatus::Mismatch,
                Some(GuardianUtxoDisagreementKind::InventoryDifference),
                detail,
            );
        } else {
            row.detail = Some(detail);
        }
    } else if !pending_states.is_empty() {
        row.detail = Some(still_pending(&pending_states));
    }
    // Anything else stays pending without detail: too few guardians responded
    // to reach the threshold, which the response's guardian list shows

    row
}

fn mark(
    row: &mut UtxoComparisonRow,
    status: UtxoComparisonStatus,
    kind: Option<GuardianUtxoDisagreementKind>,
    detail: String,
) {
    row.status = status;
    row.disagreement = kind;
    row.detail = Some(detail);
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn pending_state_label(state: GuardianClaimedUtxoState) -> Option<&'static str> {
    match state {
        GuardianClaimedUtxoState::Spendable => None,
        GuardianClaimedUtxoState::UnsignedPegOut => Some("unsigned peg out"),
        GuardianClaimedUtxoState::UnsignedChange => Some("unsigned change"),
        GuardianClaimedUtxoState::UnconfirmedPegOut => Some("unconfirmed peg out"),
        GuardianClaimedUtxoState::UnconfirmedChange => Some("unconfirmed change"),
    }
}

fn still_pending(states: &BTreeSet<&str>) -> String {
    format!(
        "This output is still pending: {}.",
        states.iter().copied().collect::<Vec<_>>().join(", ")
    )
}

/// "Guardian 1 reports" or "Guardians 0, 2 report"
fn guardians_verb(guardian_ids: &[u16], singular: &str, plural: &str) -> String {
    let ids = guardian_ids
        .iter()
        .map(|guardian_id| guardian_id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if guardian_ids.len() == 1 {
        format!("guardian {ids} {singular}")
    } else {
        format!("guardians {ids} {plural}")
    }
}

fn amount_reports(observed: Option<&FederationUtxo>, holders: &[(u16, Option<Amount>)]) -> String {
    observed
        .map(|utxo| format!("observer reports {} msat", utxo.amount.msats))
        .into_iter()
        .chain(holders.iter().map(|(guardian_id, amount)| match amount {
            Some(amount) => format!("guardian {guardian_id} reports {} msat", amount.msats),
            None => format!("guardian {guardian_id} lists several amounts"),
        }))
        .collect::<Vec<_>>()
        .join(", ")
}

impl UtxoComparison {
    /// Outputs guardians cannot vouch for, to be looked up on-chain, and
    /// whether their amount is needed. Without any responding guardian that is
    /// every output the observer holds; otherwise only outputs where the
    /// observer differs from the guardians, since chain lookups are slow.
    fn onchain_check_requests(&self) -> Vec<(OutPoint, bool)> {
        self.rows
            .iter()
            .filter_map(|row| {
                let observed = row.address.is_some();
                if !self.any_responding {
                    return observed.then_some((row.out_point, false));
                }
                if row.disagreement != Some(GuardianUtxoDisagreementKind::ObserverDifference) {
                    return None;
                }
                if observed {
                    return Some((row.out_point, false));
                }
                // Change may not have been broadcast yet, so only outputs every
                // guardian considers final must exist on-chain
                row.guardian_states
                    .values()
                    .all(|state| *state == GuardianClaimedUtxoState::Spendable)
                    .then_some((row.out_point, true))
            })
            .collect()
    }

    /// Uses on-chain state to settle outputs guardians could not confirm. The
    /// chain is authoritative: an output it shows as spent, missing or holding
    /// a different amount than reported becomes an [`OnChainConflict`].
    ///
    /// [`OnChainConflict`]: GuardianUtxoDisagreementKind::OnChainConflict
    fn apply_onchain_evidence(&mut self, onchain: &HashMap<OutPoint, OnChainOutput>) {
        for row in &mut self.rows {
            let Some(&output) = onchain.get(&row.out_point) else {
                continue;
            };
            let conflict = Some(GuardianUtxoDisagreementKind::OnChainConflict);

            if !self.any_responding {
                // Nobody else can confirm the observer's history, e.g. because
                // the federation is offline, so a spend it missed goes unnoticed
                if let OnChainOutput::Spent { by } = output {
                    mark(row, UtxoComparisonStatus::Mismatch, conflict, format!(
                        "Already spent on-chain by transaction {by}, but observer history still lists it and no guardian responded"
                    ));
                }
                continue;
            }

            if row.address.is_some() {
                // Guardians already dropped it; the chain tells whether the
                // spend happened or a peg-out is still collecting signatures
                if let OnChainOutput::Spent { by } = output {
                    row.detail = Some(format!(
                        "Spent on-chain by transaction {by}; observer history has not caught up yet"
                    ));
                }
                continue;
            }

            match output {
                OnChainOutput::Missing => mark(row, UtxoComparisonStatus::Mismatch, conflict,
                    "Guardians agree the federation holds this output, but it does not exist on-chain".to_owned(),
                ),
                OnChainOutput::Spent { by } => mark(row, UtxoComparisonStatus::Mismatch, conflict, format!(
                    "Guardians agree the federation holds this output, but it was already spent on-chain by transaction {by}"
                )),
                OnChainOutput::Unspent {
                    amount: Some(amount),
                } if Amount::from_sats(amount.to_sat()) != row.amount => mark(row, UtxoComparisonStatus::Mismatch, conflict, format!(
                    "Guardians agree on {} msat, but the output holds {} msat on-chain",
                    row.amount.msats,
                    Amount::from_sats(amount.to_sat()).msats
                )),
                OnChainOutput::Unspent { amount: Some(_) } => {
                    row.detail = Some("Guardians agree the federation holds this output and it is unspent on-chain, but observer history does not have it yet".to_owned());
                }
                OnChainOutput::Unspent { amount: None } => {}
            }
        }
    }

    fn into_response(mut self) -> FederationUtxosResponse {
        self.rows.sort_by(|left, right| {
            left.status
                .cmp(&right.status)
                .then(right.amount.cmp(&left.amount))
        });
        FederationUtxosResponse {
            threshold: self.threshold,
            guardians: self.guardians,
            utxos: self.rows,
        }
    }
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
