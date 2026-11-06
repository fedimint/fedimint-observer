use anyhow::Context;
use async_trait::async_trait;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::CommonModuleInit;
use fedimint_lnv2_common::{
    LightningCommonInit, LightningConsensusItem, LightningInput, LightningInputV0, LightningOutput,
    LightningOutputV0,
};
use fmo_core::module::{details_json, InputSummary, ObserverModule, OutputSummary};

mod activity;
mod bounded_iroh;
mod gateways;
mod probes;
mod public_http;

pub struct LnV2Observer;

#[async_trait]
impl ObserverModule for LnV2Observer {
    fn migrations(&self) -> &'static [&'static str] {
        &[
            include_str!("../schema/v0.sql"),
            include_str!("../schema/v1.sql"),
            include_str!("../schema/v2.sql"),
        ]
    }
    fn federation_routes(&self) -> Option<axum::Router<fmo_core::module::Services>> {
        Some(gateways::routes())
    }
    fn federation_task(
        &self,
        services: &fmo_core::module::Services,
        federation_id: fedimint_core::config::FederationId,
        config: fedimint_core::config::ClientConfig,
    ) -> Option<fmo_core::module::Task> {
        if gateways::instances(&config).is_empty() {
            return None;
        }
        Some(Box::pin(gateways::run(
            services.clone(),
            federation_id,
            config,
        )))
    }

    fn kind(&self) -> ModuleKind {
        LightningCommonInit::KIND
    }
    fn decoder(&self) -> Decoder {
        LightningCommonInit::decoder()
    }
    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary> {
        let value = input.as_any();
        let input = value
            .downcast_ref::<LightningInput>()
            .context("Not an lnv2 input")?;
        let contract = match input
            .maybe_v0_ref()
            .context("Unsupported lnv2 input version")?
        {
            LightningInputV0::Outgoing(contract, _) | LightningInputV0::Incoming(contract, _) => {
                *contract
            }
        };
        Ok(InputSummary {
            claimed_output: Some(contract),
            details: details_json::<LightningInput>(value, input, "Lnv2 input"),
            ..Default::default()
        })
    }
    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary> {
        let value = output.as_any();
        let output = value
            .downcast_ref::<LightningOutput>()
            .context("Not an lnv2 output")?;
        let amount = match output
            .maybe_v0_ref()
            .context("Unsupported lnv2 output version")?
        {
            LightningOutputV0::Outgoing(contract) => contract.amount,
            LightningOutputV0::Incoming(contract) => contract.commitment.amount,
        };
        Ok(OutputSummary {
            amount_msat: Some(amount.msats),
            details: details_json::<LightningOutput>(value, output, "Lnv2 output"),
            ..Default::default()
        })
    }
    async fn process_output(
        &self,
        ctx: &fmo_core::module::ProcessCtx<'_>,
        meta: &fmo_core::module::ItemMeta,
        output: &DynOutput,
        _summary: &OutputSummary,
    ) -> anyhow::Result<()> {
        activity::output(ctx, meta, output).await
    }
    async fn process_input(
        &self,
        ctx: &fmo_core::module::ProcessCtx<'_>,
        meta: &fmo_core::module::ItemMeta,
        input: &DynInput,
        _summary: &InputSummary,
    ) -> anyhow::Result<()> {
        activity::input(ctx, meta, input).await
    }
    async fn reset_federation(
        &self,
        ctx: &fmo_core::module::ProcessCtx<'_>,
        fed: fedimint_core::config::FederationId,
    ) -> anyhow::Result<()> {
        use fedimint_core::encoding::Encodable;
        let bytes = fed.consensus_encode_to_vec();
        ctx.dbtx
            .execute(
                "DELETE FROM fmo_lnv2.contract_activity WHERE federation_id=$1",
                &[&bytes],
            )
            .await?;
        ctx.dbtx
            .execute(
                "DELETE FROM fmo_lnv2.contract_resolutions WHERE federation_id=$1",
                &[&bytes],
            )
            .await?;
        Ok(())
    }
    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        let value = ci.as_any();
        match value.downcast_ref::<LightningConsensusItem>()? {
            LightningConsensusItem::UnixTimeVote(_) | LightningConsensusItem::BlockCountVote(_) => {
                None
            }
            ci => details_json::<LightningConsensusItem>(value, ci, "Lnv2 CI"),
        }
    }
    fn unix_time_vote(&self, ci: &DynModuleConsensusItem) -> Option<u64> {
        match ci.as_any().downcast_ref::<LightningConsensusItem>()? {
            LightningConsensusItem::UnixTimeVote(time) => Some(*time),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
