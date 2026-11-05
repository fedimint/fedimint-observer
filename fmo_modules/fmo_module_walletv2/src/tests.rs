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
