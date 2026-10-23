//! Observes the mint module: e-cash issuance and redemption

use anyhow::Context;
use async_trait::async_trait;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::CommonModuleInit;
use fedimint_mint_common::{MintCommonInit, MintConsensusItem, MintInput, MintOutput};
use fmo_core::module::{details_json, InputSummary, ObserverModule, OutputSummary};

pub struct MintObserver;

#[async_trait]
impl ObserverModule for MintObserver {
    fn kind(&self) -> ModuleKind {
        MintCommonInit::KIND
    }

    fn decoder(&self) -> Decoder {
        MintCommonInit::decoder()
    }

    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary> {
        let amount_msat = input
            .as_any()
            .downcast_ref::<MintInput>()
            .context("Not Mint input")?
            .maybe_v0_ref()
            .context("Unsupported MintInput version")?
            .amount
            .msats;

        Ok(InputSummary {
            amount_msat: Some(amount_msat),
            ln_contract_id: None,
            details: details_json::<MintInput>(input.as_any(), input, "Mint Transaction Input"),
        })
    }

    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary> {
        let amount_msat = output
            .as_any()
            .downcast_ref::<MintOutput>()
            .context("Not Mint output")?
            .maybe_v0_ref()
            .context("Unsupported MintOutput version")?
            .amount
            .msats;

        Ok(OutputSummary {
            amount_msat: Some(amount_msat),
            ln_contract: None,
            details: details_json::<MintOutput>(output.as_any(), output, "Mint Transaction Output"),
        })
    }

    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        details_json::<MintConsensusItem>(ci.as_any(), ci, "Mint CI")
    }
}

#[cfg(test)]
mod tests {
    use fedimint_core::core::{DynInput, DynOutput};
    use fedimint_mint_common::{MintInput, MintOutput};
    use fmo_core::module::ObserverModule;

    use super::MintObserver;

    // A federation upgrading its mint module must stall only its own sync, not
    // panic: a panic stops every federation's tasks
    #[test]
    fn unsupported_versions_are_errors() {
        let input = DynInput::from_typed(
            0,
            MintInput::Default {
                variant: 99,
                bytes: vec![],
            },
        );
        assert!(MintObserver.summarize_input(&input).is_err());

        let output = DynOutput::from_typed(
            0,
            MintOutput::Default {
                variant: 99,
                bytes: vec![],
            },
        );
        assert!(MintObserver.summarize_output(&output).is_err());
    }
}
