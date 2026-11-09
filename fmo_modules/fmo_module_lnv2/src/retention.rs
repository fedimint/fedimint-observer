//! Gateway health retention for every federation, run once per process as the
//! module's global task. Per-federation probe workers only wait for their own
//! gateways, so expired rows of one federation never wake or busy another.
//! It keeps the same number of days as guardian health,
//! `FO_HEALTH_RETENTION_DAYS`, and is off when that is zero.
use std::time::Duration;

use fmo_core::module::Services;

/// Rows deleted per statement, so a large backlog never holds one long
/// transaction against the probe writers.
pub(crate) const RETENTION_BATCH: i64 = 1000;

pub(crate) const DELETE_EXPIRED_HEALTH_SQL: &str =
    "DELETE FROM fmo_lnv2.gateway_health_hourly WHERE (federation_id,module_instance_id,api_endpoint,hour) IN
     (SELECT federation_id,module_instance_id,api_endpoint,hour FROM fmo_lnv2.gateway_health_hourly
      WHERE hour<now()-make_interval(days=>$2) ORDER BY hour LIMIT $1 FOR UPDATE SKIP LOCKED)";

/// Seconds until the oldest health row expires, by the database clock; NULL
/// when there are none.
pub(crate) const NEXT_EXPIRY_SQL: &str =
    "SELECT EXTRACT(EPOCH FROM min(hour)+make_interval(days=>$1)-now())::float8
     FROM fmo_lnv2.gateway_health_hourly";

/// Upper bound on one wait. New rows only ever expire later than the oldest,
/// so this only bounds how long a clock or restore surprise can go unnoticed.
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
        // An overdue row left behind was locked by a concurrent delete: retry
        // shortly instead of spinning on it.
        let wait = wait.map_or(MAX_WAIT, |seconds| {
            Services::wait_for_deadline(seconds).max(Duration::from_secs(1))
        });
        tokio::time::sleep(wait.min(MAX_WAIT)).await;
    }
}
