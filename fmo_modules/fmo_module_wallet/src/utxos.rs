//! The UTXO endpoint: the federation's outputs as the observer reconstructed
//! them, judged against what its guardians report and, where they can't
//! vouch, against the chain

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use bitcoin::hashes::Hash;
use bitcoin::{Network, OutPoint, Txid};
use fedimint_api_client::api::FederationApiExt;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::ModuleInstanceId;
use fedimint_core::encoding::Encodable;
use fedimint_core::endpoint_constants::STATUS_ENDPOINT;
use fedimint_core::module::ApiRequestErased;
use fedimint_core::Amount;
use fedimint_wallet_common::endpoint_constants::WALLET_SUMMARY_ENDPOINT;
use fedimint_wallet_common::{TxOutputSummary, WalletSummary};
use fmo_api_types::{
    FederationUtxo, FederationUtxosResponse, GuardianClaimedUtxo, GuardianClaimedUtxoState,
    GuardianUtxoClaim, GuardianUtxoClaimStatus, GuardianUtxoDisagreementKind, GuardianUtxoReport,
    UtxoComparisonRow, UtxoComparisonStatus, UtxoStatusCounts, UtxoView,
};
use fmo_core::module::{module_instance_id, query, ApiError, ApiResult, ClientError, Services};
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use postgres_from_row::FromRow;
use tokio::time::Instant;
use tracing::{debug, warn};

use crate::network::{address_on_network, federation_wallet_network};

/// A comparison younger than this is served as is.
const UTXO_REPORT_FRESH: Duration = Duration::from_secs(60);
/// A comparison that left something out (a guardian cut off, a chain lookup
/// unfinished) is replaced in the background after this.
const UTXO_REPORT_INCOMPLETE_FRESH: Duration = Duration::from_secs(10);
/// Older comparisons, up to this age, are served while a background refresh
/// replaces them, so only the first view after a long pause waits for the
/// guardians and the chain.
const UTXO_REPORT_MAX_STALE: Duration = Duration::from_secs(15 * 60);

/// How long each guardian gets for its wallet summary and status
const GUARDIAN_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// While a viewer waits: once a threshold of guardians answered, how much
/// longer the others get. Guardians cut off count as not answering in this
/// comparison, which is then refreshed in the background with the full timeout.
const GUARDIAN_QUORUM_GRACE: Duration = Duration::from_secs(2);

/// Unspent and missing results are rechecked after this; spends are final.
const ONCHAIN_CHECK_TTL: Duration = Duration::from_secs(10 * 60);
/// While a viewer waits: bound on chain lookups. Results that arrive in time
/// are cached, and a comparison missing some is refreshed in the background,
/// which continues where this one stopped.
const ONCHAIN_CHECK_DEADLINE: Duration = Duration::from_secs(4);
/// Bound on the chain lookups of a background refresh
const ONCHAIN_REFRESH_DEADLINE: Duration = Duration::from_secs(60);
const ONCHAIN_CHECK_CONCURRENCY: usize = 8;

/// Rows per page unless the request says otherwise, as the frontend shows
const UTXOS_DEFAULT_LIMIT: usize = 25;
const UTXOS_MAX_LIMIT: usize = 100;

/// State of a transaction output according to the blockchain
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnChainOutput {
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
    reports: Mutex<HashMap<FederationId, Arc<ReportSlot>>>,
    onchain: Mutex<HashMap<OutPoint, (Instant, OnChainOutput)>>,
}

/// One federation's latest comparison
#[derive(Default)]
struct ReportSlot {
    latest: Mutex<Option<CachedReport>>,
    /// Held by whoever computes the comparison, so concurrent misses share one
    /// computation and at most one background refresh runs at a time
    computing: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone)]
struct CachedReport {
    report: Arc<UtxoReport>,
    fresh_until: Instant,
    expires_at: Instant,
}

