//! Wiring of the observer: owns the database pools, guardian and mempool
//! clients and the task group, starts every layer's background tasks, and
//! keeps the registry of observed federations. The layers' code lives in its
//! own files (`ingest.rs`, `projector.rs`, `guardians.rs`, `metadata.rs`,
//! `summary.rs`, `crate::store`, `crate::source`, `crate::recommend`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{ensure, Context};
use fedimint_connectors::ConnectorRegistry;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::Encodable;
use fedimint_core::invite_code::InviteCode;
use fedimint_core::task::TaskGroup;
use fedimint_core::PeerId;
use subtle::ConstantTimeEq;
use tracing::{info_span, Instrument};

use crate::config::FederationConfigCache;
use crate::federation::projector::ProjectorWakeups;
use crate::module::{ModuleRegistry, Services};
use crate::source::SourceRegistry;
use crate::store::db::{self, Federation};
use crate::task::supervise;
use crate::util::{query, query_opt, query_value};

/// Per request to the mempool instance; callers bound their total time
/// themselves
const ESPLORA_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

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
    /// Data sources other than the guardians, see [`crate::source`]
    sources: SourceRegistry,
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
        sources: SourceRegistry,
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
            sources,
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
                version == crate::store::schema::core_schema_version(),
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
        slf.spawn_source_tasks();
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

    pub fn sources(&self) -> &SourceRegistry {
        &self.sources
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

    pub(crate) async fn connection(&self) -> anyhow::Result<deadpool_postgres::Object> {
        Ok(self.connection_pool.get().await?)
    }

    pub async fn list_federations(&self) -> anyhow::Result<Vec<db::Federation>> {
        query(&self.connection().await?, "SELECT * FROM federations", &[]).await
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
}

/// The 404 of a federation id that isn't observed
fn not_observed(federation_id: FederationId) -> anyhow::Error {
    crate::error::ClientError::not_found(format!("Federation {federation_id} is not observed"))
        .into()
}
