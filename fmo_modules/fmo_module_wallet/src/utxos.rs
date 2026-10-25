//! The UTXO endpoint: the federation's outputs as the observer reconstructed
//! them, judged against what its guardians report and, where they can't
//! vouch, against the chain

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use bitcoin::hashes::Hash;
use bitcoin::{Address, OutPoint, Txid};
use fedimint_api_client::api::FederationApiExt;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fedimint_core::endpoint_constants::STATUS_ENDPOINT;
use fedimint_core::module::ApiRequestErased;
use fedimint_core::Amount;
use fedimint_wallet_common::endpoint_constants::WALLET_SUMMARY_ENDPOINT;
use fedimint_wallet_common::{TxOutputSummary, WalletSummary};
use fmo_api_types::{
    FederationUtxo, FederationUtxosResponse, GuardianClaimedUtxo, GuardianClaimedUtxoState,
    GuardianUtxoClaim, GuardianUtxoClaimStatus, GuardianUtxoDisagreementKind, GuardianUtxoReport,
    UtxoComparisonRow, UtxoComparisonStatus,
};
use fmo_core::module::{federation_api, module_instance_id, query, ApiResult, Services};
use futures::future::join_all;
use futures::StreamExt;
use postgres_from_row::FromRow;
use tracing::debug;

/// Guardian wallet summaries younger than this are reused, so page views don't
/// each send a request to every guardian.
const GUARDIAN_CLAIMS_TTL: Duration = Duration::from_secs(30);

type GuardianClaimsSlot = Arc<tokio::sync::Mutex<Option<(Instant, Vec<GuardianUtxoClaim>)>>>;

/// Unspent and missing results are rechecked after this; spends are final.
const ONCHAIN_CHECK_TTL: Duration = Duration::from_secs(10 * 60);
/// Upper bound on how long a page load waits for chain lookups. Results that
/// arrive in time are cached, so a later load continues where this one stopped.
const ONCHAIN_CHECK_DEADLINE: Duration = Duration::from_secs(8);
const ONCHAIN_CHECK_CONCURRENCY: usize = 8;

/// State of a transaction output according to the blockchain
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnChainOutput {
    /// `amount` is only looked up when the caller asked for it
    Unspent {
        amount: Option<bitcoin::Amount>,
    },
    Spent {
        by: Txid,
    },
    /// The transaction or the output index does not exist
    Missing,
}

/// Shared between requests, so page views don't each ask every guardian and
/// the mempool instance again
#[derive(Default)]
pub(crate) struct UtxoCaches {
    guardian_claims: Mutex<HashMap<FederationId, GuardianClaimsSlot>>,
    onchain: Mutex<HashMap<OutPoint, (Instant, OnChainOutput)>>,
}

/// The endpoint's view of core and the module's caches
struct Utxos<'a> {
    services: &'a Services,
    caches: &'a UtxoCaches,
}

/// `GET /federations/:federation_id/utxos`
pub(crate) fn federation_routes(caches: Arc<UtxoCaches>) -> Router<Services> {
    Router::new().route(
        "/utxos",
        get(move |path, state| get_federation_utxos(caches.clone(), path, state)),
    )
}

