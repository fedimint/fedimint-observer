use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{ensure, Context};
use chrono::{DateTime, NaiveDate};
use deadpool_postgres::{GenericClient, Transaction};
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
use fedimint_core::{Amount, PeerId, TransactionId};
use fmo_api_types::{
    FederationActivity, FederationHealth, FederationRating, FederationSummary, FedimintTotals,
};
use futures::StreamExt;
use postgres_from_row::FromRow;
use subtle::ConstantTimeEq;
use tokio::sync::Notify;
use tokio::time::sleep;
use tracing::{debug, info, info_span, warn, Instrument};

use crate::config::meta::{MetaFields, MetaFieldsExt};
use crate::config::FederationConfigCache;
use crate::federation::db::Federation;
use crate::federation::projector::ProjectorWakeups;
use crate::federation::{db, instance_to_kind};
use crate::module::{schema_name, ModuleRegistry, Services};
use crate::task::supervise;
use crate::util::{query, query_one, query_opt, query_value};

/// Per request to the mempool instance; callers bound their total time
/// themselves
const ESPLORA_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
// Older SQL is retained for new databases; populated databases upgrade from
// v11.
const CORE_MIGRATIONS: &[&str] = &[
    include_str!("../../schema/v0.sql"),
    include_str!("../../schema/v1.sql"),
    include_str!("../../schema/v2.sql"),
    include_str!("../../schema/v3.sql"),
    include_str!("../../schema/v4.sql"),
    include_str!("../../schema/v5.sql"),
    include_str!("../../schema/v6.sql"),
    include_str!("../../schema/v7.sql"),
    include_str!("../../schema/v8.sql"),
    include_str!("../../schema/v9.sql"),
    include_str!("../../schema/v10.sql"),
    include_str!("../../schema/v11.sql"),
    include_str!("../../schema/v12.sql"),
];
const NO_ACTIVITY: FederationActivity = FederationActivity {
    num_transactions: 0,
    amount_transferred: Amount::ZERO,
};

/// Distinct guardian endpoint sets [`FederationObserver::federation_api`]
/// keeps clients for: observed federations
const MAX_OBSERVED_API_CLIENTS: usize = 4096;

/// Distinct guardian endpoint sets [`FederationObserver::lookup_api`] keeps
/// clients for. Public invite lookups can name any servers, so they get their
/// own budget and can't crowd out observed federations.
const MAX_LOOKUP_API_CLIENTS: usize = 512;

type ApiPeers = std::collections::BTreeMap<PeerId, fedimint_core::util::SafeUrl>;

/// Shared guardian API clients, see [`FederationObserver::federation_api`]
#[derive(Default, Debug)]
struct ApiClients {
    /// Observed federations, which only admins add
    observed: HashMap<ApiPeers, fedimint_api_client::api::DynGlobalApi>,
    /// Invite lookups, by endpoints and the invite's API secret
    lookups: HashMap<(ApiPeers, Option<String>), fedimint_api_client::api::DynGlobalApi>,
}

fn api_peers(global: &fedimint_core::config::GlobalClientConfig) -> ApiPeers {
    global
        .api_endpoints
        .iter()
        .map(|(&peer_id, endpoint)| (peer_id, endpoint.url.clone()))
        .collect()
}

#[derive(Debug, Clone)]
pub struct FederationObserver {
    connection_pool: deadpool_postgres::Pool,
    worker_pool: deadpool_postgres::Pool,
    worker_lease: Option<Arc<tokio_postgres::Client>>,
    pub(crate) work_budget: Arc<tokio::sync::Semaphore>,
    pub(crate) external_io: Arc<tokio::sync::Semaphore>,
    pub(crate) work_notifications: crate::work::WorkNotifications,
    /// `FO_HEALTH_RETENTION_DAYS`; zero keeps health history indefinitely
    pub(crate) health_retention_days: u32,
    admin_auth: String,
    /// Shared so requests reuse its connections to the mempool instance
    esplora: esplora_client::AsyncClient,
    task_group: TaskGroup,
    connectors: ConnectorRegistry,
    /// Configs fetched by invite code, shared with the config API
    federation_config_cache: FederationConfigCache,
    /// One guardian API client per set of guardian endpoints, see
    /// [`Self::federation_api`]
    api_clients: Arc<std::sync::Mutex<ApiClients>>,
    modules: ModuleRegistry,
    /// Tells a federation's module projectors about stored sessions and moved
    /// cursors
    projector_wakeups: ProjectorWakeups,
    nostr_pool: Arc<tokio::sync::OnceCell<nostr_sdk::RelayPool>>,
    /// Federations whose observer tasks this process runs, see
    /// [`Self::spawn_observer`]
    spawned_federations: Arc<std::sync::Mutex<std::collections::HashSet<FederationId>>>,
}