impl ReportSlot {
    /// The cached comparison, unless it is too old to serve at all
    fn servable(&self, now: Instant) -> Option<CachedReport> {
        self.latest
            .lock()
            .expect("UTXO report cache lock poisoned")
            .clone()
            .filter(|cached| now < cached.expires_at)
    }

    fn store(&self, report: UtxoReport, complete: bool) -> Arc<UtxoReport> {
        let now = Instant::now();
        let report = Arc::new(report);
        *self.latest.lock().expect("UTXO report cache lock poisoned") = Some(CachedReport {
            report: report.clone(),
            fresh_until: now
                + if complete {
                    UTXO_REPORT_FRESH
                } else {
                    UTXO_REPORT_INCOMPLETE_FRESH
                },
            expires_at: now + UTXO_REPORT_MAX_STALE,
        });
        report
    }
}

/// Whether the comparison is computed for a waiting viewer, who gets it
/// sooner, or refreshed in the background, which waits for everything
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Interactive,
    Background,
}

impl Mode {
    fn guardian_grace(self) -> Option<Duration> {
        match self {
            Mode::Interactive => Some(GUARDIAN_QUORUM_GRACE),
            Mode::Background => None,
        }
    }

    fn onchain_deadline(self) -> Duration {
        match self {
            Mode::Interactive => ONCHAIN_CHECK_DEADLINE,
            Mode::Background => ONCHAIN_REFRESH_DEADLINE,
        }
    }
}

/// The federation a comparison is about
#[derive(Clone)]
struct Target {
    federation_id: FederationId,
    config: ClientConfig,
    wallet_id: ModuleInstanceId,
    network: Network,
}

/// The endpoint's view of core and the module's caches
#[derive(Clone)]
struct Utxos {
    services: Services,
    caches: Arc<UtxoCaches>,
}

/// `GET /federations/:federation_id/utxos`
pub(crate) fn federation_routes(caches: Arc<UtxoCaches>) -> Router<Services> {
    Router::new().route(
        "/utxos",
        get(move |path, query, state| get_federation_utxos(caches.clone(), path, query, state)),
    )
}

async fn get_federation_utxos(
    caches: Arc<UtxoCaches>,
    Path(federation_id): Path<FederationId>,
    Query(params): Query<HashMap<String, String>>,
    State(services): State<Services>,
) -> ApiResult<Json<FederationUtxosResponse>> {
    let request = UtxoPageRequest::parse(&params)?;
    let federation = services.federation(federation_id).await?.ok_or_else(|| {
        ClientError::not_found(format!("Federation {federation_id} is not observed"))
    })?;
    let config = federation.config;
    let wallet_id = module_instance_id(&config, "wallet")
        .ok_or_else(|| ClientError::not_found("Federation has no Wallet v1 module"))?;
    let network = federation_wallet_network(&config, wallet_id)?;
    let utxos = Utxos { services, caches };
    let report = utxos
        .report(Target {
            federation_id,
            config,
            wallet_id,
            network,
        })
        .await?;
    Ok(Json(report.page(&request)))
}

/// What page of which rows a request asks for
#[derive(Debug, Clone, PartialEq, Eq)]
struct UtxoPageRequest {
    view: UtxoView,
    /// Trimmed and lowercased; empty matches everything
    search: String,
    /// 1-based
    page: usize,
    limit: usize,
}

