//! Collection of federations' history: fetches each observed federation's
//! signed sessions from its guardians and stores them with core's rows of
//! their items, from the summaries of the build's modules. Also fetches block
//! times, which date sessions, and re-stores ranges of stored sessions on an
//! admin's request (backfill). Modules' own tables are derived from the stored
//! sessions afterwards (`projector.rs`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::DateTime;
use deadpool_postgres::Transaction;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::DynModuleConsensusItem;
use fedimint_core::encoding::Encodable;
use fedimint_core::epoch::ConsensusItem;
use fedimint_core::session_outcome::SessionOutcome;
use fedimint_core::util::backoff_util::background_backoff;
use fedimint_core::util::retry;
use fedimint_core::PeerId;
use futures::StreamExt;
use tokio::sync::Notify;
use tokio::time::sleep;
use tracing::{debug, info, info_span, warn, Instrument};

use crate::federation::instance_to_kind;
use crate::federation::observer::FederationObserver;
use crate::store::db;
use crate::store::rows::{
    insert_block_height_vote, insert_consensus_item, insert_input_details, insert_output_details,
    insert_unix_time_vote, CiPosition,
};
use crate::util::{query_value, SyncProgress};

impl FederationObserver {
    pub(crate) async fn fetch_block_times(self) {
        const SLEEP_SECS: u64 = 60;
        loop {
            if let Err(e) = self.fetch_block_times_inner().await {
                warn!("Error while fetching block times: {e:?}");
            }
            info!("Block sync finished, waiting {SLEEP_SECS} seconds");
            sleep(Duration::from_secs(SLEEP_SECS)).await;
        }
    }

    async fn fetch_block_times_inner(&self) -> anyhow::Result<()> {
        let esplora_client = self.esplora();

        // block_times.sql seeds the table on first start, so the fallback only
        // matters if that seed is ever dropped. Block 820k was mined Dec 2023,
        // before any compatible federation.
        let next_block_height = self.last_fetched_block_height().await?.unwrap_or(820_000) + 1;
        let current_block_height = esplora_client.get_height().await?;

        info!("Fetching block times for block {next_block_height} to {current_block_height}");

        let mut block_stream = futures::stream::iter(next_block_height..=current_block_height)
            .map(move |block_height| {
                let esplora_client_inner = esplora_client.clone();
                async move {
                    let block_hash = esplora_client_inner.get_block_hash(block_height).await?;
                    let block = esplora_client_inner.get_header_by_hash(&block_hash).await?;

                    Result::<_, anyhow::Error>::Ok((block_height, block))
                }
            })
            .buffered(4);

        let mut progress = SyncProgress::new("block", next_block_height.into());
        while let Some((block_height, block)) = block_stream.next().await.transpose()? {
            self.connection()
                .await?
                .execute(
                    "INSERT INTO block_times VALUES ($1, $2)",
                    &[
                        &(block_height as i32),
                        &DateTime::from_timestamp(block.time as i64, 0)
                            .expect("Invalid timestamp")
                            .naive_utc(),
                    ],
                )
                .await?;
            progress.record(block_height.into());
        }

        Ok(())
    }

    /// Height of the newest block whose time is stored
    pub async fn last_fetched_block_height(&self) -> anyhow::Result<Option<u32>> {
        let max_height = query_value::<Option<i32>>(
            &self.connection().await?,
            "SELECT MAX(block_height) AS max_height FROM block_times",
            &[],
        )
        .await?;

        Ok(max_height.map(|max_height| max_height as u32))
    }

