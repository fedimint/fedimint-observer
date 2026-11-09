use super::WorkNotifications;

#[tokio::test]
async fn notification_during_query_is_retained_and_scoped() {
    let notifications = WorkNotifications::default();
    let mut deposits = notifications.subscribe("walletv2:fed:4");
    let mut gateways = notifications.subscribe("lnv2:fed:0");
    deposits.borrow_and_update();
    gateways.borrow_and_update();
    // Commit arrives after marking the generation but before awaiting changed.
    notifications.notify("walletv2:fed:4");
    assert!(deposits.has_changed().unwrap());
    assert!(!gateways.has_changed().unwrap());
    deposits.changed().await.unwrap();
    deposits.borrow_and_update();
    assert!(!deposits.has_changed().unwrap());
    notifications.notify("walletv2:fed:4");
    deposits.changed().await.unwrap();
}

#[tokio::test]
#[ignore = "requires an empty isolated FMO_WORK_TEST_DATABASE"]
async fn postgres_commit_notifications_and_block_arrival() -> anyhow::Result<()> {
    use std::time::Duration;
    let database = std::env::var("FMO_WORK_TEST_DATABASE")?;
    let notifications = WorkNotifications::default();
    let tasks = fedimint_core::task::TaskGroup::default();
    let lease =
        crate::runtime::worker_lease(&database, tasks.clone(), notifications.clone()).await?;
    let (mut writer, driver) = tokio_postgres::connect(&database, tokio_postgres::NoTls).await?;
    let driver = tokio::spawn(driver);
    // Everything v12 creates here lands in a schema of this test's own, so the
    // cleanup below cannot drop a real database's functions or triggers.
    let schema = format!("fmo_work_test_{}", std::process::id());
    writer
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET search_path TO {schema};
        CREATE TABLE schema_version(version INTEGER);
        CREATE TABLE session_time_repairs(federation_id BYTEA);
        CREATE TABLE block_times(block_height INTEGER);"
        ))
        .await?;
    writer
        // The notification triggers of v12, which only need the tables above
        .batch_execute(
            include_str!("../../schema/v12.sql")
                .split_once("-- BEGIN WORK NOTIFICATIONS")
                .and_then(|(_, rest)| rest.split_once("-- END WORK NOTIFICATIONS"))
                .expect("v12 marks its work notifications")
                .0,
        )
        .await?;
    let mut wakeup = notifications.subscribe("session-time-repair");
    wakeup.borrow_and_update();
    let tx = writer.transaction().await?;
    tx.execute("INSERT INTO session_time_repairs VALUES('\\x01')", &[])
        .await?;
    assert!(!wakeup.has_changed()?);
    tx.commit().await?;
    tokio::time::timeout(Duration::from_secs(2), wakeup.changed()).await??;
    wakeup.borrow_and_update();
    let tx = writer.transaction().await?;
    tx.execute("INSERT INTO block_times VALUES(1)", &[]).await?;
    tx.rollback().await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(50), wakeup.changed())
            .await
            .is_err()
    );
    writer
        .execute("INSERT INTO block_times VALUES(1)", &[])
        .await?;
    tokio::time::timeout(Duration::from_secs(2), wakeup.changed()).await??;
    lease.simple_query("SELECT 1").await?;
    writer
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await?;
    tasks.shutdown();
    driver.abort();
    Ok(())
}
