use std::time::Duration;

use anyhow::Context;
use fedimint_api_client::api::FederationApiExt;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::ApiRequestErased;
use fedimint_walletv2_common::endpoint_constants::{
    FEDERATION_WALLET_ENDPOINT, OUTPUT_INFO_SLICE_ENDPOINT, TRANSACTION_CHAIN_ENDPOINT,
};
use fedimint_walletv2_common::{FederationWallet, OutputInfo, TxInfo};
use fmo_api_types::WalletV2OnChainTransaction;
use fmo_core::module::Services;
use futures::future::try_join_all;

// The total inherits the oldest component's timestamp. Updating one wallet
// must not make another wallet's stale balance appear freshly observed.
pub(crate) const PUBLISH_ASSETS_SQL: &str =
    "INSERT INTO federation_observations(federation_id,kind,data,observed_at)
     SELECT $1,'assets:walletv2',to_jsonb(sum(balance_msat)::bigint),min(balance_observed_at)
     FROM fmo_walletv2.wallets WHERE federation_id=$1 AND module_instance_id=ANY($2::integer[])
       AND balance_observed_at>=now()-interval '5 minutes' AND balance_msat IS NOT NULL
     HAVING count(*)=cardinality($2::integer[]) AND count(*)>0
     ON CONFLICT(federation_id,kind) DO UPDATE SET data=EXCLUDED.data,observed_at=EXCLUDED.observed_at";

pub(crate) const CLAIM_DEPOSITS_SQL: &str = "WITH due AS (
           SELECT federation_id,txid,in_index FROM fmo_walletv2.deposit_jobs
           WHERE federation_id=$1 AND module_instance_id=$2 AND retry_at<=now()
             AND output_index < (SELECT min(output_index)+128 FROM fmo_walletv2.deposit_jobs
                 WHERE federation_id=$1 AND module_instance_id=$2 AND retry_at<=now())
           ORDER BY output_index LIMIT 32 FOR UPDATE SKIP LOCKED
         ) UPDATE fmo_walletv2.deposit_jobs j SET attempts=LEAST(attempts+1,10),
           retry_at=now()+make_interval(secs=>LEAST(3600,60*power(2,LEAST(attempts,6)))::double precision)
         FROM due WHERE j.federation_id=due.federation_id AND j.txid=due.txid AND j.in_index=due.in_index
         RETURNING j.txid,j.in_index,j.session_index,j.output_index,j.fee_sat";

pub(crate) const STORE_CHAIN_BATCH_SQL: &str =
    "INSERT INTO fmo_walletv2.chain AS c(federation_id,module_instance_id,tx_index,data)
     SELECT $1,$2,(item->>'index')::bigint,item FROM jsonb_array_elements($3::jsonb) AS items(item)
     ON CONFLICT(federation_id,module_instance_id,tx_index)
     DO UPDATE SET data=EXCLUDED.data WHERE c.data IS DISTINCT FROM EXCLUDED.data";

pub(crate) fn validate_delta(previous: i64, end_index: i64, indices: &[u64]) -> anyhow::Result<()> {
    anyhow::ensure!(
        end_index >= previous,
        "Wallet history unexpectedly regressed"
    );
    anyhow::ensure!(
        i64::try_from(indices.len())?
            == end_index
                .checked_sub(previous)
                .context("History index overflow")?,
        "Wallet history has missing entries"
    );
    for (offset, index) in indices.iter().enumerate() {
        anyhow::ensure!(
            i64::try_from(*index)? == previous + 1 + i64::try_from(offset)?,
            "Wallet history has duplicate or missing indices"
        );
    }
    Ok(())
}

pub(crate) fn instances(config: &ClientConfig) -> Vec<u16> {
    config
        .modules
        .iter()
        .filter_map(|(&id, module)| (module.kind.as_str() == "walletv2").then_some(id))
        .collect()
}

pub(crate) fn deposit_amount(value_sat: u64, fee_sat: u64) -> anyhow::Result<i64> {
    let net = value_sat
        .checked_sub(fee_sat)
        .context("Deposit fee exceeds tracked output")?;
    Ok(i64::try_from(
        net.checked_mul(1000).context("Deposit amount overflow")?,
    )?)
}