    pub(super) async fn observe_federation_history(
        &self,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        // Shared, so the supervisor restarting this task doesn't leak
        // guardian connections
        let api = self.federation_api(&config.global)?;
        let decoders = self.modules().decoders_for_config(&config);

        info!("Starting background job for {federation_id}");
        let next_session = self.next_session_index(federation_id).await?;
        debug!("Next session {next_session}");
        let api_fetch = api.clone();
        let gate = SessionFetchGate::new(next_session);
        let fetch_gate = gate.clone();
        let mut session_stream = futures::stream::iter(next_session..)
            .map(move |session_index| {
                debug!("Starting fetch job for session {session_index}");
                let api_fetch_single = api_fetch.clone();
                let decoders_single = decoders.clone();
                let gate = fetch_gate.clone();
                async move {
                    let signed_session_outcome = retry(
                        format!("Waiting for session {session_index}"),
                        background_backoff(),
                        || async {
                            gate.wait_turn(session_index).await;
                            let result = api_fetch_single
                                .await_block(session_index, &decoders_single)
                                .await;
                            gate.record(result.is_ok());
                            result
                        },
                    )
                    .await
                    .expect("Will fail after 136 years");
                    debug!("Finished fetch job for session {session_index}");
                    (session_index, signed_session_outcome)
                }
            })
            .buffered(32);

        let mut progress = SyncProgress::new("session", next_session);
        while let Some((session_index, signed_session_outcome)) = session_stream.next().await {
            let _permit = self.work_budget.acquire().await?;
            let mut connection = self.connection().await?;
            let dbtx = connection.transaction().await?;
            // Waiting is right for every lock this takes: a step of a
            // read-model rebuild (record_new_session) or another writer's
            // commit of the federation's totals. Both are short, but the
            // default 5 s would fail the session and restart the sync.
            dbtx.batch_execute("SET LOCAL lock_timeout = '60s'").await?;
            let new = self
                .process_session(
                    federation_id,
                    &config,
                    session_index,
                    signed_session_outcome,
                    &dbtx,
                )
                .await?;
            // Read models count each session once, in the transaction that
            // stores it. A session that was already stored has been counted.
            if new {
                dbtx.execute(
                    "SELECT record_new_session($1, $2)",
                    &[
                        &federation_id.consensus_encode_to_vec(),
                        &(session_index as i32),
                    ],
                )
                .await?;
            }
            dbtx.commit().await?;
            self.projector_wakeups().wake(federation_id);
            gate.stored(session_index);
            progress.record(session_index);
        }

        unreachable!("Session stream should never end")
    }

    /// Stores a session and its items. Returns whether the session was new;
    /// items of a stored session are filled in where missing.
    async fn process_session(
        &self,
        federation_id: FederationId,
        config: &ClientConfig,
        session_index: u64,
        signed_session_outcome: SessionOutcome,
        dbtx: &Transaction<'_>,
    ) -> anyhow::Result<bool> {
        let inserted = dbtx
            .execute(
                "INSERT INTO sessions VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &(session_index as i32),
                    &signed_session_outcome.consensus_encode_to_vec(),
                ],
            )
            .await?;

        for (item_idx, item) in signed_session_outcome.items.into_iter().enumerate() {
            match item.item {
                ConsensusItem::Transaction(transaction) => {
                    self.process_transaction(
                        dbtx,
                        federation_id,
                        config,
                        session_index,
                        item_idx as u64,
                        transaction,
                    )
                    .await?;
                }
                ConsensusItem::Module(module_ci) => {
                    self.process_ci(
                        dbtx,
                        federation_id,
                        config,
                        session_index,
                        item_idx as u64,
                        item.peer,
                        module_ci,
                    )
                    .await?;
                }
                _ => {
                    // Ignore unknown CIs
                }
            }
        }

