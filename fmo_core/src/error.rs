// Taken from https://github.com/tokio-rs/axum/blob/main/examples/anyhow-error-response/src/main.rs

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

pub(crate) type Result<T> = std::result::Result<T, AppError>;

pub(crate) struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, format!("Error: {}", self.0)).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}
