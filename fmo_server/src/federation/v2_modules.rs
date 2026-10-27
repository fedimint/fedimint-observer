//! Fedimint's v2 modules, which federations use from now on: mintv2, walletv2
//! and lnv2. What their items add to the tables all modules share, and the
//! repair of what the observer stored of them before it knew them.
//!
//! Two kinds of item don't carry their amount:
//! - A walletv2 deposit names the bitcoin output it claims only by its index
//!   among the outputs the guardians track. It is stored without an amount.
//! - An LNv2 input claims or refunds a contract by the out point of the output
//!   that funded it, and is stored with that output's amount.

use anyhow::Context;
use fedimint_core::config::{ClientConfig, FederationId};
use fedimint_core::core::{DynInput, DynModuleConsensusItem, DynOutput};
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::epoch::ConsensusItem;
use fedimint_core::{Amount, OutPoint};
use fedimint_lnv2_common::{
    LightningConsensusItem, LightningInput, LightningInputV0, LightningOutput, LightningOutputV0,
};
use fedimint_mintv2_common::{MintConsensusItem, MintInput, MintOutput};
use fedimint_walletv2_common::{WalletConsensusItem, WalletInput, WalletOutput};
use serde::Serialize;
use tracing::{info, warn};

use crate::federation::observer::{insert_block_height_vote, FederationObserver};
use crate::federation::{db, decoders_from_config, instance_to_kind};

pub(super) const KINDS: [&str; 3] = ["mintv2", "walletv2", "lnv2"];

/// What a v2 module's input or output adds to the shared tables
pub(super) struct ItemSummary {
    pub(super) amount_msat: Option<u64>,
    /// The earlier output whose amount the input is worth
    pub(super) claimed_output: Option<OutPoint>,
    pub(super) details: Option<serde_json::Value>,
}

