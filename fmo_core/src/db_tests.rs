//! Runs only against the disposable database created by scripts/test-db.sh.
use std::time::Duration;

#[tokio::test]
#[ignore = "requires scripts/test-db.sh"]
async fn migrations_leases_counts_and_reset_fencing() -> anyhow::Result<()> {
    let database = std::env::var("FMO_TEST_DATABASE")?;
    let (mut db, connection) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    tokio::spawn(connection);
    let (mut other, connection) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    tokio::spawn(connection);
    let migrations = [
        include_str!("../schema/v0.sql"),
        include_str!("../schema/v1.sql"),
        include_str!("../schema/v2.sql"),
        include_str!("../schema/v3.sql"),
        include_str!("../schema/v4.sql"),
        include_str!("../schema/v5.sql"),
        include_str!("../schema/v6.sql"),
        include_str!("../schema/v7.sql"),
        include_str!("../schema/v8.sql"),
        include_str!("../schema/v9.sql"),
        include_str!("../schema/v10.sql"),
        include_str!("../schema/v11.sql"),
        include_str!("../schema/v12.sql"),
    ];
    for (version, sql) in migrations.into_iter().enumerate() {
        let tx = db.transaction().await?;
        tx.batch_execute(sql)
            .await
            .map_err(|e| anyhow::anyhow!("migration {version}: {e:?}"))?;
        tx.commit().await?;
    }
    db.batch_execute(
        "INSERT INTO federations VALUES ('\\x01', '\\x');
        INSERT INTO sessions VALUES ('\\x01', 0, '\\x'), ('\\x01', 1, '\\x')",
    )
    .await?;
    let tx = db.transaction().await?;
    tx.batch_execute(include_str!("../schema/v13.sql")).await?;
    tx.batch_execute(include_str!(
        "../../fmo_modules/fmo_module_wallet/src/enrichment.sql"
    ))
    .await?;
    tx.commit().await?;
    db.batch_execute("INSERT INTO sessions VALUES ('\\x01', 1, '\\x'), ('\\x01', 2, '\\x') ON CONFLICT DO NOTHING").await?;
    let count: i64 = db
        .query_one("SELECT session_count FROM federation_session_counts", &[])
        .await?
        .get(0);
    assert_eq!(
        count, 3,
        "upgrade seeds counts and replay cannot increment them twice"
    );
    let tx = db.transaction().await?;
    tx.batch_execute("INSERT INTO sessions VALUES ('\\x01', 3, '\\x')")
        .await?;
    tx.rollback().await?;
    assert_eq!(
        db.query_one("SELECT session_count FROM federation_session_counts", &[])
            .await?
            .get::<_, i64>(0),
        3
    );

    // Actual worker ownership rejects another collector and API pools do not
    // consume this lock. No federation RPCs are needed for these checks.
    let tasks = fedimint_core::task::TaskGroup::new();
    let lease = crate::runtime::worker_lease(&database, tasks.clone()).await?;
    assert!(
        crate::runtime::worker_lease(&database, fedimint_core::task::TaskGroup::new())
            .await
            .is_err()
    );
    let pool = crate::runtime::pool(&database, 1, false)?;
    assert_eq!(
        pool.get()
            .await?
            .query_one("SHOW statement_timeout", &[])
            .await?
            .get::<_, String>(0),
        "5s"
    );
    assert_eq!(
        pool.get()
            .await?
            .query_one("SELECT 1", &[])
            .await?
            .get::<_, i32>(0),
        1
    );
    db.query_one("SELECT pg_terminate_backend(pid) FROM pg_locks WHERE locktype = 'advisory' AND classid = 1718447973 AND objid = 1", &[]).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        while !tasks.is_shutting_down() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    assert!(
        lease.is_closed(),
        "losing the ownership connection shuts down workers"
    );
    let _lease =
        crate::runtime::worker_lease(&database, fedimint_core::task::TaskGroup::new()).await?;

    // A reset to the SAME position must wait for the projector's transaction.
    let mut pooled = pool.get().await?;
    let tx = pooled.transaction().await?;
    assert_eq!(
        crate::federation::projector::module_cursor(&tx, "wallet", &[1])
            .await?
            .0,
        0
    );
    let reset = other.transaction().await?;
    reset
        .batch_execute("SET LOCAL lock_timeout = '50ms'")
        .await?;
    let blocked = reset
        .execute(
            "INSERT INTO module_cursors(module_kind, federation_id, next_session)
        VALUES ('wallet', '\\x01', 0) ON CONFLICT (module_kind, federation_id)
        DO UPDATE SET next_session = EXCLUDED.next_session",
            &[],
        )
        .await
        .unwrap_err();
    assert_eq!(
        blocked.code(),
        Some(&tokio_postgres::error::SqlState::LOCK_NOT_AVAILABLE)
    );
    reset.rollback().await?;
    tx.commit().await?;

    db.batch_execute(
        "INSERT INTO fmo_wallet.withdrawal_transactions VALUES ('\\x02', '\\x01');
        INSERT INTO fmo_wallet.enrichment_jobs(on_chain_txid) VALUES ('\\x02')",
    )
    .await?;
    let old_id: i64 = db
        .query_one("SELECT id FROM fmo_wallet.enrichment_jobs", &[])
        .await?
        .get(0);
    db.batch_execute(
        "UPDATE fmo_wallet.enrichment_jobs SET retry_at = now() + INTERVAL '2 minutes';
        DELETE FROM fmo_wallet.withdrawal_transactions WHERE on_chain_txid = '\\x02';
        INSERT INTO fmo_wallet.withdrawal_transactions VALUES ('\\x02', '\\x01');
        INSERT INTO fmo_wallet.enrichment_jobs(on_chain_txid) VALUES ('\\x02')",
    )
    .await?;
    assert!(
        db.query_opt(
            "SELECT id FROM fmo_wallet.enrichment_jobs WHERE id = $1 FOR UPDATE",
            &[&old_id]
        )
        .await?
        .is_none(),
        "a response from before a clean reset is fenced out"
    );
    let new_id: i64 = db
        .query_one("SELECT id FROM fmo_wallet.enrichment_jobs", &[])
        .await?
        .get(0);
    assert!(new_id > old_id);
    // Unfinished leases become claimable after expiry; duplicate enqueue is safe.
    db.batch_execute("UPDATE fmo_wallet.enrichment_jobs SET retry_at = now() - INTERVAL '1 second';
        INSERT INTO fmo_wallet.enrichment_jobs(on_chain_txid) VALUES ('\\x02') ON CONFLICT DO NOTHING").await?;
    assert_eq!(
        db.query_one(
            "SELECT count(*) FROM fmo_wallet.enrichment_jobs WHERE retry_at <= now()",
            &[]
        )
        .await?
        .get::<_, i64>(0),
        1
    );
    Ok(())
}
