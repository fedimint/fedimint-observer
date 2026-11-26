use std::time::Duration;

use fmo_core::module::Services;

/// Rows deleted per statement.
pub(crate) const RETENTION_BATCH: i64 = 1000;

pub(crate) const DELETE_EXPIRED_HEALTH_SQL: &str =
    "DELETE FROM fmo_lnv2.gateway_health_hourly WHERE (federation_id,module_instance_id,api_endpoint,hour) IN
     (SELECT federation_id,module_instance_id,api_endpoint,hour FROM fmo_lnv2.gateway_health_hourly
      WHERE hour<now()-make_interval(days=>$2) ORDER BY hour LIMIT $1 FOR UPDATE SKIP LOCKED)";

pub(crate) const NEXT_EXPIRY_SQL: &str =
    "SELECT EXTRACT(EPOCH FROM min(hour)+make_interval(days=>$1)-now())::float8
     FROM fmo_lnv2.gateway_health_hourly";

const MAX_WAIT: Duration = Duration::from_secs(3600);

pub(crate) async fn run(services: Services, days: u32) -> anyhow::Result<()> {
    let days = i32::try_from(days)?;
    loop {
        let deleted = services
            .connection()
            .await?
            .execute(DELETE_EXPIRED_HEALTH_SQL, &[&RETENTION_BATCH, &days])
            .await?;
        if deleted >= RETENTION_BATCH as u64 {
            continue;
        }
        let wait: Option<f64> = services
            .connection()
            .await?
            .query_one(NEXT_EXPIRY_SQL, &[&days])
            .await?
            .get(0);
        let wait = wait.map_or(MAX_WAIT, |seconds| {
            Services::wait_for_deadline(seconds).max(Duration::from_secs(1))
        });
        tokio::time::sleep(wait.min(MAX_WAIT)).await;
    }
}
