use fedimint_core::core::IntoDynInstance;

use super::*;

#[test]
fn mint_consensus_details_remain_available() {
    let item = MintConsensusItem::Default {
        variant: 7,
        bytes: vec![1, 2],
    }
    .into_dyn(0);
    assert!(MintV2Observer.summarize_ci(&item).is_some());
}
