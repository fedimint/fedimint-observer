use fedimint_core::core::{DynInput, DynOutput};
use fedimint_mint_common::{MintInput, MintOutput};
use fmo_core::module::ObserverModule;

use super::MintObserver;

// A federation upgrading its mint module must stall only its own sync, not
// panic: a panic stops every federation's tasks
#[test]
fn unsupported_versions_are_errors() {
    let input = DynInput::from_typed(
        0,
        MintInput::Default {
            variant: 99,
            bytes: vec![],
        },
    );
    assert!(MintObserver.summarize_input(&input).is_err());

    let output = DynOutput::from_typed(
        0,
        MintOutput::Default {
            variant: 99,
            bytes: vec![],
        },
    );
    assert!(MintObserver.summarize_output(&output).is_err());
}
