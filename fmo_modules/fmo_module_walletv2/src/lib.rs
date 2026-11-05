use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use fedimint_api_client::api::FederationApiExt;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::module::{ApiRequestErased, CommonModuleInit};
use fedimint_walletv2_common::endpoint_constants::{
    FEDERATION_WALLET_ENDPOINT, TRANSACTION_CHAIN_ENDPOINT,
};
use fedimint_walletv2_common::{
    FederationWallet, TxInfo, WalletCommonInit, WalletConsensusItem, WalletInput, WalletOutput,
};
use fmo_api_types::WalletV2OnChainTransaction;
use fmo_core::module::{
    details_json, federation_api, module_instance_id, ApiResult, InputSummary, ObserverModule,
    OutputSummary, Services, Task,
};

pub struct WalletV2Observer;

impl WalletV2Observer {
    async fn wallet_v2_on_chain(
        Path(federation_id): Path<FederationId>,
        State(services): State<Services>,
    ) -> ApiResult<Json<Vec<WalletV2OnChainTransaction>>> {
        let federation = services
            .federation(federation_id)
            .await?
            .context("Federation not observed")?;
        let Some(_) = module_instance_id(&federation.config, "walletv2") else {
            return Err(anyhow::anyhow!("Federation does not have a walletv2 module").into());
        };
        let (data, observed_at) = services
            .observation(federation_id, "walletv2-chain")
            .await?
            .ok_or_else(|| {
                fmo_core::module::ApiError::unavailable("Wallet history is still being collected")
            })?;
        if std::time::SystemTime::now()
            .duration_since(observed_at.into())
            .unwrap_or_default()
            > Duration::from_secs(300)
        {
            return Err(fmo_core::module::ApiError::unavailable(
                "Wallet history collection is delayed",
            ));
        }
        Ok(Json(serde_json::from_value(data)?))
    }

    async fn poll_wallet(
        services: Services,
        federation_id: FederationId,
        config: ClientConfig,
    ) -> anyhow::Result<()> {
        let instance_id =
            module_instance_id(&config, "walletv2").context("Missing walletv2 module")?;
        let api = federation_api(services.connectors(), &config.global)?.with_module(instance_id);
        loop {
            let balance = async {
                let _permit = services.external_io_permit().await?;
                let wallet = tokio::time::timeout(
                    Duration::from_secs(20),
                    api.request_current_consensus::<Option<FederationWallet>>(
                        FEDERATION_WALLET_ENDPOINT.to_owned(),
                        ApiRequestErased::default(),
                    ),
                )
                .await
                .context("Wallet balance poll timed out")??;
                let amount = wallet
                    .map_or(0, |wallet| wallet.value.to_sat())
                    .checked_mul(1000)
                    .context("Wallet amount overflow")?;
                services
                    .save_observation(federation_id, "assets:walletv2", &serde_json::json!(amount))
                    .await
            };
            let chain = async {
                let _permit = services.external_io_permit().await?;
                let transactions = tokio::time::timeout(
                    Duration::from_secs(20),
                    api.request_current_consensus::<Vec<TxInfo>>(
                        TRANSACTION_CHAIN_ENDPOINT.to_owned(),
                        ApiRequestErased::default(),
                    ),
                )
                .await
                .context("Wallet history poll timed out")??;
                let mut transactions = transactions
                    .into_iter()
                    .map(|tx| {
                        Ok(WalletV2OnChainTransaction {
                            index: tx.index,
                            txid: tx.txid.to_string(),
                            input_msat: tx
                                .input
                                .to_sat()
                                .checked_mul(1000)
                                .context("Wallet history input overflow")?,
                            output_msat: tx
                                .output
                                .to_sat()
                                .checked_mul(1000)
                                .context("Wallet history output overflow")?,
                            fee_msat: tx
                                .fee
                                .to_sat()
                                .checked_mul(1000)
                                .context("Wallet history fee overflow")?,
                            vbytes: tx.vbytes,
                            created: tx.created,
                        })
                    })
                    .collect::<anyhow::Result<Vec<_>>>()?;
                transactions.sort_by_key(|tx| std::cmp::Reverse((tx.created, tx.index)));
                transactions.truncate(20);
                services
                    .save_observation(
                        federation_id,
                        "walletv2-chain",
                        &serde_json::to_value(transactions)?,
                    )
                    .await
            };
            let (balance, chain) = tokio::join!(balance, chain);
            for result in [balance, chain] {
                if let Err(error) = result {
                    tracing::warn!(%federation_id, %error, "Wallet v2 poll failed; preserving last observation");
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

#[async_trait]
impl ObserverModule for WalletV2Observer {
    fn kind(&self) -> ModuleKind {
        WalletCommonInit::KIND
    }
    fn decoder(&self) -> Decoder {
        WalletCommonInit::decoder()
    }
    fn federation_routes(&self) -> Option<Router<Services>> {
        Some(Router::new().route("/wallet-v2/on-chain", get(Self::wallet_v2_on_chain)))
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
            .map(|_| Box::pin(Self::poll_wallet(services.clone(), federation_id, config)) as Task)
    }
}

#[cfg(test)]
mod tests {
    use fedimint_core::core::IntoDynInstance;

    use super::*;

    #[test]
    fn wallet_block_vote_and_asset_reporting_are_preserved() {
        let module = WalletV2Observer;
        let height = WalletConsensusItem::BlockCount(850_000).into_dyn(0);
        let fee = WalletConsensusItem::Feerate(Some(1)).into_dyn(0);
        assert_eq!(module.block_height_vote(&height), Some(850_000));
        assert_eq!(module.block_height_vote(&fee), None);
        assert!(module.reports_assets());
        assert!(module.summarize_ci(&height).is_some());
    }
}
