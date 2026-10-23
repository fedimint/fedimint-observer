//! Observes the Lightning (v1) module: contracts funded, claimed and cancelled

use anyhow::Context;
use async_trait::async_trait;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::CommonModuleInit;
use fedimint_ln_common::contracts::{Contract, IdentifiableContract};
use fedimint_ln_common::{
    LightningCommonInit, LightningConsensusItem, LightningInput, LightningOutput, LightningOutputV0,
};
use fmo_core::module::{
    details_json, InputSummary, ItemMeta, ObserverModule, OutputSummary, ProcessCtx,
};

pub struct LnObserver;

fn v0_output(output: &DynOutput) -> anyhow::Result<&LightningOutputV0> {
    output
        .as_any()
        .downcast_ref::<LightningOutput>()
        .context("Not LN output")?
        .maybe_v0_ref()
        .context("Unsupported LightningOutput version")
}

#[async_trait]
impl ObserverModule for LnObserver {
    fn kind(&self) -> ModuleKind {
        LightningCommonInit::KIND
    }

    fn decoder(&self) -> Decoder {
        LightningCommonInit::decoder()
    }

    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary> {
        let v0 = input
            .as_any()
            .downcast_ref::<LightningInput>()
            .context("Not LN input")?
            .maybe_v0_ref()
            .context("Unsupported LightningInput version")?;

        Ok(InputSummary {
            amount_msat: Some(v0.amount.msats),
            ln_contract_id: Some(v0.contract_id.consensus_encode_to_vec()),
            details: details_json::<LightningInput>(
                input.as_any(),
                input,
                "Lightning Transaction Input",
            ),
        })
    }

    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary> {
        let (amount_msat, interaction, contract_id) = match v0_output(output)? {
            LightningOutputV0::Contract(contract) => (
                contract.amount.msats,
                "fund",
                contract.contract.contract_id(),
            ),
            // For incoming contracts payment hash == contract id
            LightningOutputV0::Offer(offer) => (0, "offer", offer.hash.into()),
            LightningOutputV0::CancelOutgoing { contract, .. } => (0, "cancel", *contract),
        };

        Ok(OutputSummary {
            amount_msat: Some(amount_msat),
            ln_contract: Some((interaction, contract_id.consensus_encode_to_vec())),
            details: details_json::<LightningOutput>(
                output.as_any(),
                output,
                "Lightning Transaction Output",
            ),
        })
    }

    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        details_json::<LightningConsensusItem>(ci.as_any(), ci, "Lightning CI")
    }

    async fn process_output(
        &self,
        ctx: &ProcessCtx<'_>,
        meta: &ItemMeta,
        output: &DynOutput,
        _summary: &OutputSummary,
    ) -> anyhow::Result<()> {
        let LightningOutputV0::Contract(contract) = v0_output(output)? else {
            return Ok(());
        };
        let (contract_type, payment_hash) = match &contract.contract {
            Contract::Incoming(c) => ("incoming", c.hash),
            Contract::Outgoing(c) => ("outgoing", c.hash),
        };

        ctx.dbtx
            .execute(
                "INSERT INTO ln_contracts VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                &[
                    &meta.federation_id.consensus_encode_to_vec(),
                    &contract.contract.contract_id().consensus_encode_to_vec(),
                    &contract_type,
                    &payment_hash.consensus_encode_to_vec(),
                ],
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use fedimint_core::core::{DynInput, DynOutput};
    use fedimint_ln_common::{LightningInput, LightningOutput};
    use fmo_core::module::ObserverModule;

    use super::LnObserver;

    #[test]
    fn unsupported_versions_are_errors() {
        let input = DynInput::from_typed(
            0,
            LightningInput::Default {
                variant: 99,
                bytes: vec![],
            },
        );
        assert!(LnObserver.summarize_input(&input).is_err());

        let output = DynOutput::from_typed(
            0,
            LightningOutput::Default {
                variant: 99,
                bytes: vec![],
            },
        );
        assert!(LnObserver.summarize_output(&output).is_err());
    }
}
