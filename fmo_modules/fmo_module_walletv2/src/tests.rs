use fedimint_core::core::IntoDynInstance;

use super::*;

#[test]
fn wallet_block_vote_and_asset_reporting_are_preserved() {
    let module = WalletV2Observer;
    let height = WalletConsensusItem::BlockCount(850_000).into_dyn(0);
    let fee = WalletConsensusItem::Feerate(Some(1)).into_dyn(0);
    assert_eq!(module.block_height_vote(&height), Some(850_000));
    assert_eq!(module.block_height_vote(&fee), None);
    assert!(module.reports_assets());
    assert!(module.summarize_ci(&height).is_some());
}

#[test]
fn deposits_use_the_tracked_output_less_the_receive_fee() {
    assert_eq!(collector::deposit_amount(100_000, 250).unwrap(), 99_750_000);
    assert_eq!(collector::deposit_amount(250, 250).unwrap(), 0);
    assert!(collector::deposit_amount(249, 250).is_err());
    assert!(collector::deposit_amount(u64::MAX, 0).is_err());
    assert!(collector::deposit_amount(i64::MAX as u64, 0).is_err());
}

#[test]
fn history_polls_slow_down_without_becoming_unbounded() {
    assert_eq!(collector::history_interval(0).as_secs(), 60);
    assert_eq!(collector::history_interval(2).as_secs(), 240);
    assert_eq!(collector::history_interval(1000).as_secs(), 900);
}

#[test]
fn history_rejects_invalid_and_unbounded_pages() {
    use history::{page_limit, Params};
    assert_eq!(page_limit(&Params::default()).unwrap(), 20);
    assert!(page_limit(&Params {
        limit: Some(0),
        ..Default::default()
    })
    .is_err());
    assert!(page_limit(&Params {
        limit: Some(101),
        ..Default::default()
    })
    .is_err());
    assert!(page_limit(&Params {
        before: Some(-1),
        ..Default::default()
    })
    .is_err());
    assert_eq!(
        page_limit(&Params {
            before: Some(0),
            limit: Some(100),
            ..Default::default()
        })
        .unwrap(),
        100
    );
}

