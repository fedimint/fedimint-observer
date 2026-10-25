//! Observes the wallet module: peg-ins, peg-outs and the guardians' block
//! height votes that date every session

use std::sync::Arc;

use anyhow::{bail, Context};
use async_trait::async_trait;
use axum::Router;
use bitcoin::hashes::Hash;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::CommonModuleInit;
use fedimint_core::util::backoff_util::aggressive_backoff;
use fedimint_core::util::retry;
use fedimint_core::NumPeers;
use fedimint_wallet_common::{
    WalletCommonInit, WalletConsensusItem, WalletInput, WalletOutput, WalletOutputV0,
};
use fmo_core::module::{
    details_json, CiMeta, InputSummary, ItemMeta, ObserverModule, OutputSummary, ProcessCtx,
    Services,
};
use tracing::warn;

use crate::utxos::UtxoCaches;

mod utxos;

#[derive(Default)]
pub struct WalletObserver {
    utxo_caches: Arc<UtxoCaches>,
}

fn wallet_input(input: &DynInput) -> anyhow::Result<&WalletInput> {
    input
        .as_any()
        .downcast_ref::<WalletInput>()
        .context("Not Wallet input")
}

fn v0_output(output: &DynOutput) -> anyhow::Result<&WalletOutputV0> {
    output
        .as_any()
        .downcast_ref::<WalletOutput>()
        .context("Not Wallet output")?
        .maybe_v0_ref()
        .context("Unsupported WalletOutput version")
}

#[async_trait]
impl ObserverModule for WalletObserver {
    fn kind(&self) -> ModuleKind {
        WalletCommonInit::KIND
    }

    fn decoder(&self) -> Decoder {
        WalletCommonInit::decoder()
    }

    fn summarize_input(&self, input: &DynInput) -> anyhow::Result<InputSummary> {
        let amount = match wallet_input(input)? {
            WalletInput::V0(wallet_input) => wallet_input.tx_output().value,
            WalletInput::V1(wallet_input) => wallet_input.tx_out.value,
            _ => bail!("Unsupported WalletInput version"),
        };

        Ok(InputSummary {
            amount_msat: Some(amount.to_sat() * 1000),
            ln_contract_id: None,
            details: details_json::<WalletInput>(input.as_any(), input, "Wallet Transaction Input"),
        })
    }

    fn summarize_output(&self, output: &DynOutput) -> anyhow::Result<OutputSummary> {
        Ok(OutputSummary {
            amount_msat: Some(v0_output(output)?.amount().to_sat() * 1000),
            ln_contract: None,
            details: details_json::<WalletOutput>(
                output.as_any(),
                output,
                "Wallet Transaction Output",
            ),
        })
    }

    fn summarize_ci(&self, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
        details_json::<WalletConsensusItem>(ci.as_any(), ci, "Wallet CI")
    }

    async fn process_input(
        &self,
        ctx: &ProcessCtx<'_>,
        meta: &ItemMeta,
        input: &DynInput,
        summary: &InputSummary,
    ) -> anyhow::Result<()> {
        let (outpoint, script) = match wallet_input(input)? {
            WalletInput::V0(peg_in_proof) => {
                let outpoint = peg_in_proof.outpoint();
                let script = peg_in_proof.tx_output().script_pubkey;
                (outpoint, script)
            }
            WalletInput::V1(v1) => (v1.outpoint, v1.tx_out.script_pubkey.clone()),
            WalletInput::Default { variant, .. } => {
                bail!("WalletInput v{variant} not implemented");
            }
        };

        let address = bitcoin::Address::from_script(&script, bitcoin::Network::Bitcoin)
            .context("Peg-in output has no address")?;

        ctx.dbtx
            .execute(
                "INSERT INTO fmo_wallet.peg_ins VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
                &[
                    &outpoint.txid[..].to_owned(),
                    &(outpoint.vout as i32),
                    &address.to_string(),
                    &(summary
                        .amount_msat
                        .context("Wallet input must have amount")? as i64),
                    &meta.federation_id.consensus_encode_to_vec(),
                    &meta.txid.consensus_encode_to_vec(),
                    &(meta.index as i32),
                ],
            )
            .await?;
        Ok(())
    }