impl UtxoPageRequest {
    /// `view=all|verified|pending|mismatch`, `search`, `page` (from 1) and
    /// `limit` (1 to 100); anything else is a 400
    fn parse(params: &HashMap<String, String>) -> ApiResult<Self> {
        let view = match params.get("view").map(String::as_str) {
            None | Some("all") => UtxoView::All,
            Some("verified") => UtxoView::Verified,
            Some("pending") => UtxoView::Pending,
            Some("mismatch") => UtxoView::Mismatch,
            Some(_) => {
                return Err(ApiError::bad_request(
                    "view must be all, verified, pending or mismatch",
                ))
            }
        };
        let number = |name: &str, default: usize, max: usize| -> ApiResult<usize> {
            let Some(value) = params.get(name) else {
                return Ok(default);
            };
            value
                .parse::<usize>()
                .ok()
                .filter(|value| (1..=max).contains(value))
                .ok_or_else(|| {
                    ApiError::bad_request(if max == usize::MAX {
                        format!("{name} must be a positive integer")
                    } else {
                        format!("{name} must be an integer from 1 to {max}")
                    })
                })
        };
        Ok(Self {
            view,
            search: params
                .get("search")
                .map(|search| search.trim().to_lowercase())
                .unwrap_or_default(),
            page: number("page", 1, usize::MAX)?,
            limit: number("limit", UTXOS_DEFAULT_LIMIT, UTXOS_MAX_LIMIT)?,
        })
    }
}

/// A federation's full comparison, computed once per refresh and paged per
/// request
struct UtxoReport {
    threshold: usize,
    guardians: Vec<GuardianUtxoReport>,
    /// Most severe status first, then largest amount first
    rows: Vec<UtxoComparisonRow>,
    counts: UtxoStatusCounts,
}

impl UtxoReport {
    /// The requested page of the rows matching the view and search
    fn page(&self, request: &UtxoPageRequest) -> FederationUtxosResponse {
        let matching = self
            .rows
            .iter()
            .filter(|row| row_matches(row, request.view, &request.search))
            .collect::<Vec<_>>();
        let page_count = matching.len().div_ceil(request.limit).max(1);
        // Clamped so a refresh that shrinks the list never serves an empty page
        let page = request.page.min(page_count);
        FederationUtxosResponse {
            threshold: self.threshold,
            guardians: self.guardians.clone(),
            utxos: matching
                .iter()
                .skip((page - 1) * request.limit)
                .take(request.limit)
                .map(|row| (*row).clone())
                .collect(),
            counts: self.counts,
            total_rows: matching.len(),
            page,
            limit: request.limit,
            page_count,
        }
    }
}

/// Whether a row is in `view` and matches `search` (trimmed and lowercased):
/// its outpoint, address or amount in BTC with 8 decimals (`0.00150000`)
/// contains it
fn row_matches(row: &UtxoComparisonRow, view: UtxoView, search: &str) -> bool {
    let in_view = match view {
        UtxoView::All => true,
        UtxoView::Verified => row.status == UtxoComparisonStatus::Verified,
        UtxoView::Pending => row.status == UtxoComparisonStatus::Pending,
        UtxoView::Mismatch => row.status == UtxoComparisonStatus::Mismatch,
    };
    in_view
        && (search.is_empty()
            || row.out_point.to_string().to_lowercase().contains(search)
            || row.address.as_ref().is_some_and(|address| {
                address
                    .assume_checked_ref()
                    .to_string()
                    .to_lowercase()
                    .contains(search)
            })
            || btc_amount(row.amount).contains(search))
}

/// An amount in BTC with 8 decimals, as the frontend shows it (`0.00150000`)
fn btc_amount(amount: Amount) -> String {
    let sats = amount.msats.saturating_add(500) / 1000;
    format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000)
}

fn status_counts(rows: &[UtxoComparisonRow]) -> UtxoStatusCounts {
    let count = |status| rows.iter().filter(|row| row.status == status).count();
    UtxoStatusCounts {
        all: rows.len(),
        verified: count(UtxoComparisonStatus::Verified),
        pending: count(UtxoComparisonStatus::Pending),
        mismatch: count(UtxoComparisonStatus::Mismatch),
    }
}

/// `n - f`: what Fedimint, tolerating f = (n - 1) / 3 faulty guardians, acts on
fn agreement_threshold(num_guardians: usize) -> usize {
    num_guardians - num_guardians.saturating_sub(1) / 3
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
    let threshold = agreement_threshold(num_guardians);
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

    fn into_report(mut self) -> UtxoReport {
        self.rows.sort_by(|left, right| {
            left.status
                .cmp(&right.status)
                .then(right.amount.cmp(&left.amount))
        });
        UtxoReport {
            threshold: self.threshold,
            guardians: self.guardians,
            counts: status_counts(&self.rows),
            rows: self.rows,
        }
    }
}

