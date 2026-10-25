use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, ensure, Context};
use bitcoin::hashes::Hash;
use chrono::{DateTime, NaiveDate};
use deadpool_postgres::{GenericClient, Runtime, Transaction};
use fedimint_connectors::ConnectorRegistry;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::DynModuleConsensusItem;
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::epoch::ConsensusItem;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::session_outcome::SessionOutcome;
use fedimint_core::task::TaskGroup;
use fedimint_core::util::backoff_util::background_backoff;
use fedimint_core::util::retry;
use fedimint_core::{Amount, PeerId};
use fedimint_wallet_common::WalletInput;
use fmo_api_types::{
    FederationActivity, FederationHealth, FederationRating, FederationSummary, FedimintTotals,
};
use futures::StreamExt;
use postgres_from_row::FromRow;
use subtle::ConstantTimeEq;
use tokio::sync::Notify;
use tokio::time::sleep;
use tokio_postgres::NoTls;
use tracing::{debug, info, info_span, warn, Instrument};

use crate::config::meta::{ConsensusMetaCache, MetaFieldsExt};
use crate::config::FederationConfigCache;
use crate::db::{migration, migration_backfill, schema_setup, DbMigration};
use crate::federation::db::Federation;
use crate::federation::{db, instance_to_kind};
use crate::module::{
    schema_name, CiMeta, InputSummary, ItemMeta, ModuleRegistry, OutputSummary, ProcessCtx,
    Services,
};
use crate::task::supervise;
use crate::util::{federation_api, query, query_one, query_opt, query_value};

/// Per request to the mempool instance; callers bound their total time
/// themselves
const ESPLORA_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Federations whose consensus meta the list page fetches at once when it
/// isn't cached yet
const SUMMARY_META_CONCURRENCY: usize = 16;
const NO_ACTIVITY: FederationActivity = FederationActivity {
    num_transactions: 0,
    amount_transferred: Amount::ZERO,
};

#[derive(Debug, Clone)]
pub struct FederationObserver {
    connection_pool: deadpool_postgres::Pool,
    admin_auth: String,
    /// Shared so requests reuse its connections to the mempool instance
    esplora: esplora_client::AsyncClient,
    task_group: TaskGroup,
    consensus_meta_cache: ConsensusMetaCache,
    connectors: ConnectorRegistry,
    /// Configs fetched by invite code, shared with the config API
    federation_config_cache: FederationConfigCache,
    modules: ModuleRegistry,
    nostr_pool: Arc<tokio::sync::OnceCell<nostr_sdk::RelayPool>>,
}

impl FederationObserver {
    pub async fn new(
        database: &str,
        admin_auth: &str,
        mempool_url: &str,
        modules: ModuleRegistry,
    ) -> anyhow::Result<FederationObserver> {
        let connection_pool = {
            let pool_config = deadpool_postgres::Config {
                url: Some(database.to_owned()),
                pool: Some(deadpool_postgres::PoolConfig {
                    // Fail with an error instead of hanging when every
                    // connection is busy or the database doesn't answer
                    timeouts: deadpool_postgres::Timeouts {
                        wait: Some(Duration::from_secs(30)),
                        create: Some(Duration::from_secs(10)),
                        recycle: Some(Duration::from_secs(10)),
                    },
                    ..Default::default()
                }),
                ..Default::default()
            };
            pool_config.create_pool(Some(Runtime::Tokio1), NoTls)
        }?;

        let connectors = ConnectorRegistry::build_from_client_env()?.bind().await?;
        let esplora = esplora_client::Builder::new(mempool_url)
            .timeout(ESPLORA_REQUEST_TIMEOUT.as_secs())
            .build_async()?;

        let slf = FederationObserver {
            connection_pool,
            admin_auth: admin_auth.to_owned(),
            esplora,
            task_group: Default::default(),
            consensus_meta_cache: Default::default(),
            connectors,
            federation_config_cache: Default::default(),
            modules,
            nostr_pool: Default::default(),
        };

        slf.setup_schema().await?;

        for federation in slf.list_federations().await? {
            slf.spawn_observer(federation);
        }

        let observer = slf.clone();
        slf.task_group.spawn_cancellable(
            "fetch block times",
            supervise("Block time fetcher", move || {
                let observer = observer.clone();
                async move {
                    observer.fetch_block_times().await;
                    Ok(())
                }
            }),
        );
        let observer = slf.clone();
        slf.task_group.spawn_cancellable(
            "sync nostr events",
            supervise("Nostr sync", move || observer.clone().sync_nostr_events()),
        );
        let observer = slf.clone();
        slf.task_group.spawn_cancellable(
            "refresh views",
            supervise("View refresher", move || {
                let observer = observer.clone();
                async move {
                    observer.refresh_views().await;
                    Ok(())
                }
            }),
        );
        slf.spawn_module_tasks();

        Ok(slf)
    }

