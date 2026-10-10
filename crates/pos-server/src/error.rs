use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

/// Errors returned by the store server. Each maps to an HTTP status and a
/// stable `error` code the tills can switch on; `message` is shown to people.
/// Errors made with [`AppError::because`] also carry a `reason` and `params`
/// so the till can show the message in the cashier's own language.
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
    #[error("{inner}")]
    Detailed { inner: Box<AppError>, reason: &'static str, params: Value },
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    /// Adds a machine-readable reason and the values the message was built from.
    pub fn because(self, reason: &'static str, params: Value) -> AppError {
        AppError::Detailed { inner: Box::new(self), reason, params }
    }

    /// The stable code sent to the till as `error`.
    pub fn code(&self) -> &'static str {
        self.status_and_code().1
    }

    pub fn reason(&self) -> Option<&'static str> {
        match self {
            AppError::Detailed { reason, .. } => Some(reason),
            _ => None,
        }
    }

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
            AppError::Detailed { inner, .. } => inner.status_and_code(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let (inner, reason, params) = match self {
            AppError::Detailed { inner, reason, params } => (*inner, Some(reason), Some(params)),
            other => (other, None, None),
        };
        let message = match &inner {
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
        let mut body = json!({ "error": code, "message": message });
        if let Some(r) = reason {
            body["reason"] = json!(r);
        }
        if let Some(p) = params {
            body["params"] = p;
        }
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detailed_errors_keep_their_code() {
        let e = AppError::Stock("x".into()).because("stock.short", json!({ "need": 3 }));
        assert_eq!(e.code(), "insufficient_stock");
        assert_eq!(e.reason(), Some("stock.short"));
        assert_eq!(e.to_string(), "x");
    }
}
