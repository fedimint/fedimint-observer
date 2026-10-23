use std::future::Future;
use std::pin::Pin;

use deadpool_postgres::Transaction;

use crate::federation::observer::FederationObserver;

type BackfillFn = Box<
    dyn for<'a> Fn(
            &'a FederationObserver,
            &'a Transaction<'a>,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>
        + Send
        + Sync,
>;

pub struct DbMigration {
    pub sql: &'static str,
    pub backfill: Option<BackfillFn>,
}

impl DbMigration {
    /// Create a migration that is only usable for schema setup but not for
    /// actually migrating an existing database.
    ///
    /// This type of migration is useful for removing backfill functionality
    /// once it is sufficiently old and users can be expected to not make large
    /// version jumps. If one of these migrations is encountered while migrating
    /// an existing DB the migration will be aborted.
    pub fn schema_setup(sql: &'static str) -> Self {
        DbMigration {
            sql,
            backfill: None,
        }
    }

    /// Create a migration that is pure SQL, so can be used for either setup or
    /// migrating from an older schema version.
    pub fn migration(sql: &'static str) -> Self {
        DbMigration {
            sql,
            backfill: Some(Box::new(|_, _| Box::pin(async { Ok(()) }))),
        }
    }

    /// Create a migration that is used to backfill data after a schema change.
    /// Can be used to setup a new DB or to migrate an existing one.
    pub fn migration_backfill<F>(sql: &'static str, backfill: F) -> Self
    where
        F: for<'a> Fn(
                &'a FederationObserver,
                &'a Transaction<'a>,
            ) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send + 'a>>
            + Send
            + Sync
            + 'static,
    {
        DbMigration {
            sql,
            backfill: Some(Box::new(move |slf, dbtx| backfill(slf, dbtx))),
        }
    }
}

#[macro_export]
macro_rules! schema_setup {
    ($sql_path:expr) => {
        DbMigration::schema_setup(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/",
            $sql_path
        )))
    };
}

#[macro_export]
macro_rules! migration {
    ($sql_path:expr) => {
        DbMigration::migration(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/",
            $sql_path
        )))
    };
}

#[macro_export]
macro_rules! migration_backfill {
    ($sql_path:expr, $backfill:expr) => {
        DbMigration::migration_backfill(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/", $sql_path)),
            |slf, dbtx| Box::pin($backfill(slf, dbtx)),
        )
    };
}