        debug!("Processed session {session_index} of federation {federation_id}");
        Ok(inserted == 1)
    }

    async fn process_transaction(
        &self,
        dbtx: &Transaction<'_>,
        federation_id: FederationId,
        config: &ClientConfig,
        session_index: u64,
        item_index: u64,
        transaction: fedimint_core::transaction::Transaction,
    ) -> anyhow::Result<()> {
        let fedimint_txid = transaction.tx_hash();

        dbtx.execute(
            "INSERT INTO transactions VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            &[
                &fedimint_txid.consensus_encode_to_vec(),
                &federation_id.consensus_encode_to_vec(),
                &(session_index as i32),
                &(item_index as i32),
                &transaction.consensus_encode_to_vec(),
            ],
        )
        .await?;

        for (in_idx, input) in transaction.inputs.into_iter().enumerate() {
            let kind = instance_to_kind(config, input.module_instance_id());
            let summary = summary_or_default(
                &kind,
                "input",
                self.modules()
                    .get(&kind)
                    .map(|module| module.summarize_input(&input)),
            );

            dbtx.execute(
                "INSERT INTO transaction_inputs
                 VALUES ($1, $2, $3, $4, $5, COALESCE($6, (
                     SELECT amount_msat FROM transaction_outputs
                     WHERE federation_id = $1 AND txid = $7 AND out_index = $8
                 ))) ON CONFLICT DO NOTHING",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &fedimint_txid.consensus_encode_to_vec(),
                    &(in_idx as i32),
                    &kind,
                    &summary.ln_contract_id,
                    &summary.amount_msat.map(|amt| amt as i64),
                    &summary
                        .claimed_output
                        .map(|output| output.txid.consensus_encode_to_vec()),
                    &summary.claimed_output.map(|output| output.out_idx as i32),
                ],
            )
            .await?;

            if let Some(details) = &summary.details {
                insert_input_details(dbtx, federation_id, fedimint_txid, in_idx, &kind, details)
                    .await?;
            }
        }

        for (out_idx, output) in transaction.outputs.into_iter().enumerate() {
            let kind = instance_to_kind(config, output.module_instance_id());
            let summary = summary_or_default(
                &kind,
                "output",
                self.modules()
                    .get(&kind)
                    .map(|module| module.summarize_output(&output)),
            );

            dbtx.execute(
                "INSERT INTO transaction_outputs VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &fedimint_txid.consensus_encode_to_vec(),
                    &(out_idx as i32),
                    &kind,
                    &summary.ln_contract.as_ref().map(|(kind, _id)| *kind),
                    &summary.ln_contract.as_ref().map(|(_kind, id)| id),
                    &summary.amount_msat.map(|amt| amt as i64),
                ],
            )
            .await?;

            if let Some(details) = &summary.details {
                insert_output_details(dbtx, federation_id, fedimint_txid, out_idx, &kind, details)
                    .await?;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_ci(
        &self,
        dbtx: &Transaction<'_>,
        federation_id: FederationId,
        config: &ClientConfig,
        session_index: u64,
        item_index: u64,
        peer_id: PeerId,
        ci: DynModuleConsensusItem,
    ) -> anyhow::Result<()> {
        let kind = instance_to_kind(config, ci.module_instance_id());
        let Some(module) = self.modules().get(&kind) else {
            warn!("Consensus Item of kind {kind}. Not implemented.");
            return Ok(());
        };

        let at = CiPosition {
            federation_id,
            session_index,
            item_index,
            peer: peer_id,
        };
        if let Some(data) = module.summarize_ci(&ci) {
            insert_consensus_item(dbtx, &at, &kind, &data).await?;
        }

        // Votes date the session: session_times, a core read model, is built
        // from them in this transaction (record_new_session)
        if let Some(height) = module.block_height_vote(&ci) {
            insert_block_height_vote(dbtx, &at, height).await?;
        }
        if let Some(time) = module.unix_time_vote(&ci) {
            insert_unix_time_vote(dbtx, &at, time).await?;
        }

        Ok(())
    }

    /// Checks a backfill request and starts it in the background, returning
    /// the stored sessions it covers. One backfill per federation runs at a
    /// time; see [`Self::backfill_federation`].
    pub async fn start_backfill(
        &self,
        federation_id: FederationId,
        session_start: Option<i32>,
        session_end: Option<i32>,
    ) -> anyhow::Result<(i32, i32)> {
        use crate::error::ClientError;

        if session_start.is_some_and(|start| start < 0) || session_end.is_some_and(|end| end < 0) {
            return Err(ClientError::bad_request("Session indices are not negative").into());
        }
        self.ensure_observed(federation_id).await?;
        let Some(last_session) = query_value::<Option<i32>>(
            &self.connection().await?,
            "SELECT MAX(session_index) FROM sessions WHERE federation_id = $1",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?
        else {
            return Err(ClientError::bad_request(format!(
                "Federation {federation_id} has no stored sessions to backfill"
            ))
            .into());
        };
        let session_start = session_start.unwrap_or(0);
        let session_end = session_end.unwrap_or(last_session).min(last_session);
        if session_start > session_end {
            return Err(ClientError::bad_request(format!(
                "session_start {session_start} is after session_end {session_end} (the last stored session is {last_session})"
            ))
            .into());
        }

        static RUNNING: std::sync::LazyLock<
            std::sync::Mutex<std::collections::HashSet<FederationId>>,
        > = std::sync::LazyLock::new(Default::default);
        /// Ends the federation's entry in `RUNNING` however the backfill ends
        struct Running(FederationId);
        impl Drop for Running {
            fn drop(&mut self) {
                RUNNING.lock().expect("not poisoned").remove(&self.0);
            }
        }
        if !RUNNING.lock().expect("not poisoned").insert(federation_id) {
            return Err(ClientError::unavailable(format!(
                "A backfill of federation {federation_id} is already running"
            ))
            .into());
        }
        let running = Running(federation_id);
        let worker = self.worker();
        self.task_group().spawn_cancellable(
            format!("backfill of {federation_id}"),
            async move {
                let _running = running;
                // A panic must not reach the task group, which would stop
                // every task (see `supervise`)
                match futures::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(
                    worker.backfill_federation(federation_id, session_start, session_end),
                ))
                .await
                {
                    Ok(Ok(())) => info!("Backfill complete"),
                    Ok(Err(error)) => warn!("Backfill failed: {error:#}"),
                    Err(_) => warn!("Backfill panicked"),
                }
            }
            // Outlives the request, so not inside its span
            .instrument(info_span!(parent: None, "backfill", fed = %federation_id.to_prefix())),
        );
        Ok((session_start, session_end))
    }

    /// Reprocesses stored sessions of a federation, filling in core rows that
    /// are missing. Commits every `BACKFILL_BATCH` sessions, so an interrupted
    /// run keeps its progress, and has the session times and daily activity
    /// rebuilt from `session_start` by the repair worker: once with the first
    /// batch, so an interrupted run's batches are covered, and once after the
    /// last. Module tables are rebuilt by resetting the module's cursor instead
    /// (`reset_module_cursor`).
    async fn backfill_federation(
        &self,
        federation_id: FederationId,
        session_start: i32,
        session_end: i32,
    ) -> anyhow::Result<()> {
        const BACKFILL_BATCH: i32 = 1000;

        let federation = self
            .get_federation(federation_id)
            .await?
            .with_context(|| format!("Federation {federation_id} is not observed"))?;
        let federation_bytes = federation_id.consensus_encode_to_vec();

        info!("Backfilling federation {federation_id}, sessions {session_start} to {session_end}");
        let decoders = self.modules().decoders_for_config(&federation.config);
        let mut batch_start = session_start;
        let mut processed = 0;
        loop {
            let batch_end = batch_start
                .saturating_add(BACKFILL_BATCH - 1)
                .min(session_end);
            let _permit = self.work_budget.acquire().await?;
            let mut connection = self.connection().await?;
            let dbtx = connection.transaction().await?;
            let rows = dbtx
                .query(
                    "SELECT * FROM sessions WHERE federation_id = $1 AND session_index BETWEEN $2 AND $3 ORDER BY session_index",
                    &[&federation_bytes, &batch_start, &batch_end],
                )
                .await
                .context("querying sessions for federation backfill")?;
            for row in rows {
                let outcome = db::SessionOutcome::try_from_row_with_decoders(&row, &decoders)?;
                self.process_session(
                    federation_id,
                    &federation.config,
                    outcome.session_index as u64,
                    outcome.data,
                    &dbtx,
                )
                .await?;
                processed += 1;
            }
            if batch_start == session_start || batch_end >= session_end {
                dbtx.execute(
                    "SELECT schedule_read_model_rebuild($1, $2)",
                    &[&federation_bytes, &session_start],
                )
                .await?;
            }
            dbtx.commit().await?;
            info!("Backfilled up to session {batch_end} ({processed} sessions)");

            if batch_end >= session_end {
                break;
            }
            batch_start = batch_end + 1;
        }
        Ok(())
    }
}

