//! AUDENIQ's own MCP server: lets an artist prepare, validate and submit a
//! release from an AI assistant (ChatGPT, Claude, any MCP client).
//!
//! It lives inside `audeniq-api` (no separate service) and is layered:
//!
//! - [`oauth`]: OAuth 2.1 authorization server (consent screen on the Studio
//!   session, PKCE, rotating refresh tokens, CIMD + DCR clients). A token is
//!   bound to one user, one workspace and the scopes the user approved.
//! - [`login`]: other ways to connect: device authorization (link + code),
//!   password grant for operator-registered agents, Studio connection tokens.
//! - [`public`]: tools that need no account (notices, events, service
//!   status, release requirements) under daily limits per IP / per user.
//! - [`safety`]: activity notices, retry-safe creates, optional Studio
//!   approval of AI submissions.
//! - [`protocol`]: Streamable HTTP + JSON-RPC (2026-07-28 per-request
//!   metadata and the earlier `initialize`-based revisions).
//! - [`tools`]: tool schemas and handlers. Handlers only call the existing
//!   services (`catalog`, `drafts`, `submission`, `uploads`, ...) with an
//!   [`Actor`](crate::auth::Actor) built from the token, so every
//!   membership/ACL/state check is the one the Studio goes through.
//!
//! Off unless `AUDENIQ_MCP_ENABLED=true`: the endpoints answer 404 until an
//! operator turns them on. See docs/MCP.md.
pub mod login;
pub mod oauth;
pub mod prompts;
pub mod protocol;
pub mod public;
pub mod safety;
pub mod tools;

use crate::api::AppState;
use axum::{
    Router,
    routing::{get, post},
};
use base64::Engine as _;

/// Scopes a client can be granted. Everything but `releases:read` and
/// `releases:write` is opt-in on the consent screen. Nothing grants payout
/// account changes, contracts, signatures or account settings, and payouts
/// above [`Settings::payout_limit_krw`] always go through the Studio.
pub const SCOPE_READ: &str = "releases:read";
pub const SCOPE_WRITE: &str = "releases:write";
pub const SCOPE_SUBMIT: &str = "releases:submit";
pub const SCOPE_SETTLEMENT: &str = "settlement:read";
pub const SCOPE_PAYOUT: &str = "payouts:request";
pub const SCOPES: [&str; 5] = [
    SCOPE_READ,
    SCOPE_WRITE,
    SCOPE_SUBMIT,
    SCOPE_SETTLEMENT,
    SCOPE_PAYOUT,
];

/// Path of the MCP endpoint (behind the edge at the Studio origin).
pub const MCP_PATH: &str = "/api/mcp";

/// Public tools (notices, events, service status, release requirements)
/// need no account and are not counted without one. For a signed-in user:
/// calls per day (KST). Above it the artist contacts support; an operator
/// can raise one user's limit (`audeniq-admin mcp-limit`).
pub const DEFAULT_PUBLIC_DAILY_USER: i32 = 150;

/// Default AI payout limit: 300,000원 per request and per 24 hours.
pub const DEFAULT_PAYOUT_LIMIT_KRW: i64 = 300_000;

#[derive(Clone, Debug)]
pub struct Settings {
    pub enabled: bool,
    /// Hosts (any document path on them) or exact `https://` document URLs
    /// whose Client ID Metadata Documents the server fetches. Only these are
    /// ever contacted (no server-side request to an arbitrary URL a caller
    /// names).
    pub cimd_hosts: Vec<String>,
    /// RFC 7591 registration (deprecated by MCP, still used by clients).
    pub dynamic_registration: bool,
    /// Largest payout (KRW) an AI assistant may request, per request and in
    /// total over 24 hours; above it the artist uses the Studio. 0 = never.
    pub payout_limit_krw: i64,
    /// Public tools per signed-in user per day (unless overridden).
    pub public_daily_user: i32,
}

