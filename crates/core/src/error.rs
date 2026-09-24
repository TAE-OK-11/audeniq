use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("authentication required")]
    Unauthorized,
    #[error("access denied")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("concurrent change or duplicate")]
    Conflict,
    #[error("invalid input")]
    Invalid,
    #[error("rate limit exceeded")]
    RateLimited,
    #[error("feature gated")]
    Gated,
    #[error("storage unavailable")]
    Storage,
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("internal error")]
    Internal,
}
pub type Result<T> = std::result::Result<T, Error>;
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "UNAUTHENTICATED"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::NotFound => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Conflict => (StatusCode::CONFLICT, "CONFLICT"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "INVALID_INPUT"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"),
            Self::Gated => (StatusCode::NOT_IMPLEMENTED, "PRE_SUBMIT_NOT_IMPLEMENTED"),
            Self::Storage => (StatusCode::SERVICE_UNAVAILABLE, "STORAGE_UNAVAILABLE"),
            Self::Database(sqlx::Error::Database(d))
                if matches!(d.code().as_deref(), Some("23505" | "23503" | "23514" | "55P03" | "40001" | "40P01")) =>
            {
                (StatusCode::CONFLICT, "INVARIANT_CONFLICT")
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };
        // Deliberately never log SQL bind values, credentials, signed URLs, or user payloads.
        if status.is_server_error() {
            tracing::warn!(error_code = code, "request failed");
        }
        (status, Json(serde_json::json!({"error":{"code":code}}))).into_response()
    }
}
