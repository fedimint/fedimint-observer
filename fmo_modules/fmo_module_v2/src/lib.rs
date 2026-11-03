//! Observation support for Fedimint's v2 modules.

use std::collections::HashMap;

use anyhow::Context;
use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use fedimint_api_client::api::FederationApiExt;
use fedimint_core::config::FederationId;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::{ApiRequestErased, CommonModuleInit};
use fedimint_core::{Amount, OutPoint};
use fedimint_lnv2_common::{
    LightningCommonInit, LightningConsensusItem, LightningInput, LightningInputV0, LightningOutput,
    LightningOutputV0,
};
use fedimint_mintv2_common::{MintCommonInit, MintConsensusItem, MintInput, MintOutput};
use fedimint_walletv2_common::endpoint_constants::{
    FEDERATION_WALLET_ENDPOINT, TRANSACTION_CHAIN_ENDPOINT,
};
use fedimint_walletv2_common::{
    FederationWallet, TxInfo, WalletCommonInit, WalletConsensusItem, WalletInput, WalletOutput,
};
use fmo_api_types::WalletV2OnChainTransaction;
use fmo_core::module::{
    details_json, federation_api, module_instance_id, ApiResult, InputSummary, ObserverModule,
    OutputSummary, Services,
};

fn v2_input(
    module_kind: &str,
    input: &DynInput,
) -> anyhow::Result<(Option<u64>, Option<OutPoint>, Option<serde_json::Value>)> {
    let value = input.as_any();
    match module_kind {
        "mintv2" => {
            let input = value
                .downcast_ref::<MintInput>()
                .context("Not a mintv2 input")?;
            let note = &input
                .maybe_v0_ref()
                .context("Unsupported mintv2 input version")?
                .note;
            Ok((
                Some(note.amount().msats),
                None,
                details_json::<MintInput>(value, input, "Mintv2 input"),
            ))
        }
        "walletv2" => {
            let input = value
                .downcast_ref::<WalletInput>()
                .context("Not a walletv2 input")?;
            input
                .maybe_v0_ref()
                .context("Unsupported walletv2 input version")?;
            Ok((
                None,
                None,
                details_json::<WalletInput>(value, input, "Walletv2 input"),
            ))
        }
        "lnv2" => {
            let input = value
                .downcast_ref::<LightningInput>()
                .context("Not an lnv2 input")?;
            let contract = match input
                .maybe_v0_ref()
                .context("Unsupported lnv2 input version")?
            {
                LightningInputV0::Outgoing(contract, _)
                | LightningInputV0::Incoming(contract, _) => *contract,
            };
            Ok((
                None,
                Some(contract),
                details_json::<LightningInput>(value, input, "Lnv2 input"),
            ))
        }
        _ => anyhow::bail!("Unsupported v2 module kind {module_kind}"),
    }
}

fn v2_output(
    module_kind: &str,
    output: &DynOutput,
) -> anyhow::Result<(u64, Option<serde_json::Value>)> {
    let value = output.as_any();
    match module_kind {
        "mintv2" => {
            let output = value
                .downcast_ref::<MintOutput>()
                .context("Not a mintv2 output")?;
            let amount = output
                .maybe_v0_ref()
                .context("Unsupported mintv2 output version")?
                .amount();
            Ok((
                amount.msats,
                details_json::<MintOutput>(value, output, "Mintv2 output"),
            ))
        }
        "walletv2" => {
            let output = value
                .downcast_ref::<WalletOutput>()
                .context("Not a walletv2 output")?;
            let output = output
                .maybe_v0_ref()
                .context("Unsupported walletv2 output version")?;
            Ok((
                output.value.to_sat() * 1000,
                details_json::<WalletOutput>(value, output, "Walletv2 output"),
            ))
        }
        "lnv2" => {
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
            Ok((
                amount.msats,
                details_json::<LightningOutput>(value, output, "Lnv2 output"),
            ))
        }
        _ => anyhow::bail!("Unsupported v2 module kind {module_kind}"),
    }
}

fn ci_details(module_kind: &str, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
    let value = ci.as_any();
    match module_kind {
        "mintv2" => details_json::<MintConsensusItem>(value, ci, "Mintv2 CI"),
        "walletv2" => details_json::<WalletConsensusItem>(value, ci, "Walletv2 CI"),
        "lnv2" => match value.downcast_ref::<LightningConsensusItem>()? {
            LightningConsensusItem::UnixTimeVote(_) | LightningConsensusItem::BlockCountVote(_) => {
                None
            }
            ci => details_json::<LightningConsensusItem>(value, ci, "Lnv2 CI"),
        },
        _ => None,
    }
}

pub struct V2Module {
    kind: ModuleKind,
    decoder: Decoder,
}

