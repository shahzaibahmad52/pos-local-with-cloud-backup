use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Errors returned by the store server. Each maps to an HTTP status and a
/// stable `error` code the tills can switch on; `message` is shown to people.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("Please sign in again.")]
    Unauthorized,
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Stock(String),
    #[error("{0}")]
    ApprovalRequired(String),
    #[error("{0}")]
    Locked(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            AppError::Stock(_) => (StatusCode::CONFLICT, "insufficient_stock"),
            AppError::ApprovalRequired(_) => (StatusCode::FORBIDDEN, "approval_required"),
            AppError::Locked(_) => (StatusCode::TOO_MANY_REQUESTS, "locked"),
            AppError::Db(_) | AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let message = match &self {
            AppError::Db(e) => {
                eprintln!("database error: {e}");
                "Something went wrong saving to the database. Please try again.".to_string()
            }
            AppError::Internal(e) => {
                eprintln!("internal error: {e}");
                "Something went wrong. Please try again.".to_string()
            }
            other => other.to_string(),
        };
        (status, Json(json!({ "error": code, "message": message }))).into_response()
    }
}