pub(crate) fn history_interval(unchanged: u32) -> Duration {
    Duration::from_secs((60u64.saturating_mul(1u64 << unchanged.min(4))).min(900))
}

pub(crate) async fn run(
    services: Services,
    fed: FederationId,
    config: ClientConfig,
) -> anyhow::Result<()> {
    try_join_all(instances(&config).into_iter().map(|instance| {
        let services = services.clone();
        let config = config.clone();
        async move {
            tokio::try_join!(
                run_instance(services.clone(), fed, config.clone(), instance),
                enrich_loop(services, fed, config, instance),
            )?;
            anyhow::Ok(())
        }
    }))
    .await?;
    Ok(())
}

async fn fetch<T: serde::de::DeserializeOwned + Eq + std::fmt::Debug + Clone + Send>(
    services: &Services,
    config: &ClientConfig,
    instance: u16,
    endpoint: &str,
    params: ApiRequestErased,
) -> anyhow::Result<T> {
    // Bound queue time as well as remote I/O. The permit ends before any database
    // write. The guardian client is the process-wide shared one: a client per
    // call would open (and leak) a new connection to every guardian each time.
    tokio::time::timeout(Duration::from_secs(30), async {
        let _permit = services.external_io_permit().await?;
        services
            .federation_api(&config.global)?
            .with_module(instance)
            .request_current_consensus(endpoint.to_owned(), params)
            .await
            .map_err(anyhow::Error::from)
    })
    .await
    .context("Wallet v2 request timed out")?
}

async fn run_instance(
    services: Services,
    fed: FederationId,
    config: ClientConfig,
    instance: u16,
) -> anyhow::Result<()> {
    let bytes = fed.consensus_encode_to_vec();
    services.connection().await?.execute(
        "INSERT INTO fmo_walletv2.wallets(federation_id,module_instance_id) VALUES($1,$2) ON CONFLICT DO NOTHING",
        &[&bytes, &i32::from(instance)],
    ).await?;
    let mut history_due = tokio::time::Instant::now();
    let mut unchanged = 0u32;
    let mut history_failures = 0u32;
    let mut failures = 0u32;
    // Stable per-instance offset spreads restarts without a shared random
    // generator.
    let jitter = (bytes.iter().map(|b| u64::from(*b)).sum::<u64>() + u64::from(instance)) % 11;
    tokio::time::sleep(Duration::from_secs(jitter)).await;
    loop {
        let wallet = fetch::<Option<FederationWallet>>(
            &services,
            &config,
            instance,
            FEDERATION_WALLET_ENDPOINT,
            ApiRequestErased::default(),
        )
        .await;
        match wallet {
            Ok(wallet) => {
                let balance = i64::try_from(
                    wallet
                        .map_or(0, |w| w.value.to_sat())
                        .checked_mul(1000)
                        .context("Wallet amount overflow")?,
                )?;
                let mut connection = services.connection().await?;
                let tx = connection.transaction().await?;
                // Serialize balance publication across instances so a delayed
                // aggregate writer cannot overwrite a newer component total.
                tx.execute(
                    "SELECT pg_advisory_xact_lock(14,hashtext(encode($1::bytea,'hex')))",
                    &[&bytes],
                )
                .await?;
                tx.execute(
                    "UPDATE fmo_walletv2.wallets SET balance_msat=$3,balance_observed_at=now()
                     WHERE federation_id=$1 AND module_instance_id=$2",
                    &[&bytes, &i32::from(instance), &balance],
                )
                .await?;
                let ids = instances(&config)
                    .into_iter()
                    .map(i32::from)
                    .collect::<Vec<_>>();
                tx.execute(PUBLISH_ASSETS_SQL, &[&bytes, &ids]).await?;
                tx.commit().await?;
                drop(connection);
                failures = 0;
            }
            Err(error) => {
                failures = failures.saturating_add(1);
                tracing::warn!(%fed, instance, %error, "Wallet balance poll failed; retaining last observation");
            }
        }
        let revisions = services
            .connection()
            .await?
            .query_one(
                "SELECT history_revision,history_collected_revision FROM fmo_walletv2.wallets
             WHERE federation_id=$1 AND module_instance_id=$2",
                &[&bytes, &i32::from(instance)],
            )
            .await?;
        let revision: i64 = revisions.get(0);
        let collected_revision: i64 = revisions.get(1);
        if tokio::time::Instant::now() >= history_due
            || (revision > collected_revision && history_failures == 0)
        {
            match fetch::<Vec<TxInfo>>(
                &services,
                &config,
                instance,
                TRANSACTION_CHAIN_ENDPOINT,
                ApiRequestErased::default(),
            )
            .await
            {
                Ok(chain) => {
                    history_failures = 0;
                    let changed = store_chain(&services, &bytes, instance, revision, chain).await?;
                    unchanged = if changed {
                        0
                    } else {
                        unchanged.saturating_add(1)
                    };
                }
                Err(error) => {
                    history_failures = history_failures.saturating_add(1);
                    unchanged = unchanged.saturating_add(1);
                    tracing::warn!(%fed, instance, %error, "Wallet history poll failed; retaining stored history");
                }
            }
            history_due = tokio::time::Instant::now() + history_interval(unchanged);
        }
        tokio::time::sleep(
            history_interval(failures).min(Duration::from_secs(900)) + Duration::from_secs(jitter),
        )
        .await;
    }
}

