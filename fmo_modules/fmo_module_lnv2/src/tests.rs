use fedimint_core::core::IntoDynInstance;

use super::*;

#[test]
fn lightning_time_vote_keeps_its_session_timestamp_behavior() {
    let module = LnV2Observer;
    let time = LightningConsensusItem::UnixTimeVote(1_700_000_000).into_dyn(0);
    let height = LightningConsensusItem::BlockCountVote(850_000).into_dyn(0);
    assert_eq!(module.unix_time_vote(&time), Some(1_700_000_000));
    assert_eq!(module.unix_time_vote(&height), None);
    assert!(module.summarize_ci(&time).is_none());
    assert!(module.summarize_ci(&height).is_none());
}
