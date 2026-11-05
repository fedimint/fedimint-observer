use fedimint_core::core::{DynInput, DynOutput};
use fedimint_ln_common::{LightningInput, LightningOutput};
use fmo_core::module::ObserverModule;

use super::LnObserver;

#[test]
fn unsupported_versions_are_errors() {
    let input = DynInput::from_typed(
        0,
        LightningInput::Default {
            variant: 99,
            bytes: vec![],
        },
    );
    assert!(LnObserver::default().summarize_input(&input).is_err());

    let output = DynOutput::from_typed(
        0,
        LightningOutput::Default {
            variant: 99,
            bytes: vec![],
        },
    );
    assert!(LnObserver::default().summarize_output(&output).is_err());
}
