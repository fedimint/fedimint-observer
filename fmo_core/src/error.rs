// Taken from https://github.com/tokio-rs/axum/blob/main/examples/anyhow-error-response/src/main.rs

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid bearer token".to_owned(),
        }
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

/// An error the client caused or should see as is, with its HTTP status.
/// Return it through `anyhow` (`Err(ClientError::not_found(..).into())`, or
/// `bail!(ClientError::..)`): it keeps its status through `?` and `.context()`,
/// unlike other errors, which become a sanitized 500.
#[derive(Debug, Clone)]
pub struct ClientError {
    status: StatusCode,
    message: String,
}

impl ClientError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    /// 404: an unknown federation, transaction or other resource
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }
    /// 400: invalid parameters or body
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
    /// 502: an upstream (guardian, gateway) answered wrongly or refused
    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, message)
    }
    /// 504: an upstream (guardian, gateway) did not answer in time
    pub fn gateway_timeout(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GATEWAY_TIMEOUT, message)
    }
    /// 503: the server can't take this request now; retrying later may work
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ClientError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            self.status,
            axum::Json(serde_json::json!({"error": self.message})),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        let error = err.into();
        if let Some(client) = error.downcast_ref::<ClientError>() {
            tracing::debug!(error = ?error, status = %client.status, "Request rejected");
            return Self {
                status: client.status,
                message: client.message.clone(),
            };
        }
        tracing::error!(error = ?error, "Request failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "Internal server error".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;

    #[test]
    fn client_errors_keep_their_status_through_context() {
        let error: anyhow::Result<()> =
            Err(ClientError::not_found("Federation not observed").into());
        let app = AppError::from(error.context("Loading federation").unwrap_err());
        assert_eq!(app.status, StatusCode::NOT_FOUND);
        assert_eq!(app.message, "Federation not observed");
    }

    #[test]
    fn other_errors_are_sanitized() {
        let app = AppError::from(anyhow::anyhow!("connection refused to 10.0.0.1"));
        assert_eq!(app.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(app.message, "Internal server error");
    }
}
