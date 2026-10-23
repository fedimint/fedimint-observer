//! Observes the wallet module: peg-ins, peg-outs and the guardians' block
//! height votes that date every session

use std::str::FromStr;

use async_trait::async_trait;
use bitcoin::hashes::Hash;
use fedimint_core::core::{Decoder, DynInput, DynModuleConsensusItem, DynOutput, ModuleKind};
use fedimint_core::encoding::Encodable;
use fedimint_core::module::CommonModuleInit;
use fedimint_core::util::backoff_util::background_backoff;
use fedimint_core::util::retry;
use fedimint_wallet_common::{
    WalletCommonInit, WalletConsensusItem, WalletInput, WalletOutput, WalletOutputV0,
};
use fmo_core::module::{
    details_json, CiMeta, InputSummary, ItemMeta, ObserverModule, OutputSummary, ProcessCtx,
};
use tracing::warn;

pub struct WalletObserver;

fn wallet_input(input: &DynInput) -> &WalletInput {
    input
        .as_any()
        .downcast_ref::<WalletInput>()
        .expect("Not Wallet input")
}

fn v0_output(output: &DynOutput) -> &WalletOutputV0 {
    output
        .as_any()
        .downcast_ref::<WalletOutput>()
        .expect("Not Wallet input")
        .maybe_v0_ref()
        .expect("Not v0")
}

#[async_trait]
impl ObserverModule for WalletObserver {
    fn kind(&self) -> ModuleKind {
        WalletCommonInit::KIND
    }

    fn decoder(&self) -> Decoder {
        WalletCommonInit::decoder()
    }

    fn summarize_input(&self, input: &DynInput) -> InputSummary {
        let amount = match wallet_input(input) {
            WalletInput::V0(wallet_input) => wallet_input.tx_output().value,
            WalletInput::V1(wallet_input) => wallet_input.tx_out.value,
            _ => {
                panic!("Unsupported WalletInput version");
            }
        };

        InputSummary {
            amount_msat: Some(amount.to_sat() * 1000),
            ln_contract_id: None,
            details: details_json::<WalletInput>(input.as_any(), input, "Wallet Transaction Input"),
        }
    }

    fn summarize_output(&self, output: &DynOutput) -> OutputSummary {
        OutputSummary {
            amount_msat: Some(v0_output(output).amount().to_sat() * 1000),
            ln_contract: None,
            details: details_json::<WalletOutput>(
                output.as_any(),
                output,
                "Wallet Transaction Output",
            ),
        }
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
        let (outpoint, script) = match wallet_input(input) {
            WalletInput::V0(peg_in_proof) => {
                let outpoint = peg_in_proof.outpoint();
                let script = peg_in_proof.tx_output().script_pubkey;
                (outpoint, script)
            }
            WalletInput::V1(v1) => (v1.outpoint, v1.tx_out.script_pubkey.clone()),
            WalletInput::Default { variant, .. } => {
                panic!("WalletInput v{variant} not implemented");
            }
        };

        let address = bitcoin::Address::from_script(&script, bitcoin::Network::Bitcoin)
            .expect("Invalid output address");

        ctx.dbtx
            .execute(
                "INSERT INTO wallet_peg_ins VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
                &[
                    &outpoint.txid[..].to_owned(),
                    &(outpoint.vout as i32),
                    &address.to_string(),
                    &summary
                        .amount_msat
                        .map(|amt| amt as i64)
                        .expect("Wallet input must have amount"),
                    &meta.federation_id.consensus_encode_to_vec(),
                    &meta.txid.consensus_encode_to_vec(),
                    &(meta.index as i32),
                ],
            )
            .await?;
        Ok(())
    }

    async fn process_output(
        &self,
        ctx: &ProcessCtx<'_>,
        meta: &ItemMeta,
        output: &DynOutput,
        _summary: &OutputSummary,
    ) -> anyhow::Result<()> {
        match v0_output(output) {
            WalletOutputV0::PegOut(_) => {}
            WalletOutputV0::Rbf(_) => {
                // panic, since the benefits may outweigh the annoyance of removing and
                // restarting
                panic!(
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
            .expect("config says this should be a wallet CI");
        match wallet_ci {
            WalletConsensusItem::BlockCount(height_vote) => {
                dbtx.execute(
                    "INSERT INTO block_height_votes VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                    &[
                        &meta.federation_id.consensus_encode_to_vec(),
                        &(meta.session_index as i32),
                        &(meta.item_index as i32),
                        &(meta.peer.to_usize() as i32),
                        &(*height_vote as i32),
                    ],
                )
                .await?;
            }
            WalletConsensusItem::PegOutSignature(peg_out_sig) => {
                let peg_out_txid = peg_out_sig.txid.to_string();
                let peg_out_txid_encoded = peg_out_sig.txid.to_byte_array().to_vec();

                dbtx.execute(
                    "INSERT INTO wallet_withdrawal_transactions (on_chain_txid, federation_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[
                        &peg_out_txid_encoded,
                        &meta.federation_id.consensus_encode_to_vec(),
                    ],
                )
                .await?;

                dbtx.execute(
                    "INSERT INTO wallet_withdrawal_signatures VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
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
                        FROM wallet_withdrawal_signatures
                        WHERE on_chain_txid = $1
                        GROUP BY on_chain_txid
                        ",
                        &[&peg_out_txid_encoded],
                    )
                    .await?
                    .get::<_, i32>("num_sigs") as usize;

                // 3n + 1 <= num_peers
                // n <= (num_peers - 1) / 3
                // threshold = num_peers - floor((num_peers - 1) / 3)
                let threshold = {
                    let num_peers = ctx.config.global.api_endpoints.len();
                    num_peers - (num_peers - 1) / 3
                };

                if num_sigs < threshold {
                    return Ok(());
                }

                // at this point, the transaction reached threshold and should broadcast

                let cached: bool = dbtx.query_one(
                    "SELECT EXISTS(SELECT 1 FROM wallet_withdrawal_transaction_outputs WHERE on_chain_txid=$1)",
                    &[&peg_out_txid_encoded],
                ).await?.get(0);
                // Every signature after the threshold lands here too; the
                // transaction only needs fetching once
                if cached {
                    return Ok(());
                }

                let esplora_txid = esplora_client::Txid::from_str(peg_out_txid.as_str())
                    .expect("Couldn't create esplora txid");

                let builder = esplora_client::Builder::new(ctx.mempool_url);
                let client = builder
                    .build_async()
                    .expect("Failed to build esplora client");

                let fetched_tx = retry(
                    "fetching tx from esplora".to_string(),
                    background_backoff(),
                    || async {
                        client.get_tx_no_opt(&esplora_txid).await.map_err(|e| {
                            warn!("failed to fetch tx: {e:?}");
                            anyhow::anyhow!("failed fetching tx from esplora")
                        })
                    },
                )
                .await
                .expect("Reached usize::MAX retries");

                for input in fetched_tx.input {
                    let prev_out_txid = input.previous_output.txid.to_byte_array().to_vec();

                    dbtx.execute(
                        "INSERT INTO wallet_withdrawal_transaction_inputs VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
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
                        bitcoin::Script::from_bytes(output.script_pubkey.as_bytes()),
                        bitcoin::Network::Bitcoin,
                    )
                    .expect("Invalid bitcoin address");

                    dbtx.execute(
                        "INSERT INTO wallet_withdrawal_transaction_outputs VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
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
                // other WalletConsesnsusItems are not needed yet
            }
        }

        Ok(())
    }
}