/// Run against an empty, isolated local database. Every fixture and mutation is
/// rolled back; this never connects to the observer's application database.
#[tokio::test]
#[ignore = "requires an empty isolated FMO_WALLETV2_TEST_DATABASE"]
async fn postgres_deposit_replay_and_instance_pagination() -> anyhow::Result<()> {
    let database = std::env::var("FMO_WALLETV2_TEST_DATABASE")
        .expect("Set FMO_WALLETV2_TEST_DATABASE to an empty isolated database");
    let pool = deadpool_postgres::Config {
        url: Some(database),
        ..Default::default()
    }
    .create_pool(
        Some(deadpool_postgres::Runtime::Tokio1),
        tokio_postgres::NoTls,
    )?;
    let mut connection = pool.get().await?;
    let tx = connection.transaction().await?;
    tx.batch_execute(
        "CREATE SCHEMA fmo_walletv2; CREATE SCHEMA fmo_lnv2;
         CREATE TABLE federations(federation_id BYTEA PRIMARY KEY);
         CREATE TABLE federation_observations(federation_id BYTEA,kind TEXT,data JSONB,observed_at TIMESTAMPTZ,PRIMARY KEY(federation_id,kind));
         CREATE TABLE module_cursors(module_kind TEXT PRIMARY KEY, next_session BIGINT);
         INSERT INTO module_cursors VALUES('walletv2',100),('lnv2',200);
         CREATE TABLE transaction_inputs(federation_id BYTEA,txid BYTEA,in_index INTEGER,amount_msat BIGINT,
             PRIMARY KEY(federation_id,txid,in_index));
         CREATE TABLE session_times(federation_id BYTEA,session_index INTEGER,estimated_session_timestamp TIMESTAMP);
         CREATE TABLE federation_daily_activity(federation_id BYTEA,day DATE,volume_msat BIGINT);
         CREATE FUNCTION lock_federation_read_models(fed BYTEA) RETURNS VOID AS $$
             SELECT pg_advisory_xact_lock(12, hashtext(encode(fed,'hex')));
         $$ LANGUAGE sql;",
    ).await?;
    tx.batch_execute(include_str!("../schema/v0.sql")).await?;
    tx.batch_execute(include_str!("../../fmo_module_lnv2/schema/v0.sql"))
        .await?;
    assert_eq!(
        tx.query_one(
            "SELECT next_session FROM module_cursors WHERE module_kind='walletv2'",
            &[]
        )
        .await?
        .get::<_, i64>(0),
        0
    );
    assert_eq!(
        tx.query_one(
            "SELECT next_session FROM module_cursors WHERE module_kind='lnv2'",
            &[]
        )
        .await?
        .get::<_, i64>(0),
        200
    );
    let fed = vec![1u8; 32];
    let txid = vec![2u8; 32];
    tx.execute("INSERT INTO federations VALUES($1)", &[&fed])
        .await?;
    tx.execute(
        "INSERT INTO transaction_inputs VALUES($1,$2,0,NULL)",
        &[&fed, &txid],
    )
    .await?;
    tx.execute(
        "INSERT INTO session_times VALUES($1,7,'2026-11-05 12:00:00')",
        &[&fed],
    )
    .await?;
    tx.execute(
        "INSERT INTO federation_daily_activity VALUES($1,'2026-11-05',1000)",
        &[&fed],
    )
    .await?;
    let enqueue = "INSERT INTO fmo_walletv2.deposit_jobs(federation_id,module_instance_id,txid,in_index,session_index,output_index,fee_sat) VALUES($1,3,$2,0,7,5,250)";
    // No jobs: the wait stays NULL, so the worker awaits a notification
    // instead of polling (SQL GREATEST(0, NULL) would have returned 0).
    async fn wait(
        tx: &deadpool_postgres::Transaction<'_>,
        fed: &[u8],
    ) -> Result<Option<f64>, tokio_postgres::Error> {
        tx.query_one(collector::NEXT_DEPOSIT_RETRY_SQL, &[&fed, &3i32])
            .await
            .map(|row| row.get(0))
    }
    assert_eq!(wait(&tx, &fed).await?, None);
    tx.execute(enqueue, &[&fed, &txid]).await?;
    // A due job reports a non-positive delay, clamped to an immediate wake.
    let due = wait(&tx, &fed).await?.expect("a job is queued");
    assert!(due <= 0.0);
    assert_eq!(
        fmo_core::module::Services::wait_for_deadline(due),
        std::time::Duration::ZERO
    );
    let claimed = tx
        .query(collector::CLAIM_DEPOSITS_SQL, &[&fed, &3i32])
        .await?;
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].get::<_, i64>(3), 5);
    assert!(tx
        .query(collector::CLAIM_DEPOSITS_SQL, &[&fed, &3i32])
        .await?
        .is_empty());
    let amount = collector::deposit_amount(100_000, 250)?;
    tx.batch_execute("SAVEPOINT interrupted_enrichment").await?;
    collector::apply_deposit(&tx, &fed, &txid, 0, 7, amount).await?;
    tx.batch_execute("ROLLBACK TO SAVEPOINT interrupted_enrichment")
        .await?;
    assert!(tx
        .query_one("SELECT amount_msat FROM transaction_inputs", &[])
        .await?
        .get::<_, Option<i64>>(0)
        .is_none());
    assert_eq!(
        tx.query_one("SELECT count(*) FROM fmo_walletv2.deposit_jobs", &[])
            .await?
            .get::<_, i64>(0),
        1
    );
    collector::apply_deposit(&tx, &fed, &txid, 0, 7, amount).await?;
    // A replay of a completed job must not add the same amount to daily volume
    // twice.
    tx.execute(enqueue, &[&fed, &txid]).await?;
    collector::apply_deposit(&tx, &fed, &txid, 0, 7, amount).await?;
    assert_eq!(
        tx.query_one("SELECT amount_msat FROM transaction_inputs", &[])
            .await?
            .get::<_, i64>(0),
        amount
    );
    assert_eq!(
        tx.query_one("SELECT volume_msat FROM federation_daily_activity", &[])
            .await?
            .get::<_, i64>(0),
        1000 + amount
    );
    assert_eq!(
        tx.query_one("SELECT count(*) FROM fmo_walletv2.deposit_jobs", &[])
            .await?
            .get::<_, i64>(0),
        0
    );
    for instance in [3i32, 4] {
        tx.execute("INSERT INTO fmo_walletv2.wallets(federation_id,module_instance_id,history_end_index) VALUES($1,$2,2)",&[&fed,&instance]).await?;
        let batch = (0..3i64)
            .map(|index| serde_json::json!({"instance":instance,"index":index}))
            .collect::<Vec<_>>();
        assert_eq!(
            tx.execute(
                collector::STORE_CHAIN_BATCH_SQL,
                &[&fed, &instance, &serde_json::to_value(&batch)?]
            )
            .await?,
            3
        );
        assert_eq!(
            tx.execute(
                collector::STORE_CHAIN_BATCH_SQL,
                &[&fed, &instance, &serde_json::to_value(&batch)?]
            )
            .await?,
            0
        );
    }
    tx.execute(
        "UPDATE fmo_walletv2.wallets SET history_end_index=0 WHERE module_instance_id=3",
        &[],
    )
    .await?;
    let unpublished = tx
        .query(
            history::HISTORY_PAGE_SQL,
            &[&fed, &3i32, &None::<i64>, &20i64],
        )
        .await?;
    assert_eq!(unpublished.len(), 1);
    tx.execute(
        "UPDATE fmo_walletv2.wallets SET history_end_index=2 WHERE module_instance_id=3",
        &[],
    )
    .await?;
    let ids = vec![3i32, 4];
    tx.execute(collector::PUBLISH_ASSETS_SQL, &[&fed, &ids])
        .await?;
    assert_eq!(
        tx.query_one("SELECT count(*) FROM federation_observations", &[])
            .await?
            .get::<_, i64>(0),
        0
    );
    tx.execute("UPDATE fmo_walletv2.wallets SET balance_msat=100,balance_observed_at=now()-interval '1 minute' WHERE module_instance_id=3",&[]).await?;
    tx.execute("UPDATE fmo_walletv2.wallets SET balance_msat=200,balance_observed_at=now() WHERE module_instance_id=4",&[]).await?;
    tx.execute(collector::PUBLISH_ASSETS_SQL, &[&fed, &ids])
        .await?;
    assert_eq!(
        tx.query_one("SELECT data FROM federation_observations", &[])
            .await?
            .get::<_, serde_json::Value>(0),
        serde_json::json!(300)
    );
    assert!(tx
        .query_one(
            "SELECT observed_at=now()-interval '1 minute' FROM federation_observations",
            &[]
        )
        .await?
        .get::<_, bool>(0));
    tx.execute("UPDATE fmo_walletv2.wallets SET balance_msat=900,balance_observed_at=now()-interval '6 minutes' WHERE module_instance_id=3",&[]).await?;
    tx.execute(collector::PUBLISH_ASSETS_SQL, &[&fed, &ids])
        .await?;
    assert_eq!(
        tx.query_one("SELECT data FROM federation_observations", &[])
            .await?
            .get::<_, serde_json::Value>(0),
        serde_json::json!(300)
    );
    let rows = tx
        .query(
            history::HISTORY_PAGE_SQL,
            &[&fed, &3i32, &None::<i64>, &2i64],
        )
        .await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].get::<_, serde_json::Value>(0),
        serde_json::json!({"instance":3,"index":2})
    );
    let rows = tx
        .query(
            history::HISTORY_PAGE_SQL,
            &[&fed, &4i32, &Some(1i64), &2i64],
        )
        .await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get::<_, serde_json::Value>(0),
        serde_json::json!({"instance":4,"index":0})
    );
    tx.rollback().await?;
    Ok(())
}

#[test]
fn history_publication_requires_a_contiguous_append() {
    assert!(collector::validate_delta(-1, 2, &[0, 1, 2]).is_ok());
    assert!(collector::validate_delta(2, 2, &[]).is_ok());
    assert!(collector::validate_delta(2, 4, &[3, 4]).is_ok());
    assert!(collector::validate_delta(2, 4, &[4]).is_err());
    assert!(collector::validate_delta(2, 4, &[3, 3]).is_err());
    assert!(collector::validate_delta(2, 1, &[]).is_err());
}