impl Settings {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            cimd_hosts: Vec::new(),
            dynamic_registration: false,
            payout_limit_krw: 0,
            public_daily_user: DEFAULT_PUBLIC_DAILY_USER,
        }
    }
    pub fn from_env() -> Self {
        let flag = |name: &str, default: bool| match std::env::var(name).as_deref() {
            Ok("true" | "1") => true,
            Ok("false" | "0") => false,
            _ => default,
        };
        let count = |name: &str, default: i32| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.trim().parse::<i32>().ok())
                .filter(|v| *v >= 0)
                .unwrap_or(default)
        };
        let cimd_hosts = std::env::var("AUDENIQ_MCP_CIMD_HOSTS")
            .unwrap_or_else(|_| "chatgpt.com,claude.ai".into())
            .split(',')
            // Hosts compare case-insensitively; exact document URLs as given.
            .map(|h| {
                let h = h.trim();
                if h.starts_with("https://") {
                    h.to_string()
                } else {
                    h.to_ascii_lowercase()
                }
            })
            .filter(|h| !h.is_empty())
            .collect();
        Self {
            enabled: flag("AUDENIQ_MCP_ENABLED", false),
            cimd_hosts,
            dynamic_registration: flag("AUDENIQ_MCP_DYNAMIC_REGISTRATION", true),
            payout_limit_krw: std::env::var("AUDENIQ_MCP_PAYOUT_LIMIT_KRW")
                .ok()
                .and_then(|v| v.trim().parse::<i64>().ok())
                .filter(|v| *v >= 0)
                .unwrap_or(DEFAULT_PAYOUT_LIMIT_KRW),
            public_daily_user: count("AUDENIQ_MCP_PUBLIC_DAILY_USER", DEFAULT_PUBLIC_DAILY_USER),
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            MCP_PATH,
            post(protocol::post)
                .get(protocol::not_allowed)
                .delete(protocol::not_allowed),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(oauth::protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource/api/mcp",
            get(oauth::protected_resource_metadata),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(oauth::authorization_server_metadata),
        )
        .route("/api/oauth/register", post(oauth::register))
        .route(
            "/api/oauth/authorize",
            get(oauth::authorize).post(oauth::decide),
        )
        .route("/api/oauth/token", post(oauth::token))
        .route(
            "/api/oauth/device_authorization",
            post(login::device_authorization),
        )
        .route(
            "/api/oauth/device",
            get(login::device_page).post(login::device_decide),
        )
        .route("/api/oauth/revoke", post(oauth::revoke))
        .route("/api/auth/ai-connections", get(oauth::connections))
        .route(
            "/api/auth/ai-connections/{id}/revoke",
            post(oauth::revoke_connection),
        )
        .route(
            "/api/auth/ai-connections/tokens",
            post(login::create_connection_token),
        )
        .route(
            "/api/auth/ai-settings",
            get(safety::get_settings).put(safety::put_settings),
        )
        .route("/api/auth/ai-submissions", get(safety::list_pending))
        .route(
            "/api/auth/ai-submissions/{id}/approve",
            post(safety::approve),
        )
        .route("/api/auth/ai-submissions/{id}/reject", post(safety::reject))
}

/// Requests the boundary lets through without the Studio `Origin`: AI
/// clients call these server to server and authenticate with the bearer
/// token / PKCE / client credentials instead. A browser that does send an
/// Origin to `/api/mcp` must still send ours (checked in `protocol`).
pub fn origin_exempt(method: &axum::http::Method, path: &str) -> bool {
    *method == axum::http::Method::POST
        && matches!(
            path,
            MCP_PATH
                | "/api/oauth/register"
                | "/api/oauth/token"
                | "/api/oauth/revoke"
                | "/api/oauth/device_authorization"
        )
}

pub(crate) fn issuer(s: &AppState) -> String {
    s.config.origin.trim_end_matches('/').to_string()
}
pub(crate) fn resource(s: &AppState) -> String {
    format!("{}{MCP_PATH}", issuer(s))
}
pub(crate) fn resource_metadata_url(s: &AppState) -> String {
    format!(
        "{}/.well-known/oauth-protected-resource{MCP_PATH}",
        issuer(s)
    )
}

/// HMAC-SHA256 under a key derived from the service secret, per purpose.
pub(crate) fn mac(s: &AppState, purpose: &str, msg: &[u8]) -> [u8; 32] {
    use hmac::{Hmac, KeyInit, Mac};
    let mut k = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(s.config.service_secret.as_bytes())
        .expect("HMAC key");
    k.update(b"audeniq-mcp-v1:");
    k.update(purpose.as_bytes());
    let key: [u8; 32] = k.finalize().into_bytes().into();
    let mut m = <Hmac<sha2::Sha256> as KeyInit>::new_from_slice(&key).expect("HMAC key");
    m.update(msg);
    m.finalize().into_bytes().into()
}

pub(crate) fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// `payload.mac`: a server-signed JSON value (consent requests, submit
/// confirmations). Integrity only; never put secrets in the payload.
pub(crate) fn seal(s: &AppState, purpose: &str, payload: &serde_json::Value) -> String {
    let body = b64(payload.to_string().as_bytes());
    let tag = b64(&mac(s, purpose, body.as_bytes()));
    format!("{body}.{tag}")
}

pub(crate) fn open(s: &AppState, purpose: &str, sealed: &str) -> Option<serde_json::Value> {
    use subtle::ConstantTimeEq;
    if sealed.len() > 8192 {
        return None;
    }
    let (body, tag) = sealed.split_once('.')?;
    let tag = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(tag)
        .ok()?;
    let expected = mac(s, purpose, body.as_bytes());
    if !bool::from(expected.as_slice().ct_eq(&tag)) {
        return None;
    }
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(body)
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let exp = v.get("exp")?.as_i64()?;
    (exp > chrono::Utc::now().timestamp()).then_some(v)
}
