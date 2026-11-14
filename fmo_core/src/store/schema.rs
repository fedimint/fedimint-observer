//! Core's database schema: its migrations, the schemas and migrations of the
//! build's modules, and the materialized views they declare.

use std::time::{Duration, Instant};

use anyhow::{ensure, Context};
use tracing::{debug, info, warn};

use crate::federation::observer::FederationObserver;
use crate::module::schema_name;
use crate::util::query_value;

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
    include_str!("../../schema/v13.sql"),
];

/// Core migrations a database of this build has applied
pub(crate) fn core_schema_version() -> i32 {
    CORE_MIGRATIONS.len() as i32 - 1
}

impl FederationObserver {
    /// Whether core's schema (the first on the search path) has the table,
    /// for telling released migrations from pre-release ones of the same
    /// number
    async fn has_table(&self, table: &str) -> anyhow::Result<bool> {
        query_value(
            &self.connection().await?,
            "SELECT to_regclass($1) IS NOT NULL",
            &[&table],
        )
        .await
    }

    pub(crate) async fn setup_schema(&self) -> anyhow::Result<()> {
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
        // The released v13 creates source_observations, which the pre-release
        // v13 and v14 lack.
        let pre_release = match maybe_schema_version {
            Some(13 | 14) => !self.has_table("source_observations").await?,
            Some(12) => !self.has_table("session_time_repairs").await?,
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
        for kind in self.modules().kinds() {
            let module = self.modules().get(kind).expect("listed by the registry");
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

    pub(crate) async fn refresh_views(self) {
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
        for kind in self.modules().kinds() {
            let module = self.modules().get(kind).expect("listed by the registry");
            for view in module.materialized_views() {
                connection
                    .batch_execute(&format!("REFRESH MATERIALIZED VIEW CONCURRENTLY {view}"))
                    .await
                    .with_context(|| format!("refreshing {view} of module {kind}"))?;
            }
        }

        Ok(())
    }
}