async fn store_chain(
    services: &Services,
    fed: &[u8],
    instance: u16,
    revision: i64,
    chain: Vec<TxInfo>,
) -> anyhow::Result<bool> {
    // Upstream v0.12 only has a full-chain endpoint. Store all entries and poll
    // unchanged chains less often; pagination bounds reads from our own API.
    // Write bounded batches so a large import does not monopolize a connection.
    let mut end_index = -1;
    for item in &chain {
        end_index = end_index.max(i64::try_from(item.index)?);
    }
    let previous: i64 = services.connection().await?.query_one(
        "SELECT history_end_index FROM fmo_walletv2.wallets WHERE federation_id=$1 AND module_instance_id=$2",
        &[&fed,&i32::from(instance)],
    ).await?.get(0);
    // TxInfo entries are append-only in the pinned Fedimint version. Avoid
    // resending unchanged history to PostgreSQL on every full-chain response.
    let mut delta = chain
        .into_iter()
        .filter(|item| previous < 0 || item.index > previous as u64)
        .collect::<Vec<_>>();
    delta.sort_unstable_by_key(|item| item.index);
    validate_delta(
        previous,
        end_index,
        &delta.iter().map(|item| item.index).collect::<Vec<_>>(),
    )?;
    let msat = |sat: u64| sat.checked_mul(1000).context("Wallet amount overflow");
    let mut changed = false;
    for batch in delta.chunks(100) {
        let data = batch
            .iter()
            .map(|item| {
                Ok(WalletV2OnChainTransaction {
                    index: item.index,
                    txid: item.txid.to_string(),
                    input_msat: msat(item.input.to_sat())?,
                    output_msat: msat(item.output.to_sat())?,
                    fee_msat: msat(item.fee.to_sat())?,
                    vbytes: item.vbytes,
                    created: item.created,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        changed |= services
            .connection()
            .await?
            .execute(
                STORE_CHAIN_BATCH_SQL,
                &[&fed, &i32::from(instance), &serde_json::to_value(data)?],
            )
            .await?
            > 0;
    }
    services.connection().await?.execute(
        "UPDATE fmo_walletv2.wallets SET history_observed_at=now(),history_collected_revision=$3,history_end_index=$4 WHERE federation_id=$1 AND module_instance_id=$2",
        &[&fed,&i32::from(instance),&revision,&end_index],
    ).await?;
    Ok(changed)
}

pub(crate) async fn apply_deposit(
    tx: &fmo_core::module::DbTransaction<'_>,
    bytes: &[u8],
    txid: &[u8],
    in_index: i32,
    session: i32,
    amount: i64,
) -> anyhow::Result<()> {
    if tx.query_opt("SELECT 1 FROM fmo_walletv2.deposit_jobs WHERE federation_id=$1 AND txid=$2 AND in_index=$3 FOR UPDATE", &[&bytes,&txid,&in_index]).await?.is_none() {
        return Ok(());
    }
    let updated = tx
        .execute(
            "UPDATE transaction_inputs SET amount_msat=$4
         WHERE federation_id=$1 AND txid=$2 AND in_index=$3 AND amount_msat IS NULL",
            &[&bytes, &txid, &in_index, &amount],
        )
        .await?;
    if updated > 0 {
        // Match ingest's lock order: the input's totals trigger runs before
        // taking the federation read-model lock. Taking it first can deadlock
        // with a session writer that already owns the totals row.
        tx.execute("SELECT lock_federation_read_models($1)", &[&bytes])
            .await?;
        // A null input previously contributed zero; publish its delta once.
        tx.execute(
            "UPDATE federation_daily_activity a SET volume_msat=a.volume_msat+$3
             FROM session_times s WHERE s.federation_id=$1 AND s.session_index=$2
             AND a.federation_id=s.federation_id AND a.day=DATE(s.estimated_session_timestamp)",
            &[&bytes, &session, &amount],
        )
        .await?;
    }
    tx.execute(
        "DELETE FROM fmo_walletv2.deposit_jobs WHERE federation_id=$1 AND txid=$2 AND in_index=$3",
        &[&bytes, &txid, &in_index],
    )
    .await?;
    Ok(())
}

/// Seconds until the next deposit job is due, by the database clock; negative
/// when overdue, NULL when there are no jobs (wait for a notification).
pub(crate) const NEXT_DEPOSIT_RETRY_SQL: &str =
    "SELECT EXTRACT(EPOCH FROM min(retry_at)-now())::float8 FROM fmo_walletv2.deposit_jobs
     WHERE federation_id=$1 AND module_instance_id=$2";

async fn enrich_loop(
    services: Services,
    fed: FederationId,
    config: ClientConfig,
    instance: u16,
) -> anyhow::Result<()> {
    let mut wakeup = services.work_notifications(&format!("walletv2:{fed}:{instance}"));
    loop {
        wakeup.borrow_and_update();
        match enrich_deposits(&services, fed, &config, instance).await {
            Ok(true) => continue,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%fed,instance,%error,"Wallet deposit enrichment will retry");
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        }
        // Measured against the database clock that claims jobs (`retry_at<=now()`);
        // an app clock ahead of it would otherwise wake early and spin.
        let wait: Option<f64> = services
            .connection()
            .await?
            .query_one(
                NEXT_DEPOSIT_RETRY_SQL,
                &[&fed.consensus_encode_to_vec(), &i32::from(instance)],
            )
            .await?
            .get(0);
        match wait {
            Some(seconds) => tokio::select! {
                result = wakeup.changed() => { result?; }
                _ = tokio::time::sleep(Services::wait_for_deadline(seconds)) => {}
            },
            None => {
                wakeup.changed().await?;
            }
        }
    }
}

async fn enrich_deposits(
    services: &Services,
    fed: FederationId,
    config: &ClientConfig,
    instance: u16,
) -> anyhow::Result<bool> {
    let bytes = fed.consensus_encode_to_vec();
    let mut jobs = services
        .connection()
        .await?
        .query(CLAIM_DEPOSITS_SQL, &[&bytes, &i32::from(instance)])
        .await?;
    jobs.sort_by_key(|row| row.get::<_, i64>(3));
    let Some(first) = jobs.first() else {
        return Ok(false);
    };
    let start = u64::try_from(first.get::<_, i64>(3))?;
    let end = start.checked_add(128).context("Output index overflow")?;
    let outputs: Vec<OutputInfo> = fetch(
        services,
        config,
        instance,
        OUTPUT_INFO_SLICE_ENDPOINT,
        ApiRequestErased::new((start, end)),
    )
    .await?;
    for job in jobs {
        let index = u64::try_from(job.get::<_, i64>(3))?;
        if index >= end {
            break;
        }
        let txid: Vec<u8> = job.get(0);
        let in_index: i32 = job.get(1);
        let session: i32 = job.get(2);
        let fee = u64::try_from(job.get::<_, i64>(4))?;
        let Some(output) = outputs.iter().find(|out| out.index == index) else {
            continue;
        };
        let amount = deposit_amount(output.value.to_sat(), fee)?;
        let mut connection = services.connection().await?;
        let tx = connection.transaction().await?;
        apply_deposit(&tx, &bytes, &txid, in_index, session, amount).await?;
        tx.commit().await?;
    }
    Ok(true)
}
