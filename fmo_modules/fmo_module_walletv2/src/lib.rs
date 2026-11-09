use anyhow::Context;
use async_trait::async_trait;
use axum::Router;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::CommonModuleInit;
use fedimint_walletv2_common::{WalletCommonInit, WalletConsensusItem, WalletInput, WalletOutput};
use fmo_core::module::{
    details_json, module_instance_id, InputSummary, ItemMeta, ObserverModule, OutputSummary,
    ProcessCtx, Services, Task,
};

mod collector;
mod history;

pub struct WalletV2Observer;

#[async_trait]
impl ObserverModule for WalletV2Observer {
    fn migrations(&self) -> &'static [&'static str] {
        &[include_str!("../schema/v0.sql")]
    }

    fn kind(&self) -> ModuleKind {
        WalletCommonInit::KIND
    }
    fn decoder(&self) -> Decoder {
        WalletCommonInit::decoder()
    }
    fn federation_routes(&self) -> Option<Router<Services>> {
        Some(history::routes())
    }
    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary> {
        let value = input.as_any();
        let input = value
            .downcast_ref::<WalletInput>()
            .context("Not a walletv2 input")?;
        input
            .maybe_v0_ref()
            .context("Unsupported walletv2 input version")?;
        Ok(InputSummary {
            details: details_json::<WalletInput>(value, input, "Walletv2 input"),
            ..Default::default()
        })
    }
    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary> {
        let value = output.as_any();
        let output = value
            .downcast_ref::<WalletOutput>()
            .context("Not a walletv2 output")?;
        let amount = output
            .maybe_v0_ref()
            .context("Unsupported walletv2 output version")?
            .value
            .to_sat()
            .checked_mul(1000)
            .context("Wallet output amount overflow")?;
        Ok(OutputSummary {
            amount_msat: Some(amount),
            details: details_json::<WalletOutput>(value, output, "Walletv2 output"),
            ..Default::default()
        })
    }
    async fn reset_federation(
        &self,
        ctx: &ProcessCtx<'_>,
        federation_id: FederationId,
    ) -> anyhow::Result<()> {
        use fedimint_core::encoding::Encodable;
        ctx.dbtx
            .execute(
                "DELETE FROM fmo_walletv2.deposit_jobs WHERE federation_id=$1",
                &[&federation_id.consensus_encode_to_vec()],
            )
            .await?;
        Ok(())
    }

    async fn process_input(
        &self,
        ctx: &ProcessCtx<'_>,
        meta: &ItemMeta,
        input: &DynInput,
        _summary: &InputSummary,
    ) -> anyhow::Result<()> {
        use fedimint_core::encoding::Encodable;
        let input = input
            .as_any()
            .downcast_ref::<WalletInput>()
            .context("Not a walletv2 input")?
            .maybe_v0_ref()
            .context("Unsupported walletv2 input version")?;
        ctx.dbtx.execute(
            "INSERT INTO fmo_walletv2.deposit_jobs
             (federation_id, module_instance_id, txid, in_index, session_index, output_index, fee_sat)
             SELECT $1, $2, $3, $4, $5, $6, $7
             WHERE EXISTS (SELECT 1 FROM transaction_inputs
                 WHERE federation_id=$1 AND txid=$3 AND in_index=$4 AND amount_msat IS NULL)
             ON CONFLICT DO NOTHING",
            &[&meta.federation_id.consensus_encode_to_vec(), &i32::from(meta.module_instance_id),
              &meta.txid.consensus_encode_to_vec(), &i32::try_from(meta.index)?,
              &i32::try_from(meta.session_index)?, &i64::try_from(input.output_index)?,
              &i64::try_from(input.fee.to_sat())?],
        ).await?;
        ctx.dbtx
            .execute(
                "SELECT pg_notify('fmo_work',$1)",
                &[&format!(
                    "walletv2:{}:{}",
                    meta.federation_id, meta.module_instance_id
                )],
            )
            .await?;
        mark_history_dirty(ctx, meta).await?;
        Ok(())
    }

    async fn process_output(
        &self,
        ctx: &ProcessCtx<'_>,
        meta: &ItemMeta,
        _output: &DynOutput,
        _summary: &OutputSummary,
    ) -> anyhow::Result<()> {
        mark_history_dirty(ctx, meta).await
    }

    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        details_json::<WalletConsensusItem>(ci.as_any(), ci, "Walletv2 CI")
    }
    fn block_height_vote(&self, ci: &DynModuleConsensusItem) -> Option<u32> {
        match ci.as_any().downcast_ref::<WalletConsensusItem>()? {
            WalletConsensusItem::BlockCount(height) => (*height).try_into().ok(),
            _ => None,
        }
    }
    fn reports_assets(&self) -> bool {
        true
    }
    fn federation_task(
        &self,
        services: &Services,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> Option<Task> {
        module_instance_id(&config, "walletv2")
            .map(|_| Box::pin(collector::run(services.clone(), federation_id, config)) as Task)
    }
}

async fn mark_history_dirty(ctx: &ProcessCtx<'_>, meta: &ItemMeta) -> anyhow::Result<()> {
    use fedimint_core::encoding::Encodable;
    ctx.dbtx
        .execute(
            "INSERT INTO fmo_walletv2.wallets(federation_id,module_instance_id,history_revision)
         VALUES($1,$2,1) ON CONFLICT(federation_id,module_instance_id)
         DO UPDATE SET history_revision=fmo_walletv2.wallets.history_revision+1",
            &[
                &meta.federation_id.consensus_encode_to_vec(),
                &i32::from(meta.module_instance_id),
            ],
        )
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
