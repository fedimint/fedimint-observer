//! Deployment limits and database ownership. API replicas never start
//! collectors.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use deadpool_postgres::Runtime;
use postgres_native_tls::MakeTlsConnector;

#[derive(clap::Args, Debug, Clone)]
pub struct RuntimeOptions {
    #[arg(long, env = "FO_API_ONLY", default_value_t = false)]
    pub api_only: bool,
    #[arg(long, env = "FO_API_DB_CONNECTIONS", default_value_t = 16)]
    pub api_db_connections: usize,
    #[arg(long, env = "FO_WORKER_DB_CONNECTIONS", default_value_t = 8)]
    pub worker_db_connections: usize,
    #[arg(long, env = "FO_WORKER_CONCURRENCY", default_value_t = 4)]
    pub worker_concurrency: usize,
    #[arg(long, env = "FO_EXTERNAL_CONCURRENCY", default_value_t = 8)]
    pub external_concurrency: usize,
    #[arg(long, env = "FO_HTTP_CONCURRENCY", default_value_t = 128)]
    pub http_concurrency: usize,
    /// Zero retains raw health indefinitely. Otherwise keep at least 31 days.
    #[arg(long, env = "FO_HEALTH_RETENTION_DAYS", default_value_t = 90)]
    pub health_retention_days: u32,
}

impl RuntimeOptions {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.api_db_connections > 0
                && self.worker_db_connections > 0
                && self.worker_concurrency > 0
                && self.external_concurrency > 0
                && self.http_concurrency > 0,
            "Connection and concurrency limits must be positive"
        );
        anyhow::ensure!(
            self.worker_concurrency <= self.worker_db_connections,
            "Worker concurrency must not exceed the worker pool size"
        );
        anyhow::ensure!(
            self.health_retention_days == 0 || (31..=36500).contains(&self.health_retention_days),
            "Health retention must be zero or between 31 and 36500 days"
        );
        Ok(())
    }
}

pub(crate) fn tls() -> anyhow::Result<MakeTlsConnector> {
    Ok(MakeTlsConnector::new(native_tls::TlsConnector::new()?))
}

pub(crate) fn pool(
    database: &str,
    size: usize,
    worker: bool,
) -> anyhow::Result<deadpool_postgres::Pool> {
    let config = deadpool_postgres::Config {
        url: Some(database.to_owned()),
        application_name: Some(if worker { "fmo-worker" } else { "fmo-api" }.to_owned()),
        options: Some(format!("-c statement_timeout={} -c idle_in_transaction_session_timeout=60000 -c lock_timeout=5000",
            if worker { 120000 } else { 5000 })),
        pool: Some(deadpool_postgres::PoolConfig {
            max_size: size,
            timeouts: deadpool_postgres::Timeouts {
                wait: Some(Duration::from_secs(if worker { 10 } else { 2 })),
                create: Some(Duration::from_secs(10)),
                recycle: Some(Duration::from_secs(5)),
            },
            ..Default::default()
        }),
        ..Default::default()
    };
    Ok(config.create_pool(Some(Runtime::Tokio1), tls()?)?)
}

/// This dedicated connection is never returned to a pool. If it is lost,
/// cancel workers immediately rather than continue without ownership.
pub(crate) async fn worker_lease(
    database: &str,
    tasks: fedimint_core::task::TaskGroup,
) -> anyhow::Result<Arc<tokio_postgres::Client>> {
    let mut config: tokio_postgres::Config = database.parse()?;
    config.connect_timeout(Duration::from_secs(10));
    let (client, connection) =
        tokio::time::timeout(Duration::from_secs(10), config.connect(tls()?))
            .await
            .context("Worker lease connection timed out")??;
    let connection_tasks = tasks.clone();
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            tracing::error!(%error, "Worker lease connection lost");
        }
        connection_tasks.shutdown();
    });
    let row = tokio::time::timeout(
        Duration::from_secs(10),
        client.query_one("SELECT pg_try_advisory_lock(1718447973, 1)", &[]),
    )
    .await
    .context("Worker lock acquisition timed out")??;
    anyhow::ensure!(row.get::<_, bool>(0), "Another observer already owns this database's workers; set FO_API_ONLY=true for extra API replicas");
    let client = Arc::new(client);
    let heartbeat = client.clone();
    let heartbeat_tasks = tasks.clone();
    tasks.spawn_cancellable("worker lease heartbeat", async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if !matches!(
                tokio::time::timeout(Duration::from_secs(5), heartbeat.simple_query("SELECT 1"))
                    .await,
                Ok(Ok(_))
            ) {
                tracing::error!("Worker lease heartbeat failed; stopping collectors");
                heartbeat_tasks.shutdown();
                return;
            }
        }
    });
    Ok(client)
}
