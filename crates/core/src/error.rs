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
    /// Invalid input with a specific, user-explainable reason (HTTP 400).
    #[error("invalid input: {0}")]
    InvalidCode(&'static str),
    #[error("rate limit exceeded")]
    RateLimited,
    #[error("feature gated")]
    Gated,
    #[error("policy gate: {0}")]
    PolicyGate(&'static str),
    #[error("storage unavailable")]
    Storage,
    #[error("database error")]
    Database(#[from] sqlx::Error),
    #[error("internal error")]
    Internal,
    /// A worker no longer holds the job lease it tried to record a result
    /// under (expired, reclaimed by another worker, or already finished).
    /// Distinct from [`Error::Conflict`] so a lease problem is never
    /// mistaken for (or hides) the handler's own failure.
    #[error("job lease lost")]
    LeaseLost,
    /// Request headers exceed the API limit (HTTP 431).
    #[error("request headers too large")]
    HeadersTooLarge,
}
pub type Result<T> = std::result::Result<T, Error>;
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "UNAUTHENTICATED"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::NotFound => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Conflict => (StatusCode::CONFLICT, "CONFLICT"),
            Self::LeaseLost => (StatusCode::CONFLICT, "LEASE_LOST"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "INVALID_INPUT"),
            Self::InvalidCode(code) => (StatusCode::BAD_REQUEST, *code),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"),
            Self::Gated => (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED"),
            Self::PolicyGate(code) => (StatusCode::UNPROCESSABLE_ENTITY, *code),
            Self::Storage => (StatusCode::SERVICE_UNAVAILABLE, "STORAGE_UNAVAILABLE"),
            Self::HeadersTooLarge => (
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                "REQUEST_HEADERS_TOO_LARGE",
            ),
            Self::Database(sqlx::Error::Database(d))
                if matches!(
                    d.code().as_deref(),
                    Some("23505" | "23503" | "23514" | "55P03" | "40001" | "40P01")
                ) =>
            {
                (StatusCode::CONFLICT, "INVARIANT_CONFLICT")
            }
            Self::Database(e) if db_unavailable(e) => {
                (StatusCode::SERVICE_UNAVAILABLE, "DATABASE_UNAVAILABLE")
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
        };
        // Deliberately never log SQL bind values, credentials, signed URLs, or user payloads.
        if status.is_server_error() {
            // The Display form carries the cause class (e.g. "database error",
            // pool timeout) and, for sqlx, the server message; never bind
            // values or request payloads.
            match &self {
                Self::Database(e) => {
                    tracing::warn!(error_code = code, cause = %e, "request failed")
                }
                other => tracing::warn!(error_code = code, cause = %other, "request failed"),
            }
        }
        let body = match message(code) {
            Some(m) => serde_json::json!({"error":{"code":code,"message":m}}),
            None => serde_json::json!({"error":{"code":code}}),
        };
        let mut response = (status, Json(body)).into_response();
        if status == StatusCode::SERVICE_UNAVAILABLE {
            response
                .headers_mut()
                .insert(axum::http::header::RETRY_AFTER, "5".parse().unwrap());
        }
        response
    }
}

/// The database is unreachable or shutting down, as opposed to a bug or a
/// bad query (sandbox round 2: a Postgres outage answered 500). These are
/// retryable, so the API answers 503 + Retry-After.
pub fn db_unavailable(e: &sqlx::Error) -> bool {
    match e {
        sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::WorkerCrashed => true,
        // 08xxx connection exception, 57P01-57P03 admin/crash shutdown,
        // cannot connect now.
        sqlx::Error::Database(d) => d.code().is_some_and(|c| {
            c.starts_with("08") || matches!(c.as_ref(), "57P01" | "57P02" | "57P03")
        }),
        _ => false,
    }
}

/// Human-readable English explanation for an API error code. Codes stay the
/// stable machine contract; the message is for people (Studio, API users).
pub fn message(code: &str) -> Option<&'static str> {
    Some(match code {
        "UNAUTHENTICATED" => "Sign in to continue.",
        "FORBIDDEN" => "You do not have access to this resource.",
        "NOT_FOUND" => "The requested resource does not exist.",
        "CONFLICT" => {
            "The resource changed or the request was already handled. Reload and try again."
        }
        "INVALID_INPUT" => "The request is missing required fields or contains invalid values.",
        "RATE_LIMITED" => "Too many attempts. Wait a few minutes and try again.",
        "STORAGE_UNAVAILABLE" => "File storage is temporarily unavailable. Try again shortly.",
        "INVARIANT_CONFLICT" => {
            "The change conflicts with the current state of the release. Reload and try again."
        }
        "INTERNAL_ERROR" => "Something went wrong on our side. Try again later.",
        "UPLOAD_TYPE_UNSUPPORTED" => {
            "Unsupported file type. Audio must be WAV (audio/wav) or FLAC (audio/flac); cover art must be JPEG or PNG."
        }
        "UPLOAD_EMPTY" => "The file is empty.",
        "UPLOAD_AUDIO_TOO_LARGE" => {
            "The audio file is too large. The maximum is 512 MB; export a 16- or 24-bit WAV/FLAC (FLAC is about half the size)."
        }
        "UPLOAD_IMAGE_TOO_LARGE" => "The cover image is too large. The maximum is 20 MB.",
        "UPLOAD_CONTENT_MISMATCH" => {
            "The file's contents do not match its declared type (for example an MP3 or FLAC renamed to .wav). Upload the original WAV or FLAC master with its real type."
        }
        "RELEASE_NOT_SUBMITTABLE" => "This release cannot be submitted in its current state.",
        "AUDIO_NOT_VERIFIED" => {
            "An attached audio file was never verified. Upload it again before submitting."
        }
        "TEXT_INVALID_CHARACTERS" => {
            "The text contains control, invisible or text-direction characters (for example NUL, escape codes, zero-width spaces or right-to-left overrides). Remove them and try again."
        }
        "ARTIST_NAME_PROTECTED" => {
            "A name, title or credit matches a protected artist. Releasing under a protected artist name requires verified rights; change the name or contact support if you represent this artist."
        }
        "IDENTIFIER_IN_USE" => {
            "The UPC or an ISRC is already used by another release or track in your account. Assign unique codes."
        }
        "REQUEST_HEADERS_TOO_LARGE" => "The request headers are too large.",
        "TRACK_POSITION_DUPLICATE" => {
            "Two tracks in the request use the same disc and track number."
        }
        "DATABASE_UNAVAILABLE" => {
            "The service is temporarily unavailable. Try again in a few seconds."
        }
        _ => return None,
    })
}

#[cfg(test)]
mod unavailable_tests {
    use super::*;

    #[test]
    fn database_outage_is_503_with_retry_after() {
        let r = Error::Database(sqlx::Error::PoolTimedOut).into_response();
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(r.headers().get("retry-after").unwrap(), "5");
        let r = Error::Database(sqlx::Error::RowNotFound).into_response();
        assert_eq!(r.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(r.headers().get("retry-after").is_none());
    }
}
