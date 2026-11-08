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

#[test]
fn too_many_nonces_is_a_bad_request() {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use fmo_core::module::ApiError;

    use crate::nonces::{check_nonce_count, MAX_NONCES_PER_REQUEST};

    let nonces = vec![String::new(); MAX_NONCES_PER_REQUEST];
    assert!(check_nonce_count(&nonces).is_ok());
    assert!(check_nonce_count(&[]).is_ok());

    let too_many = vec![String::new(); MAX_NONCES_PER_REQUEST + 1];
    // Through `?` and `.context()`, as the handler returns it
    let error =
        anyhow::Context::context(check_nonce_count(&too_many), "Checking nonces").unwrap_err();
    assert_eq!(
        ApiError::from(error).into_response().status(),
        StatusCode::BAD_REQUEST
    );
}
