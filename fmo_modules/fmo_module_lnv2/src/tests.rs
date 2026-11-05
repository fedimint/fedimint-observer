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

#[test]
fn registries_merge_urls_without_inventing_peer_agreement() {
    let registrations = gateways::merge_registrations(vec![
        (
            2,
            vec!["https://a.example/".into(), "https://a.example/".into()],
        ),
        (
            0,
            vec!["https://a.example/".into(), "https://b.example/".into()],
        ),
    ]);
    assert_eq!(registrations["https://a.example/"], vec![0, 2]);
    assert_eq!(registrations["https://b.example/"], vec![0]);
    assert_eq!(gateways::merge_registrations(vec![]).len(), 0);
}

#[test]
fn partial_registry_polls_do_not_remove_existing_gateways() {
    assert!(!gateways::complete_registry(7, 10));
    assert!(!gateways::complete_registry(0, 0));
    assert!(gateways::complete_registry(10, 10));
}