impl V2Module {
    fn new(kind: ModuleKind, decoder: Decoder) -> Self {
        Self { kind, decoder }
    }

    async fn wallet_v2_on_chain(
        Path(federation_id): Path<FederationId>,
        State(services): State<Services>,
    ) -> ApiResult<Json<Vec<WalletV2OnChainTransaction>>> {
        let federation = services
            .federation(federation_id)
            .await?
            .context("Federation not observed")?;
        let Some(instance_id) = module_instance_id(&federation.config, "walletv2") else {
            return Err(anyhow::anyhow!("Federation does not have a walletv2 module").into());
        };
        let api = federation_api(services.connectors(), &federation.config.global)?
            .with_module(instance_id);
        let mut transactions = api
            .request_current_consensus::<Vec<TxInfo>>(
                TRANSACTION_CHAIN_ENDPOINT.to_owned(),
                ApiRequestErased::default(),
            )
            .await?
            .into_iter()
            .map(|tx| WalletV2OnChainTransaction {
                index: tx.index,
                txid: tx.txid.to_string(),
                input_msat: tx.input.to_sat() * 1000,
                output_msat: tx.output.to_sat() * 1000,
                fee_msat: tx.fee.to_sat() * 1000,
                vbytes: tx.vbytes,
                created: tx.created,
            })
            .collect::<Vec<_>>();
        transactions.sort_by_key(|tx| std::cmp::Reverse(tx.created));
        transactions.truncate(20);
        Ok(Json(transactions))
    }
}

#[async_trait]
impl ObserverModule for V2Module {
    fn kind(&self) -> ModuleKind {
        self.kind.clone()
    }

    fn decoder(&self) -> Decoder {
        self.decoder.clone()
    }

    fn federation_routes(&self) -> Option<Router<Services>> {
        (self.kind == WalletCommonInit::KIND)
            .then(|| Router::new().route("/wallet-v2/on-chain", get(Self::wallet_v2_on_chain)))
    }

    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary> {
        let (amount_msat, claimed_output, details) = v2_input(self.kind.clone().as_str(), input)?;
        Ok(InputSummary {
            amount_msat,
            ln_contract_id: None,
            claimed_output,
            details,
        })
    }

    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary> {
        let (amount_msat, details) = v2_output(self.kind.as_str(), output)?;
        Ok(OutputSummary {
            amount_msat: Some(amount_msat),
            ln_contract: None,
            details,
        })
    }

    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        ci_details(self.kind.as_str(), ci)
    }

    fn block_height_vote(&self, ci: &DynModuleConsensusItem) -> Option<u32> {
        if self.kind != WalletCommonInit::KIND {
            return None;
        }
        match ci.as_any().downcast_ref::<WalletConsensusItem>()? {
            WalletConsensusItem::BlockCount(height) => (*height).try_into().ok(),
            _ => None,
        }
    }

    fn unix_time_vote(&self, ci: &DynModuleConsensusItem) -> Option<u64> {
        if self.kind != LightningCommonInit::KIND {
            return None;
        }
        match ci.as_any().downcast_ref::<LightningConsensusItem>()? {
            LightningConsensusItem::UnixTimeVote(time) => Some(*time),
            _ => None,
        }
    }

    async fn federation_assets(
        &self,
        services: &Services,
    ) -> anyhow::Result<HashMap<FederationId, Amount>> {
        if self.kind != WalletCommonInit::KIND {
            return Ok(HashMap::new());
        }
        let mut assets = HashMap::new();
        for federation in services.federations().await? {
            let Some(instance_id) =
                federation.config.modules.iter().find_map(|(&id, module)| {
                    (module.kind == WalletCommonInit::KIND).then_some(id)
                })
            else {
                continue;
            };
            let api =
                fmo_core::module::federation_api(services.connectors(), &federation.config.global)?
                    .with_module(instance_id);
            let wallet = api
                .request_current_consensus::<Option<FederationWallet>>(
                    FEDERATION_WALLET_ENDPOINT.to_owned(),
                    ApiRequestErased::default(),
                )
                .await?;
            assets.insert(
                federation.federation_id,
                Amount::from_sats(wallet.map_or(0, |wallet| wallet.value.to_sat())),
            );
        }
        Ok(assets)
    }
}

pub fn mint() -> V2Module {
    V2Module::new(MintCommonInit::KIND, MintCommonInit::decoder())
}

pub fn wallet() -> V2Module {
    V2Module::new(WalletCommonInit::KIND, WalletCommonInit::decoder())
}

pub fn lightning() -> V2Module {
    V2Module::new(LightningCommonInit::KIND, LightningCommonInit::decoder())
}