impl Utxos {
    /// The federation's comparison: cached, refreshed in the background once
    /// stale, and computed while the viewer waits only when there is none to
    /// serve
    async fn report(&self, target: Target) -> anyhow::Result<Arc<UtxoReport>> {
        let slot = self
            .caches
            .reports
            .lock()
            .expect("UTXO report cache lock poisoned")
            .entry(target.federation_id)
            .or_default()
            .clone();

        if let Some(cached) = slot.servable(Instant::now()) {
            if Instant::now() >= cached.fresh_until {
                // Stale while revalidate: one refresh at a time per federation
                if let Ok(computing) = slot.computing.clone().try_lock_owned() {
                    let this = self.clone();
                    let slot = slot.clone();
                    tokio::spawn(async move {
                        let _computing = computing;
                        match this.compute(&target, Mode::Background).await {
                            Ok((report, complete)) => {
                                slot.store(report, complete);
                            }
                            Err(error) => warn!(
                                federation_id = %target.federation_id,
                                ?error,
                                "UTXO comparison refresh failed; serving the previous one"
                            ),
                        }
                    });
                }
            }
            return Ok(cached.report);
        }

        // Concurrent misses wait here for the first one's computation
        let _computing = slot.computing.lock().await;
        if let Some(cached) = slot.servable(Instant::now()) {
            return Ok(cached.report);
        }
        let (report, complete) = self.compute(&target, Mode::Interactive).await?;
        Ok(slot.store(report, complete))
    }

    /// The full comparison, and whether nothing was left out of it for time
    async fn compute(&self, target: &Target, mode: Mode) -> anyhow::Result<(UtxoReport, bool)> {
        let (observed, (guardian_claims, claims_complete)) = tokio::try_join!(
            self.federation_utxos(target.federation_id, target.network),
            self.guardian_utxo_claims(target, mode),
        )?;
        let mut comparison = compare_utxos(&observed, &guardian_claims);
        let (onchain, onchain_complete) = self
            .onchain_outputs(comparison.onchain_check_requests(), mode.onchain_deadline())
            .await;
        comparison.apply_onchain_evidence(&onchain);
        Ok((
            comparison.into_report(),
            claims_complete && onchain_complete,
        ))
    }

