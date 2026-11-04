//! Durable Bitcoin transaction enrichment. No database connection is held during I/O.

use std::time::Duration;

use anyhow::Context;
use bitcoin::hashes::Hash;
use bitcoin::Txid;
use fmo_core::module::Services;
use futures::{stream, StreamExt};

pub(crate) async fn run(services: Services) -> anyhow::Result<()> {
    loop {
        // The lease recovers unfinished work after a crash. The unique job id
        // also fences a response that arrives after an administrator resets a module.
        let jobs = services
            .connection()
            .await?
            .query(
                "WITH due AS (
                 SELECT id FROM fmo_wallet.enrichment_jobs
                 WHERE retry_at <= now() ORDER BY retry_at, id LIMIT 4
                 FOR UPDATE SKIP LOCKED
             )
             UPDATE fmo_wallet.enrichment_jobs j SET retry_at = now() + INTERVAL '2 minutes',
                 attempts = LEAST(attempts + 1, 20)
             FROM due WHERE j.id = due.id RETURNING j.id, j.on_chain_txid, j.attempts",
                &[],
            )
            .await?;
        if jobs.is_empty() {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }
        stream::iter(jobs).for_each_concurrent(4, |row| {
            let services = services.clone();
            async move {
                let id: i64 = row.get(0);
                let bytes: Vec<u8> = row.get(1);
                let attempts: i32 = row.get(2);
                if let Err(error) = enrich(&services, id, &bytes).await {
                    tracing::warn!(job = id, %error, "Bitcoin enrichment will retry");
                    let delay = (5_i32.saturating_mul(1 << attempts.min(10))).min(3600);
                    match services.connection().await {
                      Ok(connection) => {
                        if let Err(error) = connection.execute(
                            "UPDATE fmo_wallet.enrichment_jobs SET retry_at = now() + make_interval(secs => $2)
                             WHERE id = $1", &[&id, &f64::from(delay)],
                        ).await {
                            tracing::warn!(%error, "Could not reschedule enrichment; lease will expire");
                        }
                      }
                      Err(error) => tracing::warn!(%error, "Could not reschedule enrichment; lease will expire"),
                    }
                }
            }
        }).await;
    }
}

async fn enrich(services: &Services, id: i64, bytes: &[u8]) -> anyhow::Result<()> {
    let txid = Txid::from_slice(bytes)?;
    let fetched = tokio::time::timeout(Duration::from_secs(30), async {
        let _permit = services.external_io_permit().await?;
        services
            .esplora()
            .get_tx_no_opt(&txid)
            .await
            .map_err(anyhow::Error::from)
    })
    .await
    .context("Bitcoin transaction fetch timed out")??;
    anyhow::ensure!(
        fetched.compute_txid() == txid,
        "Explorer returned a different transaction"
    );
    let mut connection = services.connection().await?;
    let dbtx = connection.transaction().await?;
    if dbtx
        .query_opt(
            "SELECT id FROM fmo_wallet.enrichment_jobs WHERE id = $1 FOR UPDATE",
            &[&id],
        )
        .await?
        .is_none()
    {
        return Ok(());
    }
    for input in &fetched.input {
        dbtx.execute(
            "INSERT INTO fmo_wallet.withdrawal_transaction_inputs VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            &[&input.previous_output.txid.to_byte_array().to_vec(), &(input.previous_output.vout as i32), &bytes],
        ).await?;
    }
    for (index, output) in fetched.output.iter().enumerate() {
        let address =
            bitcoin::Address::from_script(&output.script_pubkey, bitcoin::Network::Bitcoin)
                .context("Peg-out output has no address")?;
        let amount = i64::try_from(
            output
                .value
                .to_sat()
                .checked_mul(1000)
                .context("Amount overflow")?,
        )?;
        dbtx.execute(
            "INSERT INTO fmo_wallet.withdrawal_transaction_outputs VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
            &[&bytes, &(index as i32), &address.to_string(), &amount],
        ).await?;
    }
    dbtx.execute(
        "DELETE FROM fmo_wallet.enrichment_jobs WHERE id = $1",
        &[&id],
    )
    .await?;
    dbtx.commit().await?;
    Ok(())
}
