//! Each module processes a federation's stored sessions on its own cursor.
//!
//! Core's ingest stores a session and the module-agnostic rows in one
//! transaction. The module tables are derived from the stored sessions after
//! that: one projector per module and federation reads the sessions after the
//! module's cursor in batches, runs the module's `process_*` on the items of
//! its kind, and moves the cursor in `module_cursors`, all in one transaction
//! per batch. So a module added to a build later fills its own tables from
//! session 0, a module's bug is fixed by resetting its cursor, and a module
//! that fails or stalls holds back only itself.
//!
//! A module added later also completes the core rows of its items, which
//! ingest stored without the module's summaries: amounts and Lightning
//! contracts, details, consensus items and block height votes. Triggers keep
//! `federation_totals` right as amounts are filled in; the session times and
//! daily activity are rebuilt once the projector has caught up, from the first
//! session it completed (`module_cursors.repair_from`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{ensure, Context};
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::encoding::Encodable;
use fedimint_core::epoch::ConsensusItem;
use fedimint_core::session_outcome::SessionOutcome;
use tokio::sync::Notify;
use tracing::{info, info_span, Instrument};

use crate::federation::observer::{
    insert_block_height_vote, insert_consensus_item, insert_input_details, insert_output_details,
    CiPosition, FederationObserver, SyncProgress,
};
use crate::federation::{db, instance_to_kind};
use crate::module::{
    module_instance_id, CiMeta, DbTransaction, ItemMeta, ObserverModule, ProcessCtx,
};
use crate::task::supervise;

/// Sessions a projector processes per transaction while it is behind
const BATCH_SESSIONS: i64 = 100;
/// How long an idle projector waits before looking for sessions again without
/// having been woken. Every store and reset wakes it; this is a safety net.
const IDLE_POLL: Duration = Duration::from_secs(60);

/// Wakes a federation's projectors when there is something new for them: a
/// stored session, or a cursor an admin moved
#[derive(Clone, Default, Debug)]
pub(crate) struct ProjectorWakeups(Arc<Mutex<HashMap<FederationId, Arc<Notify>>>>);

impl ProjectorWakeups {
    fn for_federation(&self, federation_id: FederationId) -> Arc<Notify> {
        self.0
            .lock()
            .expect("not poisoned")
            .entry(federation_id)
            .or_default()
            .clone()
    }

    pub(crate) fn wake(&self, federation_id: FederationId) {
        self.for_federation(federation_id).notify_waiters();
    }
}

impl FederationObserver {
    /// Starts a projector for every module whose kind the federation runs
    pub(super) fn spawn_projectors(&self, federation: &db::Federation) {
        let federation_id = federation.federation_id;
        for module in self.modules().modules() {
            let kind = module.kind().to_string();
            if module_instance_id(&federation.config, &kind).is_none() {
                continue;
            }
            let slf = self.clone();
            let config = federation.config.clone();
            self.task_group().spawn_cancellable(
                format!("{kind} projector for {federation_id}"),
                supervise(format!("{kind} projector"), move || {
                    let slf = slf.clone();
                    let module = module.clone();
                    let config = config.clone();
                    async move { slf.project(module, federation_id, config).await }
                })
                .instrument(
                    info_span!("projector", kind = %kind, fed = %federation_id.to_prefix()),
                ),
            );
        }
    }