    /// The federation's unspent outputs, built from peg-ins and peg-outs
    fn materialized_views(&self) -> &'static [&'static str] {
        &["fmo_wallet.utxos"]
    }

    fn federation_routes(&self) -> Option<Router<Services>> {
        Some(utxos::federation_routes(self.utxo_caches.clone()))
    }

    async fn process_output(
        &self,
        ctx: &ProcessCtx<'_>,
        meta: &ItemMeta,
        output: &DynOutput,
        _summary: &OutputSummary,
    ) -> anyhow::Result<()> {
        match v0_output(output)? {
            WalletOutputV0::PegOut(_) => {}
            WalletOutputV0::Rbf(_) => {
                // Stop this federation's sync loudly: the benefits may outweigh
                // the annoyance of removing and restarting
                bail!(
                    r#"
                    You've discovered a terribly unfortunate situation: an RBF wallet output

                    Federation ID: {}
                    Name: {}

                    If you know any of the guardians of the federation, please give them a heads up
                    that they should expect failures re-syncing, or worse. They can reach out to the
                    core dev team on Discord (chat.fedimint.org).

                    For more context, see: https://github.com/fedimint/fedimint/pull/5496
                "#,
                    meta.federation_id,
                    ctx.config
                        .global
                        .federation_name()
                        .unwrap_or("no name defined"),
                );
            }
        }
        Ok(())
    }

    async fn process_ci(
        &self,
        ctx: &ProcessCtx<'_>,
        meta: &CiMeta,
        ci: &DynModuleConsensusItem,
    ) -> anyhow::Result<()> {
        let dbtx = ctx.dbtx;
        let wallet_ci = ci
            .as_any()
            .downcast_ref::<WalletConsensusItem>()
            .context("config says this should be a wallet CI")?;
        match wallet_ci {
            WalletConsensusItem::BlockCount(height_vote) => {
                ctx.record_block_height_vote(meta, *height_vote).await?;
            }
            WalletConsensusItem::PegOutSignature(peg_out_sig) => {
                let peg_out_txid = peg_out_sig.txid;
                let peg_out_txid_encoded = peg_out_sig.txid.to_byte_array().to_vec();

                dbtx.execute(
                    "INSERT INTO fmo_wallet.withdrawal_transactions (on_chain_txid, federation_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[
                        &peg_out_txid_encoded,
                        &meta.federation_id.consensus_encode_to_vec(),
                    ],
                )
                .await?;

                dbtx.execute(
                    "INSERT INTO fmo_wallet.withdrawal_signatures VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                    &[
                        &peg_out_txid_encoded,
                        &(meta.session_index as i32),
                        &(meta.item_index as i32),
                        &(meta.peer.to_usize() as i32),
                    ],
                )
                .await?;

                let num_sigs = dbtx
                    .query_one(
                        "
                        SELECT COUNT(peer_id)::INT num_sigs
                        FROM fmo_wallet.withdrawal_signatures
                        WHERE on_chain_txid = $1
                        GROUP BY on_chain_txid
                        ",
                        &[&peg_out_txid_encoded],
                    )
                    .await?
                    .get::<_, i32>("num_sigs") as usize;

                let threshold = NumPeers::from(ctx.config.global.api_endpoints.len()).threshold();

                if num_sigs < threshold {
                    return Ok(());
                }

                // at this point, the transaction reached threshold and should broadcast

                let cached: bool = dbtx.query_one(
                    "SELECT EXISTS(SELECT 1 FROM fmo_wallet.withdrawal_transaction_outputs WHERE on_chain_txid=$1)",
                    &[&peg_out_txid_encoded],
                ).await?.get(0);
                // Every signature after the threshold lands here too; the
                // transaction only needs fetching once
                if cached {
                    return Ok(());
                }

                let client = ctx.esplora;

                // Bounded: this runs inside the session's database transaction.
                // On failure the session is rolled back and retried 30 s later,
                // instead of holding a pooled connection until esplora answers.
                let fetched_tx = retry(
                    "fetching tx from esplora".to_string(),
                    aggressive_backoff(),
                    || async {
                        client.get_tx_no_opt(&peg_out_txid).await.map_err(|e| {
                            warn!("failed to fetch tx: {e:?}");
                            anyhow::anyhow!("failed fetching tx from esplora")
                        })
                    },
                )
                .await
                .with_context(|| format!("Fetching peg-out transaction {peg_out_txid}"))?;

                for input in fetched_tx.input {
                    let prev_out_txid = input.previous_output.txid.to_byte_array().to_vec();

                    dbtx.execute(
                        "INSERT INTO fmo_wallet.withdrawal_transaction_inputs VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
                        &[
                            &prev_out_txid,
                            &(input.previous_output.vout as i32),
                            &peg_out_txid_encoded,
                        ],
                    )
                    .await?;
                }

                for (out_idx, output) in fetched_tx.output.iter().enumerate() {
                    let address = bitcoin::Address::from_script(
                        &output.script_pubkey,
                        bitcoin::Network::Bitcoin,
                    )
                    .context("Peg-out output has no address")?;

                    dbtx.execute(
                        "INSERT INTO fmo_wallet.withdrawal_transaction_outputs VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                        &[
                            &peg_out_txid_encoded,
                            &(out_idx as i32),
                            &address.to_string(),
                            &((output.value.to_sat() as i64) * 1000),
                        ],
                    )
                    .await?;
                }
            }
            _ => {
                // other WalletConsensusItems are not needed yet
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use fedimint_core::core::{DynInput, DynOutput};
    use fedimint_wallet_common::{WalletInput, WalletOutput};
    use fmo_core::module::ObserverModule;

    use super::WalletObserver;

    #[test]
    fn unsupported_versions_are_errors() {
        let input = DynInput::from_typed(
            0,
            WalletInput::Default {
                variant: 99,
                bytes: vec![],
            },
        );
        assert!(WalletObserver::default().summarize_input(&input).is_err());

        let output = DynOutput::from_typed(
            0,
            WalletOutput::Default {
                variant: 99,
                bytes: vec![],
            },
        );
        assert!(WalletObserver::default().summarize_output(&output).is_err());
    }
}