/// Up to 32 sessions are fetched at once, each retrying on its own. While the
/// guardians don't answer, that is 32 failing requests a minute for a
/// federation that may have been gone for months. Then only the fetch of the
/// next session to store keeps trying; the first fetch that succeeds lets the
/// others go on.
#[derive(Clone)]
struct SessionFetchGate(Arc<SessionFetchGateState>);

struct SessionFetchGateState {
    /// Whether the last fetch that finished failed
    failing: AtomicBool,
    next_to_store: AtomicU64,
    changed: Notify,
}

impl SessionFetchGate {
    fn new(next_to_store: u64) -> Self {
        Self(Arc::new(SessionFetchGateState {
            failing: AtomicBool::new(false),
            next_to_store: AtomicU64::new(next_to_store),
            changed: Notify::new(),
        }))
    }

    /// Returns once the session's fetch may ask the guardians
    async fn wait_turn(&self, session_index: u64) {
        loop {
            // Created before checking, so a change in between still wakes it
            let changed = self.0.changed.notified();
            if !self.0.failing.load(Ordering::SeqCst)
                || self.0.next_to_store.load(Ordering::SeqCst) >= session_index
            {
                return;
            }
            changed.await;
        }
    }

    fn record(&self, succeeded: bool) {
        let was_failing = self.0.failing.swap(!succeeded, Ordering::SeqCst);
        if was_failing && succeeded {
            self.0.changed.notify_waiters();
        }
    }