    /// Processes the federation's stored sessions for one module, from its
    /// cursor on, forever
    async fn project(
        &self,
        module: Arc<dyn ObserverModule>,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        let kind = module.kind().to_string();
        let federation_bytes = federation_id.consensus_encode_to_vec();
        let decoders = self.modules().decoders_for_config(&config);
        let wakeup = self.projector_wakeups().for_federation(federation_id);
        let mut progress: Option<SyncProgress> = None;

        loop {
            // Created before reading, so a session stored in between still wakes us
            let woken = wakeup.notified();
            let mut connection = self.connection().await?;
            let dbtx = connection.transaction().await?;
            let (cursor, repair_from) = module_cursor(&dbtx, &kind, &federation_bytes).await?;
            let rows = dbtx
                .query(
                    "SELECT session_index, session FROM sessions
                     WHERE federation_id = $1 AND session_index >= $2
                     ORDER BY session_index
                     LIMIT $3",
                    &[&federation_bytes, &(cursor as i32), &BATCH_SESSIONS],
                )
                .await?;
            if rows.is_empty() {
                if let Some(from) = repair_from {
                    // Caught up: rebuild what the completed core rows feed
                    // once, instead of after every batch
                    let claimed = dbtx
                        .execute(
                            "UPDATE module_cursors SET repair_from = NULL
                             WHERE module_kind = $1 AND federation_id = $2 AND repair_from = $3",
                            &[&kind, &federation_bytes, &(from as i32)],
                        )
                        .await?;
                    if claimed == 1 {
                        dbtx.execute(
                            "SELECT rebuild_read_models($1, $2)",
                            &[&federation_bytes, &(from as i32)],
                        )
                        .await?;
                        dbtx.commit().await?;
                        info!("Rebuilt the session times and daily activity from session {from}, whose core rows this module completed");
                    }
                    continue;
                }
                // Hand the connection back while waiting
                drop(dbtx);
                drop(connection);
                tokio::select! {
                    _ = woken => {}
                    _ = tokio::time::sleep(IDLE_POLL) => {}
                }
                continue;
            }

            let progress = progress.get_or_insert_with(|| SyncProgress::new("session", cursor));
            let sessions = rows
                .iter()
                .map(|row| {
                    let outcome = db::SessionOutcome::try_from_row_with_decoders(row, &decoders)?;
                    Ok((outcome.session_index as u64, outcome.data))
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let next = sessions.last().expect("rows aren't empty").0 + 1;

            let repaired_from =
                complete_core_rows(module.as_ref(), &dbtx, federation_id, &config, &sessions)
                    .await?;
            let ctx = ProcessCtx {
                dbtx: &dbtx,
                config: &config,
                esplora: self.esplora(),
            };
            for (session_index, outcome) in sessions {
                process_module_items(
                    module.as_ref(),
                    &ctx,
                    federation_id,
                    &config,
                    session_index,
                    outcome,
                )
                .await?;
            }

            // Only if nobody moved the cursor meanwhile, which a reset does:
            // then this batch is dropped and the next starts from the new cursor
            let moved = dbtx
                .execute(
                    "INSERT INTO module_cursors (module_kind, federation_id, next_session, repair_from)
                     VALUES ($1, $2, $3, $5)
                     ON CONFLICT (module_kind, federation_id) DO UPDATE
                         SET next_session = EXCLUDED.next_session,
                             repair_from = LEAST(module_cursors.repair_from, EXCLUDED.repair_from),
                             updated_at = now()
                         WHERE module_cursors.next_session = $4",
                    &[
                        &kind,
                        &federation_bytes,
                        &(next as i32),
                        &(cursor as i32),
                        &repaired_from.map(|session| session as i32),
                    ],
                )
                .await?;
            if moved == 0 {
                info!("The cursor was moved while sessions {cursor} to {next} were processed; starting over from it");
                continue;
            }
            dbtx.commit().await?;
            progress.record(next);
        }
    }

    /// How many sessions each module has processed for the federation, by
    /// kind, for the modules of this build the federation runs; none for a
    /// federation that isn't observed
    pub async fn module_positions(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<BTreeMap<String, u64>> {
        let Some(federation) = self.get_federation(federation_id).await? else {
            return Ok(BTreeMap::new());
        };
        let positions = self
            .connection()
            .await?
            .query(
                "SELECT module_kind, next_session FROM module_cursors WHERE federation_id = $1",
                &[&federation_id.consensus_encode_to_vec()],
            )
            .await?
            .into_iter()
            .map(|row| {
                (
                    row.get::<_, String>("module_kind"),
                    row.get::<_, i32>("next_session") as u64,
                )
            })
            .collect::<HashMap<_, _>>();

        Ok(self
            .modules()
            .kinds()
            .filter(|kind| module_instance_id(&federation.config, kind).is_some())
            .map(|kind| {
                (
                    kind.to_owned(),
                    positions.get(kind).copied().unwrap_or_default(),
                )
            })
            .collect())
    }

    /// Moves a module's cursor for the federation to `session_start`, so its
    /// projector processes the sessions from there again. With `clean` the
    /// module first deletes what it derived from the federation's sessions, so
    /// the replay, from 0, rebuilds it.
    pub async fn reset_module_cursor(
        &self,
        federation_id: FederationId,
        kind: &str,
        session_start: u64,
        clean: bool,
    ) -> anyhow::Result<()> {
        let federation = self
            .get_federation(federation_id)
            .await?
            .with_context(|| format!("Federation {federation_id} is not observed"))?;
        let module = self
            .modules()
            .get(kind)
            .with_context(|| format!("This build has no {kind} module"))?;
        ensure!(
            module_instance_id(&federation.config, kind).is_some(),
            "Federation {federation_id} has no {kind} module"
        );
        ensure!(
            !clean || session_start == 0,
            "A clean reset replays every session, so it starts from 0"
        );
        ensure!(
            session_start <= i32::MAX as u64,
            "session_start {session_start} is not a session index"
        );

        let mut connection = self.connection().await?;
        let dbtx = connection.transaction().await?;
        // The cursor first: its row lock waits for a batch the projector is
        // committing, so the delete below also removes that batch's rows, and
        // makes a batch that commits later find the cursor moved and roll back
        dbtx.execute(
            "INSERT INTO module_cursors (module_kind, federation_id, next_session)
             VALUES ($1, $2, $3)
             ON CONFLICT (module_kind, federation_id) DO UPDATE
                 SET next_session = EXCLUDED.next_session, updated_at = now()",
            &[
                &kind,
                &federation_id.consensus_encode_to_vec(),
                &(session_start as i32),
            ],
        )
        .await?;
        if clean {
            let ctx = ProcessCtx {
                dbtx: &dbtx,
                config: &federation.config,
                esplora: self.esplora(),
            };
            module.reset_federation(&ctx, federation_id).await?;
        }
        dbtx.commit().await?;
        self.projector_wakeups().wake(federation_id);

        info!(
            "Reset the {kind} cursor of federation {federation_id} to session {session_start}{}",
            if clean {
                " after deleting its rows"
            } else {
                ""
            }
        );
        Ok(())
    }
}

/// The next session the module processes for the federation, 0 without a row,
/// and the first session whose read models wait for a rebuild, if any
async fn module_cursor(
    dbtx: &DbTransaction<'_>,
    kind: &str,
    federation_bytes: &[u8],
) -> anyhow::Result<(u64, Option<u64>)> {
    Ok(dbtx
        .query_opt(
            "SELECT next_session, repair_from FROM module_cursors
             WHERE module_kind = $1 AND federation_id = $2",
            &[&kind, &federation_bytes],
        )
        .await?
        .map_or((0, None), |row| {
            (
                row.get::<_, i32>(0) as u64,
                row.get::<_, Option<i32>>(1).map(|session| session as u64),
            )
        }))
}

/// Writes the core rows that come from the module's summaries for the items
/// of these sessions that ingest stored while the build lacked the module:
/// their amounts and Lightning contracts, details, consensus items and block
/// height votes. Items whose rows are complete are left alone, so a replay
/// writes nothing here. Returns the first session whose amounts or votes it
/// filled in, from which the read models built on them must be rebuilt.
async fn complete_core_rows(
    module: &dyn ObserverModule,
    dbtx: &DbTransaction<'_>,
    federation_id: FederationId,
    config: &ClientConfig,
    sessions: &[(u64, SessionOutcome)],
) -> anyhow::Result<Option<u64>> {
    let kind = module.kind();
    let kind = kind.as_str();
    let is_ours = |instance_id| instance_to_kind(config, instance_id) == kind;
    let federation_bytes = federation_id.consensus_encode_to_vec();
    let (Some((first, _)), Some((last, _))) = (sessions.first(), sessions.last()) else {
        return Ok(None);
    };
    let session_range = (*first as i32, *last as i32);

    // Ingest stores an item of a kind without its module with no amount; the
    // module's summary always has one
    let txids = sessions
        .iter()
        .flat_map(|(_, outcome)| &outcome.items)
        .filter_map(|item| match &item.item {
            ConsensusItem::Transaction(tx)
                if tx.inputs.iter().any(|i| is_ours(i.module_instance_id()))
                    || tx.outputs.iter().any(|o| is_ours(o.module_instance_id())) =>
            {
                Some(tx.tx_hash().consensus_encode_to_vec())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let incomplete = |table: &'static str, index: &'static str| {
        let txids = &txids;
        let federation_bytes = &federation_bytes;
        async move {
            let mut by_txid: HashMap<Vec<u8>, HashSet<i32>> = HashMap::new();
            if txids.is_empty() {
                return anyhow::Ok(by_txid);
            }
            for row in dbtx
                .query(
                    &format!(
                        "SELECT txid, {index} FROM {table}
                         WHERE federation_id = $1 AND kind = $2 AND amount_msat IS NULL
                           AND txid = ANY($3)"
                    ),
                    &[federation_bytes, &kind, txids],
                )
                .await?
            {
                by_txid
                    .entry(row.get(0))
                    .or_default()
                    .insert(row.get::<_, i32>(1));
            }
            Ok(by_txid)
        }
    };
    let incomplete_inputs = incomplete("transaction_inputs", "in_index").await?;
    let incomplete_outputs = incomplete("transaction_outputs", "out_index").await?;
    let stored_items = |table: &'static str| {
        let federation_bytes = &federation_bytes;
        async move {
            anyhow::Ok(
                dbtx.query(
                    &format!(
                        "SELECT session_index, item_index FROM {table}
                         WHERE federation_id = $1 AND session_index BETWEEN $2 AND $3"
                    ),
                    &[federation_bytes, &session_range.0, &session_range.1],
                )
                .await?
                .into_iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, i32>(1)))
                .collect::<HashSet<_>>(),
            )
        }
    };
    // Read on the first consensus item of the module's kind
    let mut stored_cis: Option<HashSet<(i32, i32)>> = None;
    let mut stored_votes: Option<HashSet<(i32, i32)>> = None;

    let mut completed_from: Option<u64> = None;
    for (session_index, outcome) in sessions {
        let session_index = *session_index;
        let mut completed = |session_index| {
            completed_from = Some(completed_from.map_or(session_index, |s| s.min(session_index)));
        };
        for (item_index, item) in outcome.items.iter().enumerate() {
            match &item.item {
                ConsensusItem::Transaction(tx) => {
                    let txid = tx.tx_hash();
                    let txid_bytes = txid.consensus_encode_to_vec();
                    let missing_inputs = incomplete_inputs.get(&txid_bytes);
                    for (index, input) in tx.inputs.iter().enumerate() {
                        if !missing_inputs.is_some_and(|missing| missing.contains(&(index as i32)))
                            || !is_ours(input.module_instance_id())
                        {
                            continue;
                        }
                        let summary = module.summarize_input(input)?;
                        dbtx.execute(
                            "UPDATE transaction_inputs SET ln_contract_id = $4, amount_msat = $5
                             WHERE federation_id = $1 AND txid = $2 AND in_index = $3",
                            &[
                                &federation_bytes,
                                &txid_bytes,
                                &(index as i32),
                                &summary.ln_contract_id,
                                &summary.amount_msat.map(|amount| amount as i64),
                            ],
                        )
                        .await?;
                        if let Some(details) = &summary.details {
                            insert_input_details(dbtx, federation_id, txid, index, kind, details)
                                .await?;
                        }
                        if summary.amount_msat.is_some() {
                            completed(session_index);
                        }
                    }
                    let missing_outputs = incomplete_outputs.get(&txid_bytes);
                    for (index, output) in tx.outputs.iter().enumerate() {
                        if !missing_outputs.is_some_and(|missing| missing.contains(&(index as i32)))
                            || !is_ours(output.module_instance_id())
                        {
                            continue;
                        }
                        let summary = module.summarize_output(output)?;
                        dbtx.execute(
                            "UPDATE transaction_outputs
                             SET ln_contract_interaction_kind = $4, ln_contract_id = $5, amount_msat = $6
                             WHERE federation_id = $1 AND txid = $2 AND out_index = $3",
                            &[
                                &federation_bytes,
                                &txid_bytes,
                                &(index as i32),
                                &summary.ln_contract.as_ref().map(|(kind, _id)| *kind),
                                &summary.ln_contract.as_ref().map(|(_kind, id)| id),
                                &summary.amount_msat.map(|amount| amount as i64),
                            ],
                        )
                        .await?;
                        if let Some(details) = &summary.details {
                            insert_output_details(dbtx, federation_id, txid, index, kind, details)
                                .await?;
                        }
                        if summary.amount_msat.is_some() {
                            completed(session_index);
                        }
                    }
                }
                ConsensusItem::Module(ci) if is_ours(ci.module_instance_id()) => {
                    let key = (session_index as i32, item_index as i32);
                    let at = CiPosition {
                        federation_id,
                        session_index,
                        item_index: item_index as u64,
                        peer: item.peer,
                    };
                    if stored_cis.is_none() {
                        stored_cis = Some(stored_items("consensus_items").await?);
                    }
                    if !stored_cis.as_ref().expect("just read").contains(&key) {
                        if let Some(data) = module.summarize_ci(ci) {
                            insert_consensus_item(dbtx, &at, kind, &data).await?;
                        }
                    }
                    if let Some(height) = module.block_height_vote(ci) {
                        if stored_votes.is_none() {
                            stored_votes = Some(stored_items("block_height_votes").await?);
                        }
                        if !stored_votes.as_ref().expect("just read").contains(&key) {
                            insert_block_height_vote(dbtx, &at, height).await?;
                            completed(session_index);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(completed_from)
}

/// Runs the module's `process_*` on the items of its kind in one session. The
/// core rows of the items exist: the session was stored with them.
async fn process_module_items(
    module: &dyn ObserverModule,
    ctx: &ProcessCtx<'_>,
    federation_id: FederationId,
    config: &ClientConfig,
    session_index: u64,
    outcome: SessionOutcome,
) -> anyhow::Result<()> {
    let kind = module.kind();
    for (item_index, item) in outcome.items.into_iter().enumerate() {
        let item_index = item_index as u64;
        match item.item {
            ConsensusItem::Transaction(transaction) => {
                let txid = transaction.tx_hash();
                for (index, input) in transaction.inputs.into_iter().enumerate() {
                    if instance_to_kind(config, input.module_instance_id()) != kind.as_str() {
                        continue;
                    }
                    let summary = module.summarize_input(&input)?;
                    let meta = ItemMeta {
                        federation_id,
                        module_instance_id: input.module_instance_id(),
                        txid,
                        session_index,
                        item_index,
                        index: index as u64,
                    };
                    module.process_input(ctx, &meta, &input, &summary).await?;
                }
                for (index, output) in transaction.outputs.into_iter().enumerate() {
                    if instance_to_kind(config, output.module_instance_id()) != kind.as_str() {
                        continue;
                    }
                    let summary = module.summarize_output(&output)?;
                    let meta = ItemMeta {
                        federation_id,
                        module_instance_id: output.module_instance_id(),
                        txid,
                        session_index,
                        item_index,
                        index: index as u64,
                    };
                    module.process_output(ctx, &meta, &output, &summary).await?;
                }
            }
            ConsensusItem::Module(ci) => {
                if instance_to_kind(config, ci.module_instance_id()) != kind.as_str() {
                    continue;
                }
                let meta = CiMeta {
                    federation_id,
                    module_instance_id: ci.module_instance_id(),
                    session_index,
                    item_index,
                    peer: item.peer,
                };
                module.process_ci(ctx, &meta, &ci).await?;
            }
            _ => {
                // Unknown consensus items belong to no module
            }
        }
    }
    Ok(())
}
