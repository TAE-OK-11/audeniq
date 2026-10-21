use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
#[derive(Debug)]
pub enum Error {
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    Invalid,
    /// Invalid input with a specific, user-explainable reason (HTTP 400).
    InvalidCode(&'static str),
    RateLimited,
    Gated,
    PolicyGate(&'static str),
    Storage,
    UploadBusy,
    Database(sqlx::Error),
    Internal,
    /// Request headers exceed the API limit (HTTP 431).
    HeadersTooLarge,
    /// The request ran past the API's deadline (HTTP 504).
    Timeout,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unauthorized => f.write_str("authentication required"),
            Self::Forbidden => f.write_str("access denied"),
            Self::NotFound => f.write_str("not found"),
            Self::Conflict => f.write_str("concurrent change or duplicate"),
            Self::Invalid => f.write_str("invalid input"),
            Self::InvalidCode(v) => write!(f, "invalid input: {v}"),
            Self::RateLimited => f.write_str("rate limit exceeded"),
            Self::Gated => f.write_str("feature gated"),
            Self::PolicyGate(v) => write!(f, "policy gate: {v}"),
            Self::Storage => f.write_str("storage unavailable"),
            Self::UploadBusy => f.write_str("upload processing capacity reached"),
            Self::Database(_) => f.write_str("database error"),
            Self::Internal => f.write_str("internal error"),
            Self::HeadersTooLarge => f.write_str("request headers too large"),
            Self::Timeout => f.write_str("request timed out"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(e) => Some(e),
            _ => None,
        }
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
/// The stable API error code of a failed response, carried as a response
/// extension so the request log line (api::boundary) records it for 4xx too.
/// Only the static code: never the request body, bind values or user input.
#[derive(Clone, Copy, Debug)]
pub struct ErrorCode(pub &'static str);
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "UNAUTHENTICATED"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            Self::NotFound => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            Self::Conflict => (StatusCode::CONFLICT, "CONFLICT"),
            Self::Invalid => (StatusCode::BAD_REQUEST, "INVALID_INPUT"),
            Self::InvalidCode(code) => (StatusCode::BAD_REQUEST, *code),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED"),
            Self::Gated => (StatusCode::NOT_IMPLEMENTED, "NOT_IMPLEMENTED"),
            Self::PolicyGate(code) => (StatusCode::UNPROCESSABLE_ENTITY, *code),
            Self::Storage => (StatusCode::SERVICE_UNAVAILABLE, "STORAGE_UNAVAILABLE"),
            Self::UploadBusy => (StatusCode::SERVICE_UNAVAILABLE, "UPLOAD_BUSY"),
            Self::Timeout => (StatusCode::GATEWAY_TIMEOUT, "REQUEST_TIMEOUT"),
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
            tracing::warn!(error_code = code, "request failed");
        }
        let body = match message(code) {
            Some(m) => serde_json::json!({"error":{"code":code,"message":m}}),
            None => serde_json::json!({"error":{"code":code}}),
        };
        let mut response = (status, Json(body)).into_response();
        response.extensions_mut().insert(ErrorCode(code));
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
        "UPLOAD_BUSY" => {
            "Uploads are being processed. Retry completion in a few seconds; do not re-upload the file."
        }
        "INVARIANT_CONFLICT" => {
            "The change conflicts with the current state of the release. Reload and try again."
        }
        "INTERNAL_ERROR" => "Something went wrong on our side. Try again later.",
        "SIGNING_REQUEST_REQUIRED" => {
            "Rights documents are signed by the rights holder through a signing request with identity verification."
        }
        "SIGNING_REQUEST_CLOSED" => {
            "This signing request is already signed, declined or cancelled."
        }
        "SIGNING_LINK_EXPIRED" => "This signing link has expired. Ask the sender for a new link.",
        "SIGNING_CONSENT_REQUIRED" => {
            "Confirm the document, the electronic signature and the use of identity data before signing."
        }
        "IDENTITY_PROVIDER_NOT_CONFIGURED" => {
            "Identity verification is not available yet, so documents cannot be signed."
        }
        "IDENTITY_NOT_VERIFIED" => "Identity verification did not complete. Try again.",
        "IDENTITY_NAME_MISMATCH" => {
            "The verified name does not match the signer named in the document."
        }
        "IDENTITY_EXPIRED" => "Identity verification expired. Verify again, then sign.",
        "IDENTITY_ACCOUNT_MISMATCH" => {
            "This account was verified by a different person. Only the account holder can verify it again."
        }
        "SMS_PROVIDER_NOT_CONFIGURED" => {
            "Text message verification is not available yet. Contact support for account help."
        }
        "PHONE_INVALID" => "Enter a Korean mobile number (010-0000-0000).",
        "SMS_CODE_INVALID" => {
            "The verification code is wrong or expired. Request a new code and try again."
        }
        "RESET_CODE_INVALID" => {
            "The verification code is wrong or expired. Check the email and phone number, then request a new code."
        }
        "RESET_TOKEN_INVALID" => {
            "This password reset has expired or was already used. Start the reset again."
        }
        "TAX_COUNTRY_INVALID" => "Choose the country where you have a tax obligation.",
        "TIN_INVALID" => {
            "The taxpayer identification number must be 4 to 30 letters or digits (spaces and hyphens allowed)."
        }
        "UPLOAD_TYPE_UNSUPPORTED" => {
            "Unsupported file type. Audio must be WAV (audio/wav) or FLAC (audio/flac); cover art must be JPEG or PNG."
        }
        "UPLOAD_EMPTY" => "The file is empty.",
        "UPLOAD_UNSAFE_FILE" => {
            "The file failed the security scan and cannot be used. Export a new file from a trusted source."
        }
        "UPLOAD_SANITIZATION_FAILED" => {
            "The file could not be safely processed. Use a static JPEG/PNG or an unencrypted PDF of at most 32 pages."
        }
        "UPLOAD_REINSPECTION_REQUIRED" => {
            "This older file must be uploaded again to pass the current security checks."
        }
        "UPLOAD_AUDIO_TOO_LARGE" => {
            "The audio file is too large. The maximum is 512 MB; export a 16- or 24-bit WAV/FLAC (FLAC is about half the size)."
        }
        "UPLOAD_IMAGE_TOO_LARGE" => "The cover image is too large. The maximum is 20 MB.",
        "UPLOAD_AUDIO_TRUNCATED" => {
            "The WAV file is incomplete: its header promises more audio than the file contains. Export the master again and upload the complete file."
        }
        "UPLOAD_CONTENT_MISMATCH" => {
            "The file's contents do not match its declared type (for example an MP3 or FLAC renamed to .wav). Upload the original WAV or FLAC master with its real type."
        }
        "IMPORT_FETCH_FAILED" => {
            "The file could not be downloaded from the link. Check that it is public and leads straight to the file."
        }
        "IMPORT_SIZE_MISMATCH" => {
            "The download did not match the size the link reported. Try again, or send the file with start_upload."
        }
        "IMPORT_TIMEOUT" => {
            "The download took too long. Use a faster link, or send the file with start_upload."
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
    fn client_errors_carry_their_code_for_the_request_log() {
        let response = Error::InvalidCode("COVER_SIGNATURE_INVALID").into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.extensions().get::<ErrorCode>().map(|c| c.0),
            Some("COVER_SIGNATURE_INVALID")
        );
        let response = Error::Invalid.into_response();
        assert_eq!(
            response.extensions().get::<ErrorCode>().map(|c| c.0),
            Some("INVALID_INPUT")
        );
    }

    #[test]
    fn upload_capacity_is_retryable() {
        let response = Error::UploadBusy.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["retry-after"], "5");
    }

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
