use anyhow::Context;
use async_trait::async_trait;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::CommonModuleInit;
use fedimint_lnv2_common::{
    LightningCommonInit, LightningConsensusItem, LightningInput, LightningInputV0, LightningOutput,
    LightningOutputV0,
};
use fmo_core::module::{details_json, InputSummary, ObserverModule, OutputSummary};

pub struct LnV2Observer;

#[async_trait]
impl ObserverModule for LnV2Observer {
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
mod tests {
    use super::*;
    use fedimint_core::core::IntoDynInstance;

    #[test]
    fn lightning_time_vote_keeps_its_session_timestamp_behavior() {
        let module = LnV2Observer;
        let time = LightningConsensusItem::UnixTimeVote(1_700_000_000).into_dyn(0);
        let height = LightningConsensusItem::BlockCountVote(850_000).into_dyn(0);
        assert_eq!(module.unix_time_vote(&time), Some(1_700_000_000));
        assert_eq!(module.unix_time_vote(&height), None);
        assert!(module.summarize_ci(&time).is_none());
        assert!(module.summarize_ci(&height).is_none());
    }
}