impl FederationObserver {
    pub async fn new(
        database: &str,
        admin_auth: &str,
        mempool_url: &str,
        modules: ModuleRegistry,
        options: &crate::runtime::RuntimeOptions,
    ) -> anyhow::Result<FederationObserver> {
        let connection_pool = crate::runtime::pool(database, options.worker_db_connections, true)?;
        let api_pool = crate::runtime::pool(database, options.api_db_connections, false)?;
        let task_group = TaskGroup::default();
        let work_notifications = crate::work::WorkNotifications::default();
        let worker_lease = if options.api_only {
            None
        } else {
            Some(
                crate::runtime::worker_lease(
                    database,
                    task_group.clone(),
                    work_notifications.clone(),
                )
                .await?,
            )
        };

        let connectors = ConnectorRegistry::build_from_client_env()?.bind().await?;
        let esplora = esplora_client::Builder::new(mempool_url)
            .timeout(ESPLORA_REQUEST_TIMEOUT.as_secs())
            .build_async()?;

        let mut slf = FederationObserver {
            worker_pool: connection_pool.clone(),
            connection_pool,
            worker_lease,
            work_budget: Arc::new(tokio::sync::Semaphore::new(options.worker_concurrency)),
            external_io: Arc::new(tokio::sync::Semaphore::new(options.external_concurrency)),
            work_notifications,
            health_retention_days: options.health_retention_days,
            admin_auth: admin_auth.to_owned(),
            esplora,
            task_group,
            connectors,
            federation_config_cache: Default::default(),
            api_clients: Default::default(),
            modules,
            projector_wakeups: Default::default(),
            nostr_pool: Default::default(),
            spawned_federations: Default::default(),
        };

        if options.api_only {
            slf.connection_pool = api_pool;
            let version: i32 = query_value(
                &slf.connection().await?,
                "SELECT MAX(version) FROM schema_version",
                &[],
            )
            .await?;
            ensure!(
                version == CORE_MIGRATIONS.len() as i32 - 1,
                "Start the worker to migrate the database before API replicas"
            );
            return Ok(slf);
        }
        // The dedicated worker lease serializes startup migrations as well as
        // collection. An API-only instance never runs migrations.
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
            "repair session times",
            supervise("Session time repair", move || {
                observer.clone().repair_session_times()
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
        let observer = slf.clone();
        slf.task_group.spawn_cancellable(
            "metadata snapshots",
            supervise("Metadata snapshots", move || {
                observer.clone().refresh_metadata()
            }),
        );

        if options.health_retention_days > 0 {
            let observer = slf.clone();
            let days = options.health_retention_days;
            slf.task_group.spawn_cancellable(
                "health retention",
                supervise("Health retention", move || {
                    observer.clone().prune_health(days)
                }),
            );
        }
        slf.connection_pool = api_pool;

        Ok(slf)
    }

    pub fn connectors(&self) -> &ConnectorRegistry {
        &self.connectors
    }

    /// The shared client for the API of the guardians of an observed
    /// federation.
    ///
    /// Every new client opens its own connection to each guardian, and the
    /// connection pool keeps those connections, so they are never closed while
    /// the guardian keeps them alive. Creating a client per request therefore
    /// leaks connections on both sides; this hands out one client per set of
    /// endpoints for the life of the process. For federations that aren't
    /// observed, use [`Self::lookup_api`].
    pub fn federation_api(
        &self,
        global: &fedimint_core::config::GlobalClientConfig,
    ) -> anyhow::Result<fedimint_api_client::api::DynGlobalApi> {
        let peers = api_peers(global);
        let mut clients = self
            .api_clients
            .lock()
            .expect("API client cache lock poisoned");
        if let Some(api) = clients.observed.get(&peers) {
            return Ok(api.clone());
        }
        if clients.observed.len() >= MAX_OBSERVED_API_CLIENTS {
            return Err(crate::error::ClientError::unavailable(
                "Too many federations observed; try again after the observer restarts",
            )
            .into());
        }
        let api = crate::util::federation_api(&self.connectors, &peers, None)?;
        clients.observed.insert(peers, api.clone());
        Ok(api)
    }

    /// The shared client for guardians named by an invite code, of a
    /// federation that may not be observed. Like [`Self::federation_api`], but
    /// with the invite's API secret and a separate, smaller budget, since
    /// anyone can ask to look up any servers.
    pub fn lookup_api(
        &self,
        global: &fedimint_core::config::GlobalClientConfig,
        api_secret: Option<String>,
    ) -> anyhow::Result<fedimint_api_client::api::DynGlobalApi> {
        let peers = api_peers(global);
        let mut clients = self
            .api_clients
            .lock()
            .expect("API client cache lock poisoned");
        if api_secret.is_none() {
            if let Some(api) = clients.observed.get(&peers) {
                return Ok(api.clone());
            }
        }
        let key = (peers, api_secret);
        if let Some(api) = clients.lookups.get(&key) {
            return Ok(api.clone());
        }
        if clients.lookups.len() >= MAX_LOOKUP_API_CLIENTS {
            return Err(crate::error::ClientError::unavailable(
                "Too many federations looked up; try again after the observer restarts",
            )
            .into());
        }
        let api = crate::util::federation_api(&self.connectors, &key.0, key.1.clone())?;
        clients.lookups.insert(key, api.clone());
        Ok(api)
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

    pub(crate) fn task_group(&self) -> &TaskGroup {
        &self.task_group
    }

    pub(super) fn projector_wakeups(&self) -> &ProjectorWakeups {
        &self.projector_wakeups
    }

    pub(crate) fn worker(&self) -> Self {
        let mut worker = self.clone();
        worker.connection_pool = self.worker_pool.clone();
        worker
    }

    pub(crate) fn healthy(&self) -> bool {
        !self.task_group.is_shutting_down()
            && self
                .worker_lease
                .as_ref()
                .is_none_or(|client| !client.is_closed())
    }

    pub(crate) async fn shutdown(&self) -> anyhow::Result<()> {
        self.task_group
            .clone()
            .shutdown_join_all(Some(Duration::from_secs(10)))
            .await
    }

    pub(crate) fn pool_status(&self) -> (deadpool_postgres::Status, deadpool_postgres::Status) {
        (self.connection_pool.status(), self.worker_pool.status())
    }

    /// Starts the federation's tasks, unless this process already runs them
    fn spawn_observer(&self, federation: Federation) {
        if !self
            .spawned_federations
            .lock()
            .expect("spawned federations lock poisoned")
            .insert(federation.federation_id)
        {
            return;
        }
        let slf = self.worker();

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

        let slf = self.worker();
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

        self.spawn_projectors(&federation);
        self.spawn_module_federation_tasks(federation);
    }

    /// Runs the background task each module has for the federation, if any,
    /// supervised like core's own
    fn spawn_module_federation_tasks(&self, federation: Federation) {
        let services = Services::new(self.worker());
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

    /// Whether the v12 database has `session_time_repairs`, which the
    /// pre-release v12 lacked
    async fn has_session_time_repair_schema(&self) -> anyhow::Result<bool> {
        query_value(
            &self.connection().await?,
            "SELECT to_regclass('session_time_repairs') IS NOT NULL",
            &[],
        )
        .await
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

        // Databases that applied the pre-release v12..v14 of this branch,
        // since merged into one v12. A pre-release v12 lacks the
        // session_time_repairs table that v12 now creates.
        let pre_release = match maybe_schema_version {
            Some(13 | 14) => true,
            Some(12) => !self.has_session_time_repair_schema().await?,
            _ => false,
        };
        ensure!(
            !pre_release,
            "Database schema is from a pre-release build whose v12..v14 migrations were since merged into v12; restore the database from a v11 backup or start a new one"
        );
        ensure!(
            maybe_schema_version.is_none_or(|version| version < CORE_MIGRATIONS.len()),
            "Database schema is newer than this binary; use a compatible observer version"
        );

        // Earlier versions require Rust backfills from the previous release.
        const MIN_UPGRADE_VERSION: usize = 11;
        if let Some(version) = maybe_schema_version {
            ensure!(
                version >= MIN_UPGRADE_VERSION,
                "Database schema is at v{version}, but this version of Fedimint Observer upgrades from v{MIN_UPGRADE_VERSION} or newer; run the previous release first"
            );
        }

        for (index, sql) in CORE_MIGRATIONS.iter().enumerate() {
            if maybe_schema_version.is_none_or(|schema_version| index > schema_version) {
                let mut conn = self.connection().await?;
                let transaction = conn.transaction().await?;
                transaction
                    .batch_execute(
                        "SET LOCAL statement_timeout = 0; SET LOCAL lock_timeout = '30s'",
                    )
                    .await?;
                info!("Applying core schema v{index}");
                transaction.batch_execute(sql).await?;
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
                // Like core migrations: no statement deadline, a bounded lock wait
                dbtx.batch_execute(&format!(
                    "SET LOCAL statement_timeout = 0; SET LOCAL lock_timeout = '30s';
                     SET LOCAL search_path TO {schema}, public"
                ))
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

    pub(crate) async fn connection(&self) -> anyhow::Result<deadpool_postgres::Object> {
        Ok(self.connection_pool.get().await?)
    }

    pub async fn list_federations(&self) -> anyhow::Result<Vec<db::Federation>> {
        query(&self.connection().await?, "SELECT * FROM federations", &[]).await
    }

    /// The federation list uses a fixed number of database queries and no
    /// remote I/O.
    pub async fn list_federation_summaries(&self) -> anyhow::Result<Vec<FederationSummary>> {
        let federations = self.list_federations().await?;
        let federation_health = self.get_guardian_health_summary().await?;
        let assets = self.federation_assets().await?;
        let snapshots = self.asset_snapshots().await?;
        let mut activity = self.federations_activity(7).await?;
        let mut ratings = self.federation_ratings().await?;

        let mut metas = self
            .connection()
            .await?
            .query(
                "SELECT federation_id, data FROM federation_observations WHERE kind = 'meta'",
                &[],
            )
            .await?
            .into_iter()
            .map(|row| {
                Ok((
                    federation_id_from_db(&row.get::<_, Vec<u8>>(0))?,
                    serde_json::from_value::<MetaFields>(row.get(1))?,
                ))
            })
            .collect::<anyhow::Result<HashMap<_, _>>>()?;
        federations
            .into_iter()
            .map(|federation| {
                let meta = metas.remove(&federation.federation_id);
                let name = meta
                    .as_ref()
                    .and_then(|meta| meta.get_as::<String>("federation_name"))
                    .or_else(|| {
                        federation
                            .config
                            .global
                            .meta
                            .get("federation_name")
                            .cloned()
                    });
                let expiry_timestamp = meta
                    .as_ref()
                    .and_then(|meta| meta.get_as::<u64>("federation_expiry_timestamp"))
                    .or_else(|| {
                        federation
                            .config
                            .global
                            .meta
                            .get("federation_expiry_timestamp")
                            .and_then(|value| value.parse().ok())
                    });
                let successor = meta
                    .as_ref()
                    .and_then(|meta| meta.get_as::<String>("federation_successor"))
                    .or_else(|| {
                        federation
                            .config
                            .global
                            .meta
                            .get("federation_successor")
                            .cloned()
                    });
                (federation, name, expiry_timestamp, successor)
            })
            .map(|(federation, name, expiry_timestamp, successor)| {
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
                    deposits: self.combined_assets(&federation, &assets, &snapshots)?,
                    invite,
                    nostr_votes: ratings.remove(&federation_id).unwrap_or(FederationRating {
                        count: 0,
                        avg: None,
                    }),
                    health: federation_health
                        .get(&federation_id)
                        .copied()
                        .unwrap_or(FederationHealth::Offline),
                    expiry_timestamp,
                    // A successor that is not an invite code, or names this
                    // federation itself, points nowhere users can move to
                    successor_id: successor
                        .as_deref()
                        .and_then(|successor| successor.trim().parse::<InviteCode>().ok())
                        .map(|invite| invite.federation_id())
                        .filter(|successor_id| *successor_id != federation_id),
                    successor,
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
                    Amount::from_msats(
                        u64::try_from(row.get::<_, i64>(1)).context("Negative wallet balance")?,
                    ),
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
        if !observed {
            return Err(not_observed(federation_id));
        }
        Ok(())
    }

    /// An observed federation; a 404 for any other id
    pub async fn observed_federation(
        &self,
        federation_id: FederationId,
    ) -> anyhow::Result<Federation> {
        self.get_federation(federation_id)
            .await?
            .ok_or_else(|| not_observed(federation_id))
    }

    /// Stores the federation and starts observing it. Adding a federation
    /// again, or concurrently, is fine: it is stored once, and observed once
    /// per process.
    pub async fn add_federation(&self, invite: &InviteCode) -> anyhow::Result<FederationId> {
        let federation_id = invite.federation_id();

        if let Some(federation) = self.get_federation(federation_id).await? {
            // Its observer may not run yet, if the request that stored it was
            // cancelled before starting it
            self.spawn_observer(federation);
            return Ok(federation_id);
        }

        // Observing uses the shared clients, which carry no API secret
        if invite.api_secret().is_some() {
            return Err(crate::error::ClientError::bad_request(
                "Federations whose invite code has an API secret can't be observed",
            )
            .into());
        }
        let config = crate::config::download_client_config(invite, self).await?;

        // Stored and started apart from the request, so a request cancelled at
        // its deadline can't leave a stored federation unobserved
        let observer = self.clone();
        tokio::spawn(async move {
            observer
                .connection()
                .await?
                .execute(
                    "INSERT INTO federations VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[
                        &federation_id.consensus_encode_to_vec(),
                        &config.consensus_encode_to_vec(),
                    ],
                )
                .await?;
            // A concurrent add may have stored it first
            let federation = observer
                .get_federation(federation_id)
                .await?
                .context("Stored federation is missing")?;
            observer.spawn_observer(federation);
            anyhow::Ok(())
        })
        .await??;

        Ok(federation_id)
    }

    // FIXME: use middleware for auth and get it out of here
    pub fn check_auth(&self, bearer_token: &str) -> crate::error::Result<()> {
        // Constant time, so response timing doesn't reveal how much of a
        // guessed token is right
        if !bool::from(self.admin_auth.as_bytes().ct_eq(bearer_token.as_bytes())) {
            return Err(crate::error::AppError::unauthorized());
        }
        if self.worker_lease.is_none() {
            return Err(crate::error::AppError::unavailable(
                "Administrative operations must be sent to the worker instance",
            ));
        }
        Ok(())
    }

    /// A repair's durable request is deleted once its rebuild is done, so a
    /// crash or database error leaves it available for the next attempt.
    async fn repair_session_times(self) -> anyhow::Result<()> {
        let mut wakeup = self.work_notifications.subscribe("session-time-repair");
        loop {
            // Mark the generation before querying, retaining commits racing the query.
            wakeup.borrow_and_update();
            let permit = self.work_budget.acquire().await?;
            let repaired = self.process_session_time_repair().await?;
            drop(permit);
            if repaired {
                continue;
            }
            wakeup.changed().await?;
        }
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
        // Shared, so the supervisor restarting this task doesn't leak
        // guardian connections
        let api = self.federation_api(&config.global)?;
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
            self.projector_wakeups.wake(federation_id);
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
                self.modules
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
                self.modules
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
        let Some(module) = self.modules.get(&kind) else {
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
    ) -> anyhow::Result<Option<Amount>> {
        let federation = self.observed_federation(federation_id).await?;
        let services = Services::new(self.clone());
        let total_assets_msat = query_value::<i64>(
            &self.connection().await?,
            "SELECT COALESCE(
                 (SELECT wallet_in_msat - wallet_out_msat FROM federation_totals WHERE federation_id = $1),
                 0
             )",
            &[&federation_id.consensus_encode_to_vec()],
        )
        .await?;

        let mut amount = Amount::from_msats(
            u64::try_from(total_assets_msat).context("Negative wallet balance")?,
        );
        for module in self
            .modules
            .modules()
            .filter(|module| module.reports_assets())
        {
            let kind = module.kind().to_string();
            if crate::util::module_instance_id(&federation.config, &kind).is_some() {
                let Some((value, observed_at)) = services
                    .observation(federation_id, &format!("assets:{kind}"))
                    .await?
                else {
                    return Ok(None);
                };
                if chrono::Utc::now().signed_duration_since(observed_at)
                    > chrono::Duration::minutes(5)
                {
                    return Ok(None);
                }
                amount = amount
                    .checked_add(Amount::from_msats(
                        value.as_u64().context("Invalid asset snapshot")?,
                    ))
                    .context("Federation asset total overflow")?;
            }
        }
        Ok(Some(amount))
    }

    async fn asset_snapshots(&self) -> anyhow::Result<HashMap<(FederationId, String), Amount>> {
        self.connection().await?.query(
            "SELECT federation_id, kind, data FROM federation_observations WHERE kind LIKE 'assets:%' AND observed_at >= now() - INTERVAL '5 minutes'", &[]
        ).await?.into_iter().map(|row| {
            let value: serde_json::Value = row.get(2);
            Ok(((federation_id_from_db(&row.get::<_, Vec<u8>>(0))?, row.get(1)),
                Amount::from_msats(value.as_u64().context("Invalid asset snapshot")?)))
        }).collect()
    }

    fn combined_assets(
        &self,
        federation: &Federation,
        legacy: &HashMap<FederationId, Amount>,
        snapshots: &HashMap<(FederationId, String), Amount>,
    ) -> anyhow::Result<Option<Amount>> {
        let mut total = legacy
            .get(&federation.federation_id)
            .copied()
            .unwrap_or(Amount::ZERO);
        for module in self
            .modules
            .modules()
            .filter(|module| module.reports_assets())
        {
            let kind = module.kind().to_string();
            if crate::util::module_instance_id(&federation.config, &kind).is_some() {
                let Some(amount) =
                    snapshots.get(&(federation.federation_id, format!("assets:{kind}")))
                else {
                    return Ok(None);
                };
                total = total
                    .checked_add(*amount)
                    .context("Federation asset total overflow")?;
            }
        }
        Ok(Some(total))
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
        self.task_group.spawn_cancellable(
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
        let decoders = self.modules.decoders_for_config(&federation.config);
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

/// Logs how far a sync got and how fast, at most every 5 s
pub(super) struct SyncProgress {
    unit: &'static str,
    logged_at: Instant,
    logged_position: u64,
}

impl SyncProgress {
    pub(super) fn new(unit: &'static str, position: u64) -> Self {
        SyncProgress {
            unit,
            logged_at: Instant::now(),
            logged_position: position,
        }
    }

    pub(super) fn record(&mut self, position: u64) {
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

// Core rows written from a module's summary of an item. Ingest writes them as
// it stores a session; the module's projector writes the ones of items stored
// while the build lacked the module (projector.rs).

pub(super) async fn insert_input_details(
    dbtx: &Transaction<'_>,
    federation_id: FederationId,
    txid: TransactionId,
    in_index: usize,
    kind: &str,
    details: &serde_json::Value,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO transaction_input_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &federation_id.consensus_encode_to_vec(),
            &txid.consensus_encode_to_vec(),
            &(in_index as i32),
            &kind,
            details,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn insert_output_details(
    dbtx: &Transaction<'_>,
    federation_id: FederationId,
    txid: TransactionId,
    out_index: usize,
    kind: &str,
    details: &serde_json::Value,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO transaction_output_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &federation_id.consensus_encode_to_vec(),
            &txid.consensus_encode_to_vec(),
            &(out_index as i32),
            &kind,
            details,
        ],
    )
    .await?;
    Ok(())
}

/// Where a module consensus item sits in a federation's history
pub(super) struct CiPosition {
    pub(super) federation_id: FederationId,
    pub(super) session_index: u64,
    pub(super) item_index: u64,
    pub(super) peer: PeerId,
}

pub(super) async fn insert_consensus_item(
    dbtx: &Transaction<'_>,
    at: &CiPosition,
    kind: &str,
    data: &serde_json::Value,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO consensus_items VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
        &[
            &at.federation_id.consensus_encode_to_vec(),
            &(at.session_index as i32),
            &(at.item_index as i32),
            &(at.peer.to_usize() as i32),
            &kind,
            data,
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn insert_block_height_vote(
    dbtx: &Transaction<'_>,
    at: &CiPosition,
    height: u32,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO block_height_votes VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &at.federation_id.consensus_encode_to_vec(),
            &(at.session_index as i32),
            &(at.item_index as i32),
            &(at.peer.to_usize() as i32),
            &(height as i32),
        ],
    )
    .await?;
    Ok(())
}

pub(super) async fn insert_unix_time_vote(
    dbtx: &Transaction<'_>,
    at: &CiPosition,
    time: u64,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO unix_time_votes VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &at.federation_id.consensus_encode_to_vec(),
            &(at.session_index as i32),
            &(at.item_index as i32),
            &(at.peer.to_usize() as i32),
            &(time as i64),
        ],
    )
    .await?;
    Ok(())
}

/// The 404 of a federation id that isn't observed
fn not_observed(federation_id: FederationId) -> anyhow::Error {
    crate::error::ClientError::not_found(format!("Federation {federation_id} is not observed"))
        .into()
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

    use crate::federation::observer::{last_n_day_iter, summary_or_default, SessionFetchGate};
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
