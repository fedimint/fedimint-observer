//! Writers of core's rows of a session's items. Ingest writes them as it
//! stores a session; a module's projector writes the ones of items stored
//! while the build lacked the module (`federation/projector.rs`).

use deadpool_postgres::Transaction;
use fedimint_core::config::FederationId;
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::{PeerId, TransactionId};

pub(crate) async fn insert_input_details(
    dbtx: &Transaction<'_>,
    federation_id: FederationId,
    txid: TransactionId,
    in_index: usize,
    kind: &str,
    details: &serde_json::Value,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO transaction_input_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &federation_id.consensus_encode_to_vec(),
            &txid.consensus_encode_to_vec(),
            &(in_index as i32),
            &kind,
            details,
        ],
    )
    .await?;
    Ok(())
}

pub(crate) async fn insert_output_details(
    dbtx: &Transaction<'_>,
    federation_id: FederationId,
    txid: TransactionId,
    out_index: usize,
    kind: &str,
    details: &serde_json::Value,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO transaction_output_details VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &federation_id.consensus_encode_to_vec(),
            &txid.consensus_encode_to_vec(),
            &(out_index as i32),
            &kind,
            details,
        ],
    )
    .await?;
    Ok(())
}

/// Where a module consensus item sits in a federation's history
pub(crate) struct CiPosition {
    pub(crate) federation_id: FederationId,
    pub(crate) session_index: u64,
    pub(crate) item_index: u64,
    pub(crate) peer: PeerId,
}

pub(crate) async fn insert_consensus_item(
    dbtx: &Transaction<'_>,
    at: &CiPosition,
    kind: &str,
    data: &serde_json::Value,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO consensus_items VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
        &[
            &at.federation_id.consensus_encode_to_vec(),
            &(at.session_index as i32),
            &(at.item_index as i32),
            &(at.peer.to_usize() as i32),
            &kind,
            data,
        ],
    )
    .await?;
    Ok(())
}

pub(crate) async fn insert_block_height_vote(
    dbtx: &Transaction<'_>,
    at: &CiPosition,
    height: u32,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO block_height_votes VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &at.federation_id.consensus_encode_to_vec(),
            &(at.session_index as i32),
            &(at.item_index as i32),
            &(at.peer.to_usize() as i32),
            &(height as i32),
        ],
    )
    .await?;
    Ok(())
}

pub(crate) async fn insert_unix_time_vote(
    dbtx: &Transaction<'_>,
    at: &CiPosition,
    time: u64,
) -> anyhow::Result<()> {
    dbtx.execute(
        "INSERT INTO unix_time_votes VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
        &[
            &at.federation_id.consensus_encode_to_vec(),
            &(at.session_index as i32),
            &(at.item_index as i32),
            &(at.peer.to_usize() as i32),
            &(time as i64),
        ],
    )
    .await?;
    Ok(())
}

pub(crate) fn federation_id_from_db(bytes: &[u8]) -> anyhow::Result<FederationId> {
    Ok(FederationId::consensus_decode_whole(
        bytes,
        &Default::default(),
    )?)
}