    fn stored(&self, session_index: u64) {
        self.0
            .next_to_store
            .store(session_index + 1, Ordering::SeqCst);
        self.0.changed.notify_waiters();
    }
}

/// The summary core stores an input or output with: the module's, or, when the
/// build has no module of its kind or the module can't summarize it (a variant
/// newer than the build), none. The item is then stored without amount or
/// details, which the module's projector fills in once a build summarizes it.
/// Failing instead would roll back the session and stall the federation's
/// sync on it until the build is upgraded.
fn summary_or_default<S: Default>(kind: &str, item: &str, summary: Option<anyhow::Result<S>>) -> S {
    match summary {
        Some(Ok(summary)) => summary,
        Some(Err(error)) => {
            warn!("Storing a {kind} {item} without its summary: {error:#}");
            S::default()
        }
        None => {
            warn!("Transaction {item} of kind {kind}. Not implemented.");
            S::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::{summary_or_default, SessionFetchGate};
    use crate::module::{InputSummary, OutputSummary};

    #[tokio::test]
    async fn only_the_next_session_is_fetched_while_fetches_fail() {
        let gate = SessionFetchGate::new(10);
        assert!(gate.wait_turn(12).now_or_never().is_some());

        gate.record(false);
        assert!(gate.wait_turn(10).now_or_never().is_some());
        let mut later = Box::pin(gate.wait_turn(12));
        assert!((&mut later).now_or_never().is_none());

        // Storing session 10 makes 11 the next, 12 still waits
        gate.stored(10);
        assert!(gate.wait_turn(11).now_or_never().is_some());
        assert!((&mut later).now_or_never().is_none());

        // A fetch that succeeds lets the waiting ones go on
        gate.record(true);
        assert!(later.now_or_never().is_some());
    }

    #[test]
    fn unsummarizable_items_are_stored_without_summary() {
        // A module that doesn't know an item's version must not stall ingest
        let input: InputSummary = summary_or_default(
            "mint",
            "input",
            Some(Err(anyhow::anyhow!("Unsupported mint input version"))),
        );
        assert!(input.amount_msat.is_none() && input.details.is_none());
        let output: OutputSummary = summary_or_default("unknown", "output", None);
        assert!(output.amount_msat.is_none() && output.ln_contract.is_none());
        let known = summary_or_default(
            "mint",
            "output",
            Some(Ok(OutputSummary {
                amount_msat: Some(1000),
                ..Default::default()
            })),
        );
        assert_eq!(known.amount_msat, Some(1000));
    }
}