/// `None` for an input of a module other than a v2 one. A version of an input
/// the observer doesn't know is an error: it holds back the federation's sync
/// until the observer knows it, instead of storing the input incomplete.
pub(super) fn summarize_input(kind: &str, input: &DynInput) -> anyhow::Result<Option<ItemSummary>> {
    let input = input.as_any();
    let summary = match kind {
        "mintv2" => {
            let input = input
                .downcast_ref::<MintInput>()
                .context("Not a mintv2 input")?;
            let note = &input
                .maybe_v0_ref()
                .context("Unsupported mintv2 input version")?
                .note;
            ItemSummary {
                amount_msat: Some(note.amount().msats),
                claimed_output: None,
                details: details(input),
            }
        }
        "walletv2" => {
            let input = input
                .downcast_ref::<WalletInput>()
                .context("Not a walletv2 input")?;
            input
                .maybe_v0_ref()
                .context("Unsupported walletv2 input version")?;
            ItemSummary {
                // The deposit is worth what the bitcoin output it claims
                // holds, which only the guardians know
                amount_msat: None,
                claimed_output: None,
                details: details(input),
            }
        }
        "lnv2" => {
            let input = input
                .downcast_ref::<LightningInput>()
                .context("Not an lnv2 input")?;
            let contract = match input
                .maybe_v0_ref()
                .context("Unsupported lnv2 input version")?
            {
                LightningInputV0::Outgoing(contract, _)
                | LightningInputV0::Incoming(contract, _) => *contract,
            };
            ItemSummary {
                amount_msat: None,
                claimed_output: Some(contract),
                details: details(input),
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(summary))
}

/// `None` for an output of a module other than a v2 one; see
/// [`summarize_input`] for unknown versions
pub(super) fn summarize_output(
    kind: &str,
    output: &DynOutput,
) -> anyhow::Result<Option<ItemSummary>> {
    let output = output.as_any();
    let (amount, details) = match kind {
        "mintv2" => {
            let output = output
                .downcast_ref::<MintOutput>()
                .context("Not a mintv2 output")?;
            let amount = output
                .maybe_v0_ref()
                .context("Unsupported mintv2 output version")?
                .amount();
            (amount, details(output))
        }
        "walletv2" => {
            let output = output
                .downcast_ref::<WalletOutput>()
                .context("Not a walletv2 output")?;
            let value = output
                .maybe_v0_ref()
                .context("Unsupported walletv2 output version")?
                .value;
            (Amount::from_sats(value.to_sat()), details(output))
        }
        "lnv2" => {
            let output = output
                .downcast_ref::<LightningOutput>()
                .context("Not an lnv2 output")?;
            let amount = match output
                .maybe_v0_ref()
                .context("Unsupported lnv2 output version")?
            {
                LightningOutputV0::Outgoing(contract) => contract.amount,
                LightningOutputV0::Incoming(contract) => contract.commitment.amount,
            };
            (amount, details(output))
        }
        _ => return Ok(None),
    };
    Ok(Some(ItemSummary {
        amount_msat: Some(amount.msats),
        claimed_output: None,
        details,
    }))
}

/// The details stored of a v2 module's consensus item. LNv2's are the
/// guardians' votes on the time, every minute, and on the block count, every
/// block: nothing reads them, so they aren't stored.
pub(super) fn ci_details(kind: &str, ci: &DynModuleConsensusItem) -> Option<serde_json::Value> {
    let ci = ci.as_any();
    match kind {
        "mintv2" => details(ci.downcast_ref::<MintConsensusItem>()?),
        "walletv2" => details(ci.downcast_ref::<WalletConsensusItem>()?),
        "lnv2" => match ci.downcast_ref::<LightningConsensusItem>()? {
            LightningConsensusItem::UnixTimeVote(_) | LightningConsensusItem::BlockCountVote(_) => {
                None
            }
            ci => details(ci),
        },
        _ => None,
    }
}

/// A walletv2 guardian's block count vote, which dates the session like the
/// v1 wallet's: the guardian's block count less a finality delay, 6 blocks
/// here (10 in v1), so sessions are dated about an hour early
pub(super) fn block_height_vote(kind: &str, ci: &DynModuleConsensusItem) -> Option<u32> {
    if kind != "walletv2" {
        return None;
    }
    match ci.as_any().downcast_ref::<WalletConsensusItem>()? {
        WalletConsensusItem::BlockCount(count) => u32::try_from(*count).ok(),
        _ => None,
    }
}

fn details(item: &impl Serialize) -> Option<serde_json::Value> {
    serde_json::to_value(item)
        .inspect_err(|error| warn!(?error, "Could not serialize a v2 module item"))
        .ok()
}

impl FederationObserver {
    /// Completes what the observer stored of the federation's v2 module items
    /// before it knew these modules: just their kind, without amounts and
    /// details, and none of walletv2's block count votes, so a walletv2
    /// federation's sessions had no time. Runs before the federation's new
    /// sessions are processed, so that an LNv2 claim among them finds the
    /// amount of the contract it claims. Once done, it finds nothing to do.
    pub(super) async fn complete_v2_items(
        &self,
        federation_id: FederationId,
        config: &ClientConfig,
    ) -> anyhow::Result<()> {
        let kinds = config
            .modules
            .values()
            .map(|module| module.kind.as_str().to_owned())
            .filter(|kind| KINDS.contains(&kind.as_str()))
            .collect::<Vec<_>>();
        if kinds.is_empty() {
            return Ok(());
        }
        let decoders = decoders_from_config(config);
        let federation_id_db = federation_id.consensus_encode_to_vec();

        let mut connection = self.connection().await?;
        let dbtx = connection.transaction().await?;

        // Everything stored since has details. In the order the federation
        // processed them, so a contract's funding output has its amount
        // before the input claiming it is completed.
        let transactions = dbtx
            .query(
                "SELECT txid, data FROM transactions
                 WHERE federation_id = $1 AND txid IN (
                     SELECT i.txid FROM transaction_inputs i
                     WHERE i.federation_id = $1 AND i.kind = ANY($2) AND NOT EXISTS (
                         SELECT 1 FROM transaction_input_details d
                         WHERE d.federation_id = $1 AND d.txid = i.txid AND d.in_index = i.in_index
                     )
                     UNION
                     SELECT o.txid FROM transaction_outputs o
                     WHERE o.federation_id = $1 AND o.kind = ANY($2) AND NOT EXISTS (
                         SELECT 1 FROM transaction_output_details d
                         WHERE d.federation_id = $1 AND d.txid = o.txid AND d.out_index = o.out_index
                     )
                 )
                 ORDER BY session_index, item_index",
                &[&federation_id_db, &kinds],
            )
            .await?;

        let mut added_volume_msat = 0;
        for row in &transactions {
            let txid = row.get::<_, Vec<u8>>("txid");
            let transaction = fedimint_core::transaction::Transaction::consensus_decode_whole(
                &row.get::<_, Vec<u8>>("data"),
                &decoders,
            )?;

            for (out_idx, output) in transaction.outputs.iter().enumerate() {
                let kind = instance_to_kind(config, output.module_instance_id());
                let Some(summary) = summarize_output(&kind, output)? else {
                    continue;
                };
                dbtx.execute(
                    "UPDATE transaction_outputs SET amount_msat = $4
                     WHERE federation_id = $1 AND txid = $2 AND out_index = $3 AND amount_msat IS NULL",
                    &[
                        &federation_id_db,
                        &txid,
                        &(out_idx as i32),
                        &summary.amount_msat.map(|amount| amount as i64),
                    ],
                )
                .await?;
                if let Some(details) = summary.details {
                    dbtx.execute(
                        "INSERT INTO transaction_output_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                        &[&federation_id_db, &txid, &(out_idx as i32), &kind, &details],
                    )
                    .await?;
                }
            }

            for (in_idx, input) in transaction.inputs.iter().enumerate() {
                let kind = instance_to_kind(config, input.module_instance_id());
                let Some(summary) = summarize_input(&kind, input)? else {
                    continue;
                };
                let claimed_output = summary.claimed_output;
                let amount = dbtx
                    .query_opt(
                        "UPDATE transaction_inputs SET amount_msat = COALESCE($4, (
                             SELECT amount_msat FROM transaction_outputs
                             WHERE federation_id = $1 AND txid = $5 AND out_index = $6
                         ))
                         WHERE federation_id = $1 AND txid = $2 AND in_index = $3 AND amount_msat IS NULL
                         RETURNING amount_msat",
                        &[
                            &federation_id_db,
                            &txid,
                            &(in_idx as i32),
                            &summary.amount_msat.map(|amount| amount as i64),
                            &claimed_output.map(|output| output.txid.consensus_encode_to_vec()),
                            &claimed_output.map(|output| output.out_idx as i32),
                        ],
                    )
                    .await?
                    .and_then(|row| row.get::<_, Option<i64>>("amount_msat"));
                added_volume_msat += amount.unwrap_or_default();
                if let Some(details) = summary.details {
                    dbtx.execute(
                        "INSERT INTO transaction_input_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
                        &[&federation_id_db, &txid, &(in_idx as i32), &kind, &details],
                    )
                    .await?;
                }
            }
        }
        // The trigger adding inputs to the volume only fires on inserts
        if added_volume_msat != 0 {
            dbtx.execute(
                "UPDATE federation_totals SET tx_volume_msat = tx_volume_msat + $2 WHERE federation_id = $1",
                &[&federation_id_db, &added_volume_msat],
            )
            .await?;
        }

        // A walletv2 federation without any votes has only sessions stored
        // before; walletv2's guardians vote with every new block
        let mut votes = 0;
        let has_walletv2 = kinds.iter().any(|kind| kind == "walletv2");
        if has_walletv2
            && !dbtx
                .query_one(
                    "SELECT EXISTS (SELECT 1 FROM block_height_votes WHERE federation_id = $1)",
                    &[&federation_id_db],
                )
                .await?
                .get::<_, bool>(0)
        {
            let sessions = dbtx
                .query(
                    "SELECT session_index, session FROM sessions WHERE federation_id = $1 ORDER BY session_index",
                    &[&federation_id_db],
                )
                .await?;
            for row in &sessions {
                let outcome = db::SessionOutcome::try_from_row_with_decoders(row, &decoders)?;
                let session_index = outcome.session_index as u64;
                for (item_idx, item) in outcome.data.items.into_iter().enumerate() {
                    let ConsensusItem::Module(ci) = item.item else {
                        continue;
                    };
                    let kind = instance_to_kind(config, ci.module_instance_id());
                    if let Some(details) = ci_details(&kind, &ci) {
                        dbtx.execute(
                            "INSERT INTO consensus_items VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
                            &[
                                &federation_id_db,
                                &outcome.session_index,
                                &(item_idx as i32),
                                &(item.peer.to_usize() as i32),
                                &kind,
                                &details,
                            ],
                        )
                        .await?;
                    }
                    if let Some(height_vote) = block_height_vote(&kind, &ci) {
                        insert_block_height_vote(
                            &dbtx,
                            federation_id,
                            session_index,
                            item_idx as u64,
                            item.peer,
                            height_vote,
                        )
                        .await?;
                        votes += 1;
                    }
                }
            }
        }

        dbtx.commit().await?;
        if !transactions.is_empty() || votes > 0 {
            info!(
                "Completed {} transactions and {votes} block height votes stored before v2 module support",
                transactions.len()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use fedimint_core::bitcoin::hashes::{hash160, sha256, Hash};
    use fedimint_core::bitcoin::{self, Txid};
    use fedimint_core::core::{DynInput, DynModuleConsensusItem, DynOutput};
    use fedimint_core::secp256k1::ecdsa::Signature;
    use fedimint_core::secp256k1::PublicKey;
    use fedimint_core::{Amount, OutPoint, TransactionId};
    use fedimint_lnv2_common::contracts::{OutgoingContract, PaymentImage};
    use fedimint_lnv2_common::{
        LightningConsensusItem, LightningInput, LightningInputV0, LightningOutput,
        LightningOutputV0, OutgoingWitness,
    };
    use fedimint_mintv2_common::{Denomination, MintInput, MintOutput, Note};
    use fedimint_walletv2_common::{
        StandardScript, WalletConsensusItem, WalletInput, WalletInputV0, WalletOutput,
        WalletOutputV0,
    };
    use tbs::{BlindedMessage, Message, Signature as NoteSignature};

    use super::{block_height_vote, ci_details, summarize_input, summarize_output};

    // The secp256k1 generator: any valid key will do
    fn public_key() -> PublicKey {
        PublicKey::from_str("0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .expect("valid point")
    }

    #[test]
    fn other_modules_are_left_alone() {
        let input = DynInput::from_typed(
            0,
            fedimint_mint_common::MintInput::Default {
                variant: 0,
                bytes: vec![],
            },
        );
        assert!(summarize_input("mint", &input).unwrap().is_none());
    }

    #[test]
    fn notes_are_worth_their_denomination() {
        // Any points will do: only the denomination is read
        let point = || Message::from_bytes(b"note").0;
        let output = DynOutput::from_typed(
            0,
            MintOutput::new_v0(Denomination(10), BlindedMessage(point()), [0; 16]),
        );
        let summary = summarize_output("mintv2", &output).unwrap().unwrap();
        assert_eq!(summary.amount_msat, Some(1024));
        assert!(summary.details.is_some());

        let input = DynInput::from_typed(
            0,
            MintInput::new_v0(Note {
                denomination: Denomination(10),
                nonce: public_key(),
                signature: NoteSignature(point()),
            }),
        );
        let summary = summarize_input("mintv2", &input).unwrap().unwrap();
        assert_eq!(summary.amount_msat, Some(1024));
        assert!(summary.details.is_some());
    }

    #[test]
    fn withdrawals_have_their_value_and_deposits_no_amount() {
        let output = DynOutput::from_typed(
            0,
            WalletOutput::V0(WalletOutputV0 {
                destination: StandardScript::P2WPKH(hash160::Hash::all_zeros()),
                value: bitcoin::Amount::from_sat(50_000),
                fee: bitcoin::Amount::from_sat(1_000),
            }),
        );
        let summary = summarize_output("walletv2", &output).unwrap().unwrap();
        assert_eq!(summary.amount_msat, Some(50_000_000));
        assert!(summary.details.is_some());

        let input = DynInput::from_typed(
            0,
            WalletInput::V0(WalletInputV0 {
                output_index: 3,
                tweak: public_key(),
                fee: bitcoin::Amount::from_sat(500),
            }),
        );
        let summary = summarize_input("walletv2", &input).unwrap().unwrap();
        assert_eq!(summary.amount_msat, None);
        assert_eq!(summary.claimed_output, None);
        assert!(summary.details.is_some());
    }

    #[test]
    fn walletv2_items_have_details_and_block_counts_are_votes() {
        let signatures = WalletConsensusItem::Signatures(
            Txid::all_zeros(),
            vec![Signature::from_compact(&[1; 64]).expect("valid signature")],
        );
        for ci in [
            signatures,
            WalletConsensusItem::BlockCount(921_000),
            WalletConsensusItem::Feerate(None),
        ] {
            let ci = DynModuleConsensusItem::from_typed(0, ci);
            assert!(ci_details("walletv2", &ci).is_some());
        }

        let vote = |ci| block_height_vote("walletv2", &DynModuleConsensusItem::from_typed(0, ci));
        assert_eq!(
            vote(WalletConsensusItem::BlockCount(921_000)),
            Some(921_000)
        );
        assert_eq!(vote(WalletConsensusItem::Feerate(Some(1_000))), None);
    }

    #[test]
    fn contracts_have_their_amount_and_claims_name_them() {
        let pk = public_key();
        let output = DynOutput::from_typed(
            0,
            LightningOutput::V0(LightningOutputV0::Outgoing(OutgoingContract {
                payment_image: PaymentImage::Hash(sha256::Hash::all_zeros()),
                amount: Amount::from_sats(1_000),
                expiration: 900_000,
                claim_pk: pk,
                refund_pk: pk,
                ephemeral_pk: pk,
            })),
        );
        let summary = summarize_output("lnv2", &output).unwrap().unwrap();
        assert_eq!(summary.amount_msat, Some(1_000_000));
        assert!(summary.details.is_some());

        // Claiming it names the output, not the amount: the input is stored
        // with the amount stored for the output
        let funding = OutPoint {
            txid: TransactionId::all_zeros(),
            out_idx: 0,
        };
        let input = DynInput::from_typed(
            0,
            LightningInput::V0(LightningInputV0::Outgoing(
                funding,
                OutgoingWitness::Claim([0; 32]),
            )),
        );
        let summary = summarize_input("lnv2", &input).unwrap().unwrap();
        assert_eq!(summary.amount_msat, None);
        assert_eq!(summary.claimed_output, Some(funding));
        assert!(summary.details.is_some());
    }

    #[test]
    fn lnv2_votes_are_not_stored() {
        for ci in [
            LightningConsensusItem::UnixTimeVote(1_793_059_200),
            LightningConsensusItem::BlockCountVote(900_000),
        ] {
            let ci = DynModuleConsensusItem::from_typed(0, ci);
            assert_eq!(ci_details("lnv2", &ci), None);
            assert_eq!(block_height_vote("lnv2", &ci), None);
        }
    }

    // A federation upgrading a module must hold back only its own sync, not
    // panic: a panic stops every federation's tasks
    #[test]
    fn unsupported_versions_are_errors() {
        let inputs = [
            (
                "mintv2",
                DynInput::from_typed(
                    0,
                    MintInput::Default {
                        variant: 99,
                        bytes: vec![],
                    },
                ),
            ),
            (
                "walletv2",
                DynInput::from_typed(
                    0,
                    WalletInput::Default {
                        variant: 99,
                        bytes: vec![],
                    },
                ),
            ),
            (
                "lnv2",
                DynInput::from_typed(
                    0,
                    LightningInput::Default {
                        variant: 99,
                        bytes: vec![],
                    },
                ),
            ),
        ];
        for (kind, input) in inputs {
            assert!(summarize_input(kind, &input).is_err(), "{kind}");
        }

        let outputs = [
            (
                "mintv2",
                DynOutput::from_typed(
                    0,
                    MintOutput::Default {
                        variant: 99,
                        bytes: vec![],
                    },
                ),
            ),
            (
                "walletv2",
                DynOutput::from_typed(
                    0,
                    WalletOutput::Default {
                        variant: 99,
                        bytes: vec![],
                    },
                ),
            ),
            (
                "lnv2",
                DynOutput::from_typed(
                    0,
                    LightningOutput::Default {
                        variant: 99,
                        bytes: vec![],
                    },
                ),
            ),
        ];
        for (kind, output) in outputs {
            assert!(summarize_output(kind, &output).is_err(), "{kind}");
        }
    }
}