    /// Shared with the API so the list page and the meta endpoints agree and
    /// each federation's consensus meta is fetched once
    pub(crate) fn consensus_meta_cache(&self) -> &ConsensusMetaCache {
        &self.consensus_meta_cache
    }

    pub fn connectors(&self) -> &ConnectorRegistry {
        &self.connectors
    }

    pub fn modules(&self) -> &ModuleRegistry {
        &self.modules
    }

    pub(crate) fn nostr_pool(&self) -> &tokio::sync::OnceCell<nostr_sdk::RelayPool> {
        &self.nostr_pool
    }

    pub(crate) fn federation_config_cache(&self) -> &FederationConfigCache {
        &self.federation_config_cache
    }

    /// Client of the configured mempool instance
    pub(crate) fn esplora(&self) -> &esplora_client::AsyncClient {
        &self.esplora
    }

    fn spawn_observer(&self, federation: Federation) {
        let slf = self.clone();

        let federation_id = federation.federation_id;
        let config = federation.config.clone();
        self.task_group.spawn_cancellable(
            format!("Observer for {}", federation_id),
            supervise("Observer", move || {
                let slf = slf.clone();
                let config = config.clone();
                async move { slf.observe_federation_history(federation_id, config).await }
            })
            .instrument(info_span!("sessions", fed = %federation_id.to_prefix())),
        );

        let slf = self.clone();
        let federation_id = federation.federation_id;
        let config = federation.config.clone();
        self.task_group.spawn_cancellable(
            format!("Health Monitor for {}", federation_id),
            supervise("Health Monitor", move || {
                let slf = slf.clone();
                let config = config.clone();
                async move { slf.monitor_health(federation_id, config).await }
            })
            .instrument(info_span!("health", fed = %federation_id.to_prefix())),
        );

        self.spawn_module_federation_tasks(federation);
    }

    /// Runs the background task each module has for the federation, if any,
    /// supervised like core's own
    fn spawn_module_federation_tasks(&self, federation: Federation) {
        let services = Services::new(self.clone());
        let federation_id = federation.federation_id;
        for module in self.modules.modules() {
            if module
                .federation_task(&services, federation_id, federation.config.clone())
                .is_none()
            {
                continue;
            }
            let kind = module.kind().to_string();
            let services = services.clone();
            let config = federation.config.clone();
            self.task_group.spawn_cancellable(
                format!("{kind} module task for {federation_id}"),
                supervise(format!("{kind} module task"), move || {
                    module
                        .federation_task(&services, federation_id, config.clone())
                        .expect("the module had a task for this federation")
                })
                .instrument(info_span!("module", kind = %kind, fed = %federation_id.to_prefix())),
            );
        }
    }

    /// Runs the background task each module has for all federations, if any
    fn spawn_module_tasks(&self) {
        let services = Services::new(self.clone());
        for module in self.modules.modules() {
            if module.task(&services).is_none() {
                continue;
            }
            let kind = module.kind().to_string();
            let services = services.clone();
            self.task_group.spawn_cancellable(
                format!("{kind} module task"),
                supervise(format!("{kind} module task"), move || {
                    module.task(&services).expect("the module had a task")
                })
                .instrument(info_span!("module", kind = %kind)),
            );
        }
    }

    async fn setup_schema(&self) -> anyhow::Result<()> {
        self.connection()
            .await?
            .batch_execute(
                "
            CREATE OR REPLACE FUNCTION get_max_version() RETURNS INTEGER AS $$
            DECLARE
                max_version INTEGER;
            BEGIN
                IF EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_tables
                    WHERE schemaname = current_schema()
                    AND tablename = 'schema_version'
                ) THEN
                    SELECT COALESCE(MAX(version), 0) INTO max_version
                    FROM schema_version;
                ELSIF EXISTS (
                    SELECT 1
                    FROM pg_catalog.pg_tables
                    WHERE schemaname = current_schema()
                    AND tablename = 'federations'
                ) THEN
                    max_version := 0;
                ELSE
                    max_version := -1;
                END IF;

                RETURN max_version;
            END;
            $$ LANGUAGE plpgsql;
            ",
            )
            .await?;