    /// The outputs in the observer's history, with addresses for the
    /// federation's network
    async fn federation_utxos(
        &self,
        federation_id: FederationId,
        network: Network,
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
            // language=postgresql
            "SELECT on_chain_txid, on_chain_vout, address, amount_msat FROM fmo_wallet.utxos WHERE federation_id = $1 ORDER BY amount_msat DESC",
            &[&federation_id.consensus_encode_to_vec()],
        ).await?.into_iter().map(|utxo| {
            Result::<_, anyhow::Error>::Ok(FederationUtxo {
                // Older rows hold mainnet renderings whatever the network
                address: address_on_network(&utxo.address, network)?,
                out_point: OutPoint {
                    txid: Txid::from_slice(&utxo.on_chain_txid)?,
                    vout: utxo.on_chain_vout.try_into()?,
                },
                amount: Amount::from_msats(utxo.amount_msat.try_into()?),
            })
        }).collect()
    }

    /// Every guardian's wallet summary, and whether every guardian got its
    /// full time to answer
    async fn guardian_utxo_claims(
        &self,
        target: &Target,
        mode: Mode,
    ) -> anyhow::Result<(Vec<GuardianUtxoClaim>, bool)> {
        let endpoints = &target.config.global.api_endpoints;
        // The process-wide client: one per request would leak a connection to
        // every guardian each time
        let api = self.services.federation_api(&target.config.global)?;
        let module_api = api.with_module(target.wallet_id);

        let requests = endpoints
            .keys()
            .map(|&peer_id| {
                let api = api.clone();
                let module_api = module_api.clone();
                let guardian_id = peer_id.to_usize() as u16;
                let claim = async move {
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
                        tokio::time::timeout(GUARDIAN_REQUEST_TIMEOUT, summary),
                        tokio::time::timeout(GUARDIAN_REQUEST_TIMEOUT, status),
                    )
                    .await;

                    let (status, session_count, utxos, error) = match summary {
                        Ok(Ok(summary)) => {
                            let session_count =
                                status.ok().and_then(Result::ok).and_then(|status| {
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
                };
                (guardian_id, claim)
            })
            .collect::<Vec<_>>();
        let quorum = mode
            .guardian_grace()
            .map(|grace| (agreement_threshold(endpoints.len()), grace));
        let (mut claims, complete) = gather_claims(requests, quorum).await;
        mark_lagging_guardians(&mut claims, endpoints.len());

        Ok((claims, complete))
    }

    /// Looks up outputs on-chain. Each request says whether the output's amount
    /// is needed too, which costs an extra request. Outputs whose lookup fails
    /// or does not finish by `deadline` are left out, and the result says
    /// whether any were.
    async fn onchain_outputs(
        &self,
        requests: Vec<(OutPoint, bool)>,
        deadline: Duration,
    ) -> (HashMap<OutPoint, OnChainOutput>, bool) {
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
            return (results, true);
        }

        let wanted = results.len() + to_fetch.len();
        let mut lookups = futures::stream::iter(to_fetch)
            .map(|(out_point, with_amount)| {
                let client = self.services.esplora();
                async move {
                    let output = fetch_onchain_output(client, out_point, with_amount).await;
                    (out_point, output)
                }
            })
            .buffer_unordered(ONCHAIN_CHECK_CONCURRENCY);
        let _ = tokio::time::timeout(deadline, async {
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

        let complete = results.len() == wanted;
        (results, complete)
    }
}

/// Collects each guardian's claim, in guardian order. With a quorum
/// `(threshold, grace)`, guardians still outstanding `grace` after `threshold`
/// of them answered are reported as not answering in time, and the result says
/// it is incomplete.
async fn gather_claims<F>(
    requests: Vec<(u16, F)>,
    quorum: Option<(usize, Duration)>,
) -> (Vec<GuardianUtxoClaim>, bool)
where
    F: Future<Output = GuardianUtxoClaim>,
{
    let mut outstanding = requests.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
    let mut pending = requests
        .into_iter()
        .map(|(_, claim)| claim)
        .collect::<FuturesUnordered<_>>();
    let mut claims = Vec::new();
    let mut answered = 0;
    let mut cutoff = None;
    loop {
        let next = match cutoff {
            Some(cutoff) => match tokio::time::timeout_at(cutoff, pending.next()).await {
                Ok(next) => next,
                Err(_) => break,
            },
            None => pending.next().await,
        };
        let Some(claim) = next else {
            break;
        };
        outstanding.remove(&claim.guardian_id);
        if matches!(claim.status, GuardianUtxoClaimStatus::Ok) {
            answered += 1;
        }
        claims.push(claim);
        if let (None, Some((threshold, grace))) = (cutoff, quorum) {
            if answered >= threshold {
                cutoff = Some(Instant::now() + grace);
            }
        }
    }
    let complete = outstanding.is_empty();
    claims.extend(
        outstanding
            .into_iter()
            .map(|guardian_id| GuardianUtxoClaim {
                guardian_id,
                status: GuardianUtxoClaimStatus::Error,
                session_count: None,
                utxos: Vec::new(),
                error: Some("Guardian wallet summary timed out".to_owned()),
            }),
    );
    claims.sort_by_key(|claim| claim.guardian_id);
    (claims, complete)
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

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    use super::*;

    fn row(
        txid_byte: u8,
        sats: u64,
        address: Option<&str>,
        status: UtxoComparisonStatus,
    ) -> UtxoComparisonRow {
        UtxoComparisonRow {
            out_point: OutPoint {
                txid: Txid::from_byte_array([txid_byte; 32]),
                vout: 1,
            },
            amount: Amount::from_sats(sats),
            address: address.map(|address| address.parse().unwrap()),
            guardian_states: BTreeMap::new(),
            status,
            disagreement: None,
            detail: None,
        }
    }

    fn report(rows: Vec<UtxoComparisonRow>) -> UtxoReport {
        UtxoReport {
            threshold: 3,
            guardians: Vec::new(),
            counts: status_counts(&rows),
            rows,
        }
    }

    fn request(pairs: &[(&str, &str)]) -> ApiResult<UtxoPageRequest> {
        UtxoPageRequest::parse(
            &pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
        )
    }

    #[test]
    fn page_requests_default_and_reject_out_of_range_values() {
        assert_eq!(
            request(&[]).unwrap(),
            UtxoPageRequest {
                view: UtxoView::All,
                search: String::new(),
                page: 1,
                limit: 25,
            }
        );
        let parsed = request(&[
            ("view", "mismatch"),
            ("search", "  TB1Q  "),
            ("page", "3"),
            ("limit", "100"),
        ])
        .unwrap();
        assert_eq!(parsed.view, UtxoView::Mismatch);
        assert_eq!(parsed.search, "tb1q");
        assert_eq!((parsed.page, parsed.limit), (3, 100));

        for bad in [
            ("view", "spent"),
            ("page", "0"),
            ("page", "-1"),
            ("page", "x"),
            ("limit", "0"),
            ("limit", "101"),
            ("limit", ""),
        ] {
            let status = request(&[bad]).unwrap_err().into_response().status();
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad:?}");
        }
    }

    #[test]
    fn rows_are_filtered_searched_and_paged_on_the_server() {
        use UtxoComparisonStatus::*;
        let report = report(vec![
            row(
                0x11,
                150_000,
                Some("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4"),
                Mismatch,
            ),
            row(
                0x22,
                2_000_000,
                Some("1BvBMSEYstWetqTFn5Au4m4GFg7xJaNVN2"),
                Pending,
            ),
            row(0x33, 1_000, None, Pending),
            row(0x44, 50_000, None, Verified),
            row(0x55, 60_000, None, Verified),
            row(0x66, 70_000, None, Verified),
        ]);
        let page = |pairs: &[(&str, &str)]| report.page(&request(pairs).unwrap());

        let all = page(&[]);
        assert_eq!(
            all.counts,
            UtxoStatusCounts {
                all: 6,
                verified: 3,
                pending: 2,
                mismatch: 1,
            }
        );
        assert_eq!(
            (all.total_rows, all.page, all.limit, all.page_count),
            (6, 1, 25, 1)
        );
        // Same order as the full comparison
        assert_eq!(
            all.utxos.iter().map(|row| row.amount).collect::<Vec<_>>(),
            report.rows.iter().map(|row| row.amount).collect::<Vec<_>>()
        );

        let verified = page(&[("view", "verified"), ("limit", "2")]);
        assert_eq!((verified.total_rows, verified.page_count), (3, 2));
        assert_eq!(verified.utxos.len(), 2);
        assert_eq!(verified.counts.all, 6, "counts ignore the view");
        let second = page(&[("view", "verified"), ("limit", "2"), ("page", "2")]);
        assert_eq!(second.utxos.len(), 1);
        assert_eq!(second.utxos[0].out_point, report.rows[5].out_point);
        // Past the end: the last page, not an empty one
        let clamped = page(&[("view", "verified"), ("limit", "2"), ("page", "9")]);
        assert_eq!(clamped.page, 2);
        assert_eq!(clamped.utxos.len(), 1);

        // Amount in BTC with 8 decimals
        let by_amount = page(&[("search", "0.00150000")]);
        assert_eq!(by_amount.total_rows, 1);
        assert_eq!(by_amount.utxos[0].status, Mismatch);
        assert_eq!(page(&[("search", "0.02000000")]).total_rows, 1);
        // The frontend's amounts end in an uppercase " BTC" that a lowercased
        // query never matched, so the unit isn't searchable
        assert_eq!(page(&[("search", "btc")]).total_rows, 0);
        // Addresses match case-insensitively, base58 included
        assert_eq!(page(&[("search", "BC1QW508")]).total_rows, 1);
        assert_eq!(page(&[("search", "1bvbmseys")]).total_rows, 1);
        // Outpoints, as txid:vout
        assert_eq!(
            page(&[("search", &format!("{}:1", "33".repeat(32)))]).total_rows,
            1
        );
        // Search and view combine
        assert_eq!(
            page(&[("search", "bc1q"), ("view", "pending")]).total_rows,
            0
        );

        let none = page(&[("search", "nothing like this")]);
        assert_eq!((none.total_rows, none.page, none.page_count), (0, 1, 1));
        assert!(none.utxos.is_empty());
        assert_eq!(none.counts.all, 6);
    }

    #[test]
    fn amounts_are_shown_like_the_frontend() {
        assert_eq!(btc_amount(Amount::from_sats(150_000)), "0.00150000");
        assert_eq!(
            btc_amount(Amount::from_sats(2_100_000_000_000_000)),
            "21000000.00000000"
        );
        assert_eq!(btc_amount(Amount::from_msats(0)), "0.00000000");
    }

    fn claim(guardian_id: u16) -> GuardianUtxoClaim {
        GuardianUtxoClaim {
            guardian_id,
            status: GuardianUtxoClaimStatus::Ok,
            session_count: Some(10),
            utxos: Vec::new(),
            error: None,
        }
    }

    #[tokio::test]
    async fn a_waiting_viewer_does_not_wait_for_a_silent_guardian() {
        let requests = (0..4u16)
            .map(|id| {
                let claim = async move {
                    if id == 2 {
                        std::future::pending::<()>().await;
                    }
                    claim(id)
                };
                (id, Box::pin(claim))
            })
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let (claims, complete) = gather_claims(
            requests,
            Some((agreement_threshold(4), Duration::from_millis(50))),
        )
        .await;
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!complete);
        assert_eq!(
            claims
                .iter()
                .map(|claim| claim.guardian_id)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert!(matches!(claims[2].status, GuardianUtxoClaimStatus::Error));
        assert!(matches!(claims[3].status, GuardianUtxoClaimStatus::Ok));
    }

    #[tokio::test]
    async fn without_a_quorum_every_guardian_is_awaited() {
        let requests = (0..4u16)
            .rev()
            .map(|id| {
                let claim = async move {
                    tokio::time::sleep(Duration::from_millis(u64::from(id) * 20)).await;
                    claim(id)
                };
                (id, Box::pin(claim))
            })
            .collect::<Vec<_>>();
        let (claims, complete) = gather_claims(requests, None).await;
        assert!(complete);
        assert_eq!(claims.len(), 4);
        assert!(claims
            .iter()
            .all(|claim| matches!(claim.status, GuardianUtxoClaimStatus::Ok)));
    }

    #[test]
    fn incomplete_reports_go_stale_sooner() {
        let slot = ReportSlot::default();
        let now = Instant::now();
        slot.store(report(Vec::new()), false);
        let cached = slot.servable(now).unwrap();
        assert!(cached.fresh_until <= now + UTXO_REPORT_INCOMPLETE_FRESH + Duration::from_secs(1));
        assert!(slot
            .servable(now + UTXO_REPORT_MAX_STALE + Duration::from_secs(1))
            .is_none());
        slot.store(report(Vec::new()), true);
        assert!(slot.servable(now).unwrap().fresh_until >= now + UTXO_REPORT_FRESH);
    }
}
