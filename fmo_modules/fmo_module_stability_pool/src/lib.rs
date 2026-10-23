//! Observes Fedi's stability pool v1 module. Its items have no amounts core
//! can store, only their JSON details.

use async_trait::async_trait;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::CommonModuleInit;
use fmo_core::module::{details_json, InputSummary, ObserverModule, OutputSummary};
use stability_pool_common::{
    StabilityPoolCommonGen, StabilityPoolConsensusItem, StabilityPoolInput, StabilityPoolOutput,
};

pub struct StabilityPoolObserver;

#[async_trait]
impl ObserverModule for StabilityPoolObserver {
    fn kind(&self) -> ModuleKind {
        ModuleKind::from_static_str("stability_pool")
    }

    fn decoder(&self) -> Decoder {
        StabilityPoolCommonGen::decoder()
    }

    fn summarize_input(&self, input: &DynInput) -> InputSummary {
        InputSummary {
            amount_msat: None,
            ln_contract_id: None,
            details: details_json::<StabilityPoolInput>(
                input.as_any(),
                input,
                "Stability Pool Input",
            ),
        }
    }

    fn summarize_output(&self, output: &DynOutput) -> OutputSummary {
        OutputSummary {
            amount_msat: None,
            ln_contract: None,
            details: details_json::<StabilityPoolOutput>(
                output.as_any(),
                output,
                "Stability Pool Output",
            ),
        }
    }

    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        details_json::<StabilityPoolConsensusItem>(ci.as_any(), ci, "Stability Pool CI")
    }
}
