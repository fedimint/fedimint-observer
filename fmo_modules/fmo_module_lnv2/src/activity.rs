use anyhow::Context;
use fedimint_core::core::{DynInput, DynOutput};
use fedimint_core::encoding::Encodable;
use fedimint_lnv2_common::{
    LightningInput, LightningInputV0, LightningOutput, LightningOutputV0, OutgoingWitness,
};
use fmo_core::module::{ItemMeta, ProcessCtx};

pub(crate) async fn output(
    ctx: &ProcessCtx<'_>,
    meta: &ItemMeta,
    output: &DynOutput,
) -> anyhow::Result<()> {
    let output = output
        .as_any()
        .downcast_ref::<LightningOutput>()
        .context("Not an lnv2 output")?
        .maybe_v0_ref()
        .context("Unsupported lnv2 output version")?;
    let (key, direction, amount) = match output {
        LightningOutputV0::Outgoing(contract) => (
            contract.claim_pk.to_string(),
            "outgoing",
            contract.amount.msats,
        ),
        LightningOutputV0::Incoming(contract) => (
            contract.commitment.refund_pk.to_string(),
            "incoming",
            contract.commitment.amount.msats,
        ),
    };
    ctx.dbtx.execute(
        "INSERT INTO fmo_lnv2.contract_activity(federation_id,module_instance_id,txid,out_index,session_index,gateway_key,direction,amount_msat)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING",
        &[&meta.federation_id.consensus_encode_to_vec(),&i32::from(meta.module_instance_id),&meta.txid.consensus_encode_to_vec(),
          &i32::try_from(meta.index)?,&i32::try_from(meta.session_index)?,&key,&direction,&i64::try_from(amount)?],
    ).await?;
    Ok(())
}

pub(crate) fn outgoing_outcome(witness: &OutgoingWitness) -> &'static str {
    match witness {
        OutgoingWitness::Claim(_) => "claimed",
        OutgoingWitness::Refund => "refunded",
        OutgoingWitness::Cancel(_) => "cancelled",
    }
}

pub(crate) async fn input(
    ctx: &ProcessCtx<'_>,
    meta: &ItemMeta,
    input: &DynInput,
) -> anyhow::Result<()> {
    let input = input
        .as_any()
        .downcast_ref::<LightningInput>()
        .context("Not an lnv2 input")?
        .maybe_v0_ref()
        .context("Unsupported lnv2 input version")?;
    let (funding, outcome) = match input {
        LightningInputV0::Outgoing(funding, witness) => (funding, outgoing_outcome(witness)),
        LightningInputV0::Incoming(funding, _) => (funding, "incoming_spent"),
    };
    ctx.dbtx.execute(
        "INSERT INTO fmo_lnv2.contract_resolutions(federation_id,module_instance_id,funding_txid,funding_out_index,session_index,outcome)
         VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
        &[&meta.federation_id.consensus_encode_to_vec(),&i32::from(meta.module_instance_id),&funding.txid.consensus_encode_to_vec(),
          &i32::try_from(funding.out_idx)?,&i32::try_from(meta.session_index)?,&outcome],
    ).await?;
    Ok(())
}
