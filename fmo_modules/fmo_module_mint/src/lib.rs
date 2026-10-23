//! Observes the mint module: e-cash issuance and redemption

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

    fn summarize_input(&self, input: &DynInput) -> InputSummary {
        let amount_msat = input
            .as_any()
            .downcast_ref::<MintInput>()
            .expect("Not Mint input")
            .maybe_v0_ref()
            .expect("Not v0")
            .amount
            .msats;

        InputSummary {
            amount_msat: Some(amount_msat),
            ln_contract_id: None,
            details: details_json::<MintInput>(input.as_any(), input, "Mint Transaction Input"),
        }
    }

    fn summarize_output(&self, output: &DynOutput) -> OutputSummary {
        let amount_msat = output
            .as_any()
            .downcast_ref::<MintOutput>()
            .expect("Not Mint input")
            .maybe_v0_ref()
            .expect("Not v0")
            .amount
            .msats;

        OutputSummary {
            amount_msat: Some(amount_msat),
            ln_contract: None,
            details: details_json::<MintOutput>(output.as_any(), output, "Mint Transaction Output"),
        }
    }

    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        details_json::<MintConsensusItem>(ci.as_any(), ci, "Mint CI")
    }
}
