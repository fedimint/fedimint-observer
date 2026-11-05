use anyhow::Context;
use async_trait::async_trait;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::CommonModuleInit;
use fedimint_mintv2_common::{MintCommonInit, MintConsensusItem, MintInput, MintOutput};
use fmo_core::module::{details_json, InputSummary, ObserverModule, OutputSummary};

pub struct MintV2Observer;

#[async_trait]
impl ObserverModule for MintV2Observer {
    fn kind(&self) -> ModuleKind {
        MintCommonInit::KIND
    }
    fn decoder(&self) -> Decoder {
        MintCommonInit::decoder()
    }
    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary> {
        let value = input.as_any();
        let input = value
            .downcast_ref::<MintInput>()
            .context("Not a mintv2 input")?;
        let note = &input
            .maybe_v0_ref()
            .context("Unsupported mintv2 input version")?
            .note;
        Ok(InputSummary {
            amount_msat: Some(note.amount().msats),
            details: details_json::<MintInput>(value, input, "Mintv2 input"),
            ..Default::default()
        })
    }
    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary> {
        let value = output.as_any();
        let output = value
            .downcast_ref::<MintOutput>()
            .context("Not a mintv2 output")?;
        let amount = output
            .maybe_v0_ref()
            .context("Unsupported mintv2 output version")?
            .amount();
        Ok(OutputSummary {
            amount_msat: Some(amount.msats),
            details: details_json::<MintOutput>(value, output, "Mintv2 output"),
            ..Default::default()
        })
    }
    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        details_json::<MintConsensusItem>(ci.as_any(), ci, "Mintv2 CI")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fedimint_core::core::IntoDynInstance;

    #[test]
    fn mint_consensus_details_remain_available() {
        let item = MintConsensusItem::Default {
            variant: 7,
            bytes: vec![1, 2],
        }
        .into_dyn(0);
        assert!(MintV2Observer.summarize_ci(&item).is_some());
    }
}