        let maybe_schema_version =
            match query_value::<i32>(&self.connection().await?, "SELECT get_max_version();", &[])
                .await?
            {
                -1 => {
                    // No schema version table, assume schema is not set up
                    info!("No schema version found, setting up schema");
                    None
                }
                version => Some(version as usize),
            };

        let migrations: &[DbMigration] = &[
            schema_setup!("/schema/v0.sql"),
            schema_setup!("/schema/v1.sql"),
            schema_setup!("/schema/v2.sql"),
            schema_setup!("/schema/v3.sql"),
            schema_setup!("/schema/v4.sql"),
            schema_setup!("/schema/v5.sql"),
            schema_setup!("/schema/v6.sql"),
            migration!("/schema/v7.sql"),
            migration_backfill!(
                "/schema/v8.sql",
                FederationObserver::backfill_reprocess_all_sessions
            ),
            migration!("/schema/v9.sql"),
            migration!("/schema/v10.sql"),
            migration_backfill!(
                "/schema/v11.sql",
                FederationObserver::backfill_peg_in_outpoints
            ),
            migration!("/schema/v12.sql"),
        ];

        ensure!(
            maybe_schema_version.is_none_or(|version| version < migrations.len()),
            "Database schema is newer than this binary; use a compatible observer version"
        );

        // The v8 and v11 backfills reprocess stored sessions with the modules
        // of this build, whose tables now live in the fmo_<kind> schemas that
        // only exist from v12 on. Only an empty database may run them: a
        // populated one must already be at the last version before the move.
        const MIN_UPGRADE_VERSION: usize = 11;
        if let Some(version) = maybe_schema_version {
            ensure!(
                version >= MIN_UPGRADE_VERSION,
                "Database schema is at v{version}, but this version of Fedimint Observer upgrades from v{MIN_UPGRADE_VERSION} or newer; run the previous release first"
            );
        }

        for (index, migration) in migrations.iter().enumerate() {
            if maybe_schema_version.is_none_or(|schema_version| index > schema_version) {
                let mut conn = self.connection().await?;
                let transaction = conn.transaction().await?;
                transaction.batch_execute(migration.sql).await?;

                if let Some(backfill_fn) = &migration.backfill {
                    info!("Running backfill procedure for migration to V{}", index);
                    backfill_fn(self, &transaction).await?;
                } else if maybe_schema_version.is_some() {
                    bail!("Migration V{} does not have a backfill procedure but needs one since the DB is already populated. Please use an older version of Fedimint Observer to run the migration.", index);
                }

                transaction.commit().await?;
            }
        }

