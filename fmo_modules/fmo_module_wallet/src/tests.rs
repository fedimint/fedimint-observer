use fedimint_core::core::{DynInput, DynModuleConsensusItem, DynOutput};
use fedimint_wallet_common::{WalletConsensusItem, WalletInput, WalletOutput};
use fmo_core::module::ObserverModule;

use super::WalletObserver;

#[test]
fn only_block_count_items_are_height_votes() {
    let vote = DynModuleConsensusItem::from_typed(0, WalletConsensusItem::BlockCount(840_000));
    assert_eq!(
        WalletObserver::default().block_height_vote(&vote),
        Some(840_000)
    );

    let other = DynModuleConsensusItem::from_typed(
        0,
        WalletConsensusItem::Default {
            variant: 99,
            bytes: vec![],
        },
    );
    assert_eq!(WalletObserver::default().block_height_vote(&other), None);
}

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