async fn get_federation_utxos(
    caches: Arc<UtxoCaches>,
    Path(federation_id): Path<FederationId>,
    State(services): State<Services>,
) -> ApiResult<Json<FederationUtxosResponse>> {
    let utxos = Utxos {
        services: &services,
        caches: &caches,
    };
    let (observed, guardian_claims) = tokio::try_join!(
        utxos.federation_utxos(federation_id),
        utxos.guardian_utxo_claims(federation_id),
    )?;
    let mut comparison = compare_utxos(&observed, &guardian_claims);
    let onchain = utxos
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

impl Utxos<'_> {
    pub(crate) async fn federation_utxos(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Vec<FederationUtxo>> {
        #[derive(Debug, FromRow)]
        struct FederationUtxoRaw {
            on_chain_txid: Vec<u8>,
            on_chain_vout: i32,
            address: String,
            amount_msat: i64,
        }

        query::<FederationUtxoRaw>(
            &self.services.connection().await?,
            // The wallet module's view, read here until the UTXO endpoint
            // moves into the module
            // language=postgresql
            "SELECT on_chain_txid, on_chain_vout, address, amount_msat FROM fmo_wallet.utxos WHERE federation_id = $1 ORDER BY amount_msat DESC",
            &[&federation_id.consensus_encode_to_vec()],
        ).await?.into_iter().map(|utxo| {
            Result::<_, anyhow::Error>::Ok(FederationUtxo {
                address: Address::from_str(&utxo.address)?,
                out_point: OutPoint {
                    txid: Txid::from_slice(&utxo.on_chain_txid)?,
                    vout: utxo.on_chain_vout.try_into()?,
                },
                amount: Amount::from_msats(utxo.amount_msat.try_into()?),
            })
        }).collect()
    }

    pub(crate) async fn guardian_utxo_claims(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Vec<GuardianUtxoClaim>> {
        // Unknown ids from the request path must not get a cache slot each
        self.services.ensure_observed(federation_id).await?;
        let slot = self
            .caches
            .guardian_claims
            .lock()
            .expect("guardian claims cache lock poisoned")
            .entry(federation_id)
            .or_default()
            .clone();
        // Held across the fetch so concurrent misses share one round of requests
        let mut cached = slot.lock().await;
        if let Some((fetched_at, claims)) = cached.as_ref() {
            if fetched_at.elapsed() < GUARDIAN_CLAIMS_TTL {
                return Ok(claims.clone());
            }
        }

        let claims = self.fetch_guardian_utxo_claims(federation_id).await?;
        *cached = Some((Instant::now(), claims.clone()));
        Ok(claims)
    }

    async fn fetch_guardian_utxo_claims(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Vec<GuardianUtxoClaim>> {
        let federation = self
            .services
            .federation(federation_id)
            .await?
            .context("Federation not observed")?;
        let config = federation.config;

        let Some(wallet_module_id) = module_instance_id(&config, "wallet") else {
            return Ok(config
                .global
                .api_endpoints
                .keys()
                .map(|peer_id| GuardianUtxoClaim {
                    guardian_id: peer_id.to_usize() as u16,
                    status: GuardianUtxoClaimStatus::Unavailable,
                    session_count: None,
                    utxos: Vec::new(),
                    error: Some("federation config has no wallet module".to_owned()),
                })
                .collect());
        };

        let api = federation_api(self.services.connectors(), &config.global)?;
        let module_api = api.with_module(wallet_module_id);

        let mut claims = join_all(config.global.api_endpoints.keys().map(|&peer_id| {
            let api = api.clone();
            let module_api = module_api.clone();
            async move {
                let summary = module_api.request_single_peer::<WalletSummary>(
                    WALLET_SUMMARY_ENDPOINT.to_owned(),
                    ApiRequestErased::default(),
                    peer_id,
                );
                let status = api.request_single_peer::<serde_json::Value>(
                    STATUS_ENDPOINT.to_owned(),
                    ApiRequestErased::default(),
                    peer_id,
                );
                // Timed out separately so a slow status call can't discard a
                // wallet summary that did arrive
                let (summary, status) = futures::future::join(
                    tokio::time::timeout(Duration::from_secs(10), summary),
                    tokio::time::timeout(Duration::from_secs(10), status),
                )
                .await;

                let guardian_id = peer_id.to_usize() as u16;
                let (status, session_count, utxos, error) = match summary {
                    Ok(Ok(summary)) => {
                        let session_count = status.ok().and_then(Result::ok).and_then(|status| {
                            status.get("federation")?.get("session_count")?.as_u64()
                        });
                        (
                            GuardianUtxoClaimStatus::Ok,
                            session_count,
                            wallet_summary_claimed_utxos(summary),
                            None,
                        )
                    }
                    Ok(Err(error)) => (
                        GuardianUtxoClaimStatus::Error,
                        None,
                        Vec::new(),
                        Some(error.to_string()),
                    ),
                    Err(_) => (
                        GuardianUtxoClaimStatus::Error,
                        None,
                        Vec::new(),
                        Some("Guardian wallet summary timed out".to_owned()),
                    ),
                };
                GuardianUtxoClaim {
                    guardian_id,
                    status,
                    session_count,
                    utxos,
                    error,
                }
            }
        }))
        .await;
        mark_lagging_guardians(&mut claims, config.global.api_endpoints.len());

        Ok(claims)
    }

    /// Looks up outputs on-chain. Each request says whether the output's amount
    /// is needed too, which costs an extra request. Outputs whose lookup fails
    /// or does not finish within [`ONCHAIN_CHECK_DEADLINE`] are left out.
    pub(crate) async fn onchain_outputs(
        &self,
        requests: Vec<(OutPoint, bool)>,
    ) -> HashMap<OutPoint, OnChainOutput> {
        let mut results = HashMap::new();
        let mut to_fetch = Vec::new();
        {
            let cache = self
                .caches
                .onchain
                .lock()
                .expect("onchain cache lock poisoned");
            for (out_point, with_amount) in requests {
                let cached = cache.get(&out_point).filter(|(checked_at, output)| {
                    let fresh = matches!(output, OnChainOutput::Spent { .. })
                        || checked_at.elapsed() < ONCHAIN_CHECK_TTL;
                    let complete =
                        !with_amount || !matches!(output, OnChainOutput::Unspent { amount: None });
                    fresh && complete
                });
                match cached {
                    Some((_, output)) => {
                        results.insert(out_point, *output);
                    }
                    None => to_fetch.push((out_point, with_amount)),
                }
            }
        }
        if to_fetch.is_empty() {
            return results;
        }

        let mut lookups = futures::stream::iter(to_fetch)
            .map(|(out_point, with_amount)| {
                let client = self.services.esplora();
                async move {
                    let output = fetch_onchain_output(client, out_point, with_amount).await;
                    (out_point, output)
                }
            })
            .buffer_unordered(ONCHAIN_CHECK_CONCURRENCY);
        let _ = tokio::time::timeout(ONCHAIN_CHECK_DEADLINE, async {
            while let Some((out_point, output)) = lookups.next().await {
                match output {
                    Ok(output) => {
                        self.caches
                            .onchain
                            .lock()
                            .expect("onchain cache lock poisoned")
                            .insert(out_point, (Instant::now(), output));
                        results.insert(out_point, output);
                    }
                    Err(error) => debug!("On-chain lookup of {out_point} failed: {error:?}"),
                }
            }
        })
        .await;

        results
    }
}

async fn fetch_onchain_output(
    client: &esplora_client::AsyncClient,
    out_point: OutPoint,
    with_amount: bool,
) -> anyhow::Result<OnChainOutput> {
    let amount = if with_amount {
        let Some(tx) = client.get_tx_info(&out_point.txid).await? else {
            return Ok(OnChainOutput::Missing);
        };
        let Some(output) = tx.vout.get(out_point.vout as usize) else {
            return Ok(OnChainOutput::Missing);
        };
        Some(bitcoin::Amount::from_sat(output.value))
    } else {
        None
    };

    let status = client
        .get_output_status(&out_point.txid, out_point.vout.into())
        .await?
        .context("No output status")?;
    Ok(match status.txid {
        Some(by) if status.spent => OnChainOutput::Spent { by },
        _ => OnChainOutput::Unspent { amount },
    })
}

/// Marks responding guardians whose session count is behind the federation as
/// lagging: their wallet summary predates outputs the others already know.
///
/// The reference is the `f+1`-th highest reported count, so up to `f` faulty
/// guardians claiming an inflated count cannot push honest ones out of the
/// comparison. Guardians that did not report a count stay in.
fn mark_lagging_guardians(claims: &mut [GuardianUtxoClaim], num_guardians: usize) {
    let max_faulty = num_guardians.saturating_sub(1) / 3;
    let mut session_counts = claims
        .iter()
        .filter(|claim| matches!(claim.status, GuardianUtxoClaimStatus::Ok))
        .filter_map(|claim| claim.session_count)
        .collect::<Vec<_>>();
    session_counts.sort_unstable_by(|a, b| b.cmp(a));
    let Some(&reference) = session_counts.get(max_faulty) else {
        return;
    };

    for claim in claims {
        if matches!(claim.status, GuardianUtxoClaimStatus::Ok)
            && claim
                .session_count
                .is_some_and(|session_count| session_count < reference)
        {
            claim.status = GuardianUtxoClaimStatus::Lagging;
        }
    }
}

fn wallet_summary_claimed_utxos(summary: WalletSummary) -> Vec<GuardianClaimedUtxo> {
    let mut utxos = Vec::new();
    append_claimed_utxos(
        &mut utxos,
        summary.spendable_utxos,
        GuardianClaimedUtxoState::Spendable,
    );
    append_claimed_utxos(
        &mut utxos,
        summary.unsigned_peg_out_txos,
        GuardianClaimedUtxoState::UnsignedPegOut,
    );
    append_claimed_utxos(
        &mut utxos,
        summary.unsigned_change_utxos,
        GuardianClaimedUtxoState::UnsignedChange,
    );
    append_claimed_utxos(
        &mut utxos,
        summary.unconfirmed_peg_out_txos,
        GuardianClaimedUtxoState::UnconfirmedPegOut,
    );
    append_claimed_utxos(
        &mut utxos,
        summary.unconfirmed_change_utxos,
        GuardianClaimedUtxoState::UnconfirmedChange,
    );
    utxos
}

fn append_claimed_utxos(
    utxos: &mut Vec<GuardianClaimedUtxo>,
    txos: Vec<TxOutputSummary>,
    state: GuardianClaimedUtxoState,
) {
    utxos.extend(txos.into_iter().map(|txo| GuardianClaimedUtxo {
        out_point: txo.outpoint,
        amount: Amount::from_sats(txo.amount.to_sat()),
        state,
    }));
}