        if query_value::<i64>(
            &self.connection().await?,
            "SELECT COUNT(*)::bigint FROM block_times",
            &[],
        )
        .await?
            == 0
        {
            // Seed block times table
            self.connection()
                .await?
                .batch_execute(include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/schema/block_times.sql"
                )))
                .await?;
        }

        self.setup_module_schemas().await
    }

    /// Creates every registered module's schema and runs the module's own
    /// migrations a database hasn't seen, each in its own transaction with the
    /// module's schema first on the search path
    async fn setup_module_schemas(&self) -> anyhow::Result<()> {
        for kind in self.modules.kinds() {
            let module = self.modules.get(kind).expect("listed by the registry");
            let schema = schema_name(kind)?;
            let mut connection = self.connection().await?;
            connection
                .batch_execute(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
                .await?;

            let applied = connection
                .query_opt(
                    "SELECT migrations_applied FROM module_schema_versions WHERE module_kind = $1",
                    &[&kind],
                )
                .await?
                .map_or(0, |row| row.get::<_, i32>(0) as usize);
            let migrations = module.migrations();
            ensure!(
                applied <= migrations.len(),
                "Module {kind} ran {applied} migrations but this binary only knows {}; use a compatible observer version",
                migrations.len()
            );

            for (index, sql) in migrations.iter().enumerate().skip(applied) {
                info!("Running migration {} of module {kind}", index + 1);
                let dbtx = connection.transaction().await?;
                dbtx.batch_execute(&format!("SET LOCAL search_path TO {schema}, public"))
                    .await?;
                dbtx.batch_execute(sql).await?;
                dbtx.execute(
                    "INSERT INTO module_schema_versions (module_kind, migrations_applied)
                     VALUES ($1, $2)
                     ON CONFLICT (module_kind) DO UPDATE
                         SET migrations_applied = EXCLUDED.migrations_applied",
                    &[&kind, &(index as i32 + 1)],
                )
                .await?;
                dbtx.commit().await?;
            }
        }
        Ok(())
    }

    async fn backfill_reprocess_all_sessions(&self, dbtx: &Transaction<'_>) -> anyhow::Result<()> {
        info!("Beginning data backfill needed by schema update, this may take a long time");

        let num_cpus = std::thread::available_parallelism()
            .map(|non_zero_cpus| non_zero_cpus.get())
            .unwrap_or(12);

        for fed in self.list_federations().await? {
            info!(
                "Parsing all session outcomes for fed: {}",
                fed.federation_id
            );
            let decoders = self.modules.decoders_for_config(&fed.config);
            let session_outcome_rows = dbtx
                .query(
                    "SELECT * FROM sessions WHERE federation_id = $1",
                    &[&fed.federation_id.consensus_encode_to_vec()],
                )
                .await?;

            let rows_count = session_outcome_rows.len();

            // take advantage of all cores, otherwise backfilling can take a long time
            let mut parsing_stream =
                futures::stream::iter(session_outcome_rows.into_iter().enumerate())
                    .map(|(row_idx, row)| {
                        let decoders_clone = decoders.clone();
                        tokio::task::spawn(async move {
                            if row_idx % 1000 == 0 {
                                let percentage = (row_idx as f64) / (rows_count as f64);
                                let percentage_str = format!("{:.2}%", percentage * 100.0);
                                info!(
                                    "parsing session index: {:?}/{:?} ({})",
                                    row_idx, rows_count, percentage_str
                                );
                            }
                            db::SessionOutcome::try_from_row_with_decoders(&row, &decoders_clone)
                        })
                    })
                    .buffered(num_cpus)
                    .boxed();

            while let Some(outcome) = parsing_stream.next().await {
                let outcome = outcome??;
                self.process_session(
                    fed.federation_id,
                    &fed.config,
                    outcome.session_index as u64,
                    outcome.data,
                    dbtx,
                )
                .await?;
            }
        }
        Ok(())
    }

    /// Rewrites every peg-in's outpoint from the stored transaction that
    /// brought it in. Some older rows were stored in display byte order, a few
    /// of them twice, and only the original transaction tells which is right.
    async fn backfill_peg_in_outpoints(&self, dbtx: &Transaction<'_>) -> anyhow::Result<()> {
        info!("Rewriting peg-in outpoints from their stored transactions");

        // Read through the migration's transaction: dropping
        // wallet_withdrawal_addresses locks the federations table it references
        // until commit, so another connection would wait on us forever
        for fed in query::<Federation>(dbtx, "SELECT * FROM federations", &[]).await? {
            let decoders = self.modules.decoders_for_config(&fed.config);
            let rows = dbtx
                .query(
                    "SELECT p.on_chain_txid, p.on_chain_vout, p.in_index, t.data
                     FROM wallet_peg_ins p
                     JOIN transactions t ON t.federation_id = p.federation_id AND t.txid = p.txid
                     WHERE p.federation_id = $1",
                    &[&fed.federation_id.consensus_encode_to_vec()],
                )
                .await?;

            let mut rewritten = 0;
            for row in rows {
                let stored_txid = row.get::<_, Vec<u8>>("on_chain_txid");
                let stored_vout = row.get::<_, i32>("on_chain_vout");
                let in_index = row.get::<_, i32>("in_index");
                let transaction =
                    match fedimint_core::transaction::Transaction::consensus_decode_whole(
                        &row.get::<_, Vec<u8>>("data"),
                        &decoders,
                    ) {
                        Ok(transaction) => transaction,
                        Err(error) => {
                            warn!("Skipping peg-in whose transaction cannot be decoded: {error}");
                            continue;
                        }
                    };
                let Some(outpoint) = transaction
                    .inputs
                    .get(in_index as usize)
                    .and_then(|input| input.as_any().downcast_ref::<WalletInput>())
                    .and_then(|input| match input {
                        WalletInput::V0(peg_in_proof) => Some(peg_in_proof.outpoint()),
                        WalletInput::V1(v1) => Some(v1.outpoint),
                        WalletInput::Default { .. } => None,
                    })
                else {
                    warn!("Skipping peg-in whose input {in_index} is not a known wallet input");
                    continue;
                };

                let txid = outpoint.txid.to_byte_array().to_vec();
                let vout = outpoint.vout as i32;
                if txid == stored_txid && vout == stored_vout {
                    continue;
                }
                // The peg-in may already be stored correctly, making this row a duplicate
                let deleted = dbtx
                    .execute(
                        "DELETE FROM wallet_peg_ins
                         WHERE on_chain_txid = $1 AND on_chain_vout = $2
                           AND EXISTS (SELECT 1 FROM wallet_peg_ins WHERE on_chain_txid = $3 AND on_chain_vout = $4)",
                        &[&stored_txid, &stored_vout, &txid, &vout],
                    )
                    .await?;
                if deleted == 0 {
                    dbtx.execute(
                        "UPDATE wallet_peg_ins SET on_chain_txid = $3, on_chain_vout = $4
                         WHERE on_chain_txid = $1 AND on_chain_vout = $2",
                        &[&stored_txid, &stored_vout, &txid, &vout],
                    )
                    .await?;
                }
                rewritten += 1;
            }
            if rewritten > 0 {
                info!(
                    "Rewrote {rewritten} peg-in outpoints for federation {}",
                    fed.federation_id
                );
            }
        }

        dbtx.execute("REFRESH MATERIALIZED VIEW utxos", &[]).await?;
        Ok(())
    }

    pub(crate) async fn connection(&self) -> anyhow::Result<deadpool_postgres::Object> {
        Ok(self.connection_pool.get().await?)
    }

    pub async fn list_federations(&self) -> anyhow::Result<Vec<db::Federation>> {
        query(&self.connection().await?, "SELECT * FROM federations", &[]).await
    }

    /// The federation list. Runs the same few queries however many
    /// federations are observed; only consensus meta not cached yet is fetched
    /// per federation.
    pub async fn list_federation_summaries(&self) -> anyhow::Result<Vec<FederationSummary>> {
        let federations = self.list_federations().await?;
        let federation_health = self.get_guardian_health_summary().await?;
        let assets = self.federation_assets().await?;
        let mut activity = self.federations_activity(7).await?;
        let mut ratings = self.federation_ratings().await?;

        futures::stream::iter(federations)
            .map(|federation| async {
                let name = self
                    .consensus_meta_cache
                    .fetch_meta_cached(&federation.config.to_json(), &self.connectors)
                    .await
                    .and_then(|meta| meta.get_as::<String>("federation_name"))
                    .or_else(|| {
                        federation
                            .config
                            .global
                            .meta
                            .get("federation_name")
                            .cloned()
                    });
                (federation, name)
            })
            .buffered(SUMMARY_META_CONCURRENCY)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .map(|(federation, name)| {
                let federation_id = federation.federation_id;
                let (first_peer_id, first_peer_url) = federation
                    .config
                    .global
                    .api_endpoints
                    .first_key_value()
                    .context("Federation config has no guardians")?;
                let invite = InviteCode::new(
                    first_peer_url.url.clone(),
                    *first_peer_id,
                    federation_id,
                    None,
                )
                .to_string();

                Ok(FederationSummary {
                    id: federation_id,
                    name,
                    last_7d_activity: activity
                        .remove(&federation_id)
                        .unwrap_or_else(|| vec![NO_ACTIVITY; 7]),
                    deposits: assets.get(&federation_id).copied().unwrap_or(Amount::ZERO),
                    invite,
                    nostr_votes: ratings.remove(&federation_id).unwrap_or(FederationRating {
                        count: 0,
                        avg: None,
                    }),
                    health: federation_health
                        .get(&federation_id)
                        .copied()
                        .unwrap_or(FederationHealth::Offline),
                })
            })
            .collect()
    }

    /// Each federation's transactions and volume on each of the last `days`
    /// days, oldest first
    async fn federations_activity(
        &self,
        days: u32,
    ) -> anyhow::Result<HashMap<FederationId, Vec<FederationActivity>>> {
        #[derive(Debug, FromRow)]
        struct ActivityRow {
            federation_id: Vec<u8>,
            day: NaiveDate,
            tx_count: i64,
            volume_msat: i64,
        }

        let today = chrono::Utc::now().date_naive();
        let rows = query::<ActivityRow>(
            &self.connection().await?,
            "SELECT federation_id, day, tx_count, volume_msat
             FROM federation_daily_activity
             WHERE day > $1",
            &[&(today - chrono::Duration::days(days.into()))],
        )
        .await?;

        let mut by_federation = HashMap::<FederationId, HashMap<NaiveDate, ActivityRow>>::new();
        for row in rows {
            by_federation
                .entry(federation_id_from_db(&row.federation_id)?)
                .or_default()
                .insert(row.day, row);
        }
        Ok(by_federation
            .into_iter()
            .map(|(federation_id, days_activity)| {
                let activity = last_n_day_iter(today, days)
                    .map(|day| {
                        days_activity
                            .get(&day)
                            .map_or(NO_ACTIVITY, |row| FederationActivity {
                                num_transactions: row.tx_count as u64,
                                amount_transferred: Amount::from_msats(row.volume_msat as u64),
                            })
                    })
                    .collect();
                (federation_id, activity)
            })
            .collect())
    }

    /// What each federation's wallet holds: all peg-ins minus all peg-outs
    async fn federation_assets(&self) -> anyhow::Result<HashMap<FederationId, Amount>> {
        self.connection()
            .await?
            .query(
                "SELECT federation_id, wallet_in_msat - wallet_out_msat FROM federation_totals",
                &[],
            )
            .await?
            .iter()
            .map(|row| {
                Ok((
                    federation_id_from_db(&row.get::<_, Vec<u8>>(0))?,
                    Amount::from_msats(row.get::<_, i64>(1) as u64),
                ))
            })
            .collect()
    }

    pub async fn get_federation(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Option<Federation>> {
        query_opt(
            &self.connection().await?,
            "SELECT * FROM federations WHERE federation_id = $1",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await
    }

    /// Fails unless the federation is observed. Unlike
    /// [`Self::get_federation`], doesn't decode the federation's config.
    pub async fn ensure_observed(&self, federation_id: FederationId) -> anyhow::Result<()> {
        let observed = query_value::<bool>(
            &self.connection().await?,
            "SELECT EXISTS (SELECT 1 FROM federations WHERE federation_id = $1)",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;
        ensure!(observed, "Federation {federation_id} is not observed");
        Ok(())
    }

    pub async fn add_federation(&self, invite: &InviteCode) -> anyhow::Result<FederationId> {
        let federation_id = invite.federation_id();

        if self.get_federation(federation_id).await?.is_some() {
            return Ok(federation_id);
        }

        let (config, _api) =
            fedimint_api_client::download_from_invite_code(&self.connectors, invite).await?;

        self.connection()
            .await?
            .execute(
                "INSERT INTO federations VALUES ($1, $2)",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &config.consensus_encode_to_vec(),
                ],
            )
            .await?;

        self.spawn_observer(Federation {
            federation_id,
            config,
        });

        Ok(federation_id)
    }

    // FIXME: use middleware for auth and get it out of here
    pub fn check_auth(&self, bearer_token: &str) -> anyhow::Result<()> {
        // Constant time, so response timing doesn't reveal how much of a
        // guessed token is right
        ensure!(
            bool::from(self.admin_auth.as_bytes().ct_eq(bearer_token.as_bytes())),
            "Invalid bearer token"
        );
        Ok(())
    }

    async fn fetch_block_times(self) {
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
        let esplora_client = &self.esplora;

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

    async fn observe_federation_history(
        &self,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        let api = federation_api(&self.connectors, &config.global)?;
        let decoders = self.modules.decoders_for_config(&config);

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
            let mut connection = self.connection().await?;
            let dbtx = connection.transaction().await?;
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
        let ctx = ProcessCtx {
            dbtx,
            config,
            esplora: &self.esplora,
        };

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
            let module = self.modules.get(&kind);
            let summary = match module {
                Some(module) => module.summarize_input(&input)?,
                None => InputSummary::default(),
            };

            dbtx.execute(
                "INSERT INTO transaction_inputs VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &fedimint_txid.consensus_encode_to_vec(),
                    &(in_idx as i32),
                    &kind,
                    &summary.ln_contract_id,
                    &summary.amount_msat.map(|amt| amt as i64),
                ],
            )
            .await?;

            let meta = ItemMeta {
                federation_id,
                module_instance_id: input.module_instance_id(),
                txid: fedimint_txid,
                session_index,
                item_index,
                index: in_idx as u64,
            };
            match module {
                Some(module) => module.process_input(&ctx, &meta, &input, &summary).await?,
                None => warn!("Transaction Input of kind {kind}. Not implemented."),
            }

            if let Some(json_value) = &summary.details {
                dbtx.execute(
                    "INSERT INTO transaction_input_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                    &[
                        &federation_id.consensus_encode_to_vec(),
                        &fedimint_txid.consensus_encode_to_vec(),
                        &(in_idx as i32),
                        &kind,
                        json_value,
                    ],
                )
                .await?;
            }
        }

        for (out_idx, output) in transaction.outputs.into_iter().enumerate() {
            let kind = instance_to_kind(config, output.module_instance_id());
            let module = self.modules.get(&kind);
            let summary = match module {
                Some(module) => module.summarize_output(&output)?,
                None => OutputSummary::default(),
            };

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

            let meta = ItemMeta {
                federation_id,
                module_instance_id: output.module_instance_id(),
                txid: fedimint_txid,
                session_index,
                item_index,
                index: out_idx as u64,
            };
            match module {
                Some(module) => {
                    module
                        .process_output(&ctx, &meta, &output, &summary)
                        .await?
                }
                None => warn!("Transaction Output of kind {kind}. Not implemented."),
            }

            if let Some(json_value) = &summary.details {
                dbtx.execute(
                    "INSERT INTO transaction_output_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                    &[
                        &federation_id.consensus_encode_to_vec(),
                        &fedimint_txid.consensus_encode_to_vec(),
                        &(out_idx as i32),
                        &kind,
                        json_value,
                    ],
                )
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
        let Some(module) = self.modules.get(&kind) else {
            warn!("Consensus Item of kind {kind}. Not implemented.");
            return Ok(());
        };

        if let Some(json_value) = module.summarize_ci(&ci) {
            dbtx.execute(
                "INSERT INTO consensus_items VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
                &[
                    &federation_id.consensus_encode_to_vec(),
                    &(session_index as i32),
                    &(item_index as i32),
                    &(peer_id.to_usize() as i32),
                    &kind,
                    &json_value,
                ],
            )
            .await?;
        }

        let ctx = ProcessCtx {
            dbtx,
            config,
            esplora: &self.esplora,
        };
        let meta = CiMeta {
            federation_id,
            module_instance_id: ci.module_instance_id(),
            session_index,
            item_index,
            peer: peer_id,
        };
        module.process_ci(&ctx, &meta, &ci).await
    }

    async fn refresh_views(self) {
        loop {
            let start = Instant::now();
            debug!("Refreshing views...");
            if let Err(e) = self.refresh_views_inner().await {
                warn!("Error while refreshing views: {e:?}");
            }
            let elapsed = start.elapsed().as_secs_f64();
            info!("Views refresh completed in {elapsed:.2}s. Waiting for next refresh window");
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    /// Refreshes the materialized views the modules declare
    async fn refresh_views_inner(&self) -> anyhow::Result<()> {
        let connection = self.connection().await?;
        for kind in self.modules.kinds() {
            let module = self.modules.get(kind).expect("listed by the registry");
            for view in module.materialized_views() {
                connection
                    .batch_execute(&format!("REFRESH MATERIALIZED VIEW CONCURRENTLY {view}"))
                    .await
                    .with_context(|| format!("refreshing {view} of module {kind}"))?;
            }
        }

        Ok(())
    }

    pub async fn get_federation_assets(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Amount> {
        let total_assets_msat = query_value::<i64>(
            &self.connection().await?,
            "SELECT COALESCE(
                 (SELECT wallet_in_msat - wallet_out_msat FROM federation_totals WHERE federation_id = $1),
                 0
             )",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        Ok(Amount::from_msats(total_assets_msat as u64))
    }

    pub async fn totals(&self) -> anyhow::Result<FedimintTotals> {
        #[derive(Debug, FromRow)]
        struct FedimintTotalsResult {
            federations: i64,
            tx_count: i64,
            tx_volume: i64,
        }

        let offline_federations = self
            .get_guardian_health_summary()
            .await?
            .values()
            .filter(|&health| *health == FederationHealth::Offline)
            .count() as u64;

        // Reads the counters maintained by the triggers introduced in v9 instead of
        // aggregating over `transactions` and `transaction_inputs`, which meant a full
        // scan of both tables (~1.3GB) on every request.
        let totals = query_one::<FedimintTotalsResult>(
            &self.connection().await?,
            // language=postgresql
            "
                SELECT (SELECT count(*) from federations)::bigint as federations,
                       COALESCE(sum(tx_count), 0)::bigint         as tx_count,
                       COALESCE(sum(tx_volume_msat), 0)::bigint   as tx_volume
                FROM federation_totals
            ",
            &[],
        )
        .await?;

        Ok(FedimintTotals {
            federations: (totals.federations as u64) - offline_federations,
            tx_count: totals.tx_count as u64,
            tx_volume: Amount::from_msats(totals.tx_volume as u64),
        })
    }

    /// Reprocesses stored sessions of a federation, filling in rows that are
    /// missing. Commits every `BACKFILL_BATCH` sessions, so an interrupted run
    /// keeps its progress.
    pub async fn backfill_federation(
        &self,
        federation_id: FederationId,
        session_start: Option<i32>,
        session_end: Option<i32>,
    ) -> anyhow::Result<()> {
        const BACKFILL_BATCH: i32 = 1000;

        let federation = self
            .get_federation(federation_id)
            .await?
            .with_context(|| format!("Federation {federation_id} is not observed"))?;
        let session_start = session_start.unwrap_or(0);
        let Some(last_session) = query_value::<Option<i32>>(
            &self.connection().await?,
            "SELECT MAX(session_index) FROM sessions WHERE federation_id = $1",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?
        else {
            info!("Federation {federation_id} has no stored sessions to backfill");
            return Ok(());
        };
        let session_end = session_end.unwrap_or(last_session).min(last_session);
        ensure!(
            session_start <= session_end,
            "session_start {session_start} is after session_end {session_end}"
        );

        info!("Backfilling federation {federation_id}, sessions {session_start} to {session_end}");
        let decoders = self.modules.decoders_for_config(&federation.config);
        let mut batch_start = session_start;
        let mut processed = 0;
        loop {
            let batch_end = batch_start
                .saturating_add(BACKFILL_BATCH - 1)
                .min(session_end);
            let mut connection = self.connection().await?;
            let dbtx = connection.transaction().await?;
            let rows = dbtx
                .query(
                    "SELECT * FROM sessions WHERE federation_id = $1 AND session_index BETWEEN $2 AND $3 ORDER BY session_index",
                    &[&federation_id.consensus_encode_to_vec(), &batch_start, &batch_end],
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
            dbtx.commit().await?;
            info!("Backfilled up to session {batch_end} ({processed} sessions)");

            if batch_end >= session_end {
                break;
            }
            batch_start = batch_end + 1;
        }

        // Restored votes can move session times, and restored transactions
        // change daily activity
        info!("Rebuilding read models of federation {federation_id} from session {session_start}");
        self.connection()
            .await?
            .execute(
                "SELECT rebuild_read_models($1, $2)",
                &[&federation_id.consensus_encode_to_vec(), &session_start],
            )
            .await?;

        info!("Backfill complete!");
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

/// Logs how far a sync got and how fast, at most every 5 s
struct SyncProgress {
    unit: &'static str,
    logged_at: Instant,
    logged_position: u64,
}

impl SyncProgress {
    fn new(unit: &'static str, position: u64) -> Self {
        SyncProgress {
            unit,
            logged_at: Instant::now(),
            logged_position: position,
        }
    }

    fn record(&mut self, position: u64) {
        let elapsed = self.logged_at.elapsed();
        if elapsed < Duration::from_secs(5) {
            return;
        }
        let unit = self.unit;
        let synced = position.saturating_sub(self.logged_position);
        let rate = synced as f64 / elapsed.as_secs_f64();
        info!("Synced up to {unit} {position}, processed {synced} {unit}s at a rate of {rate:.2} {unit}s/s");
        self.logged_at = Instant::now();
        self.logged_position = position;
    }
}

pub(crate) fn federation_id_from_db(bytes: &[u8]) -> anyhow::Result<FederationId> {
    Ok(FederationId::consensus_decode_whole(
        bytes,
        &Default::default(),
    )?)
}

fn last_n_day_iter(now: NaiveDate, days: u32) -> impl Iterator<Item = NaiveDate> {
    (0..days)
        .rev()
        .map(move |day| now - chrono::Duration::days(day as i64))
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use crate::federation::observer::{last_n_day_iter, SessionFetchGate};

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
    fn test_day_iter() {
        let now = chrono::offset::Utc::now().date_naive();
        let days = 7;
        let last_7_days = last_n_day_iter(now, days).collect::<Vec<_>>();
        assert_eq!(last_7_days.len(), days as usize);
        assert_eq!(last_7_days[6], now);
        assert_eq!(last_7_days[0], now - chrono::Duration::days(6));
    }
}
