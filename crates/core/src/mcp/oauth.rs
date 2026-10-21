//! OAuth 2.1 authorization server for AI clients (MCP authorization spec,
//! RFC 8414 / 9728 / 8707 / 7591 / 7009 / 9207, Client ID Metadata
//! Documents).
//!
//! - Consent happens in the browser on the user's existing Studio session;
//!   this server never sees or asks for a password.
//! - Authorization code + PKCE (S256 only). Codes live 10 minutes, once.
//! - Opaque tokens, stored as SHA-256 digests. Access 1 h; refresh 30 days,
//!   rotated on every use, reuse after a 30 s grace revokes the grant; a grant ends 90 days
//!   after consent at the latest.
//! - Every token is bound to one user, one workspace (org), the approved
//!   scopes and this server's MCP resource (audience).
use super::{
    SCOPE_PAYOUT, SCOPE_READ, SCOPE_SETTLEMENT, SCOPE_SUBMIT, SCOPE_WRITE, SCOPES, Settings,
};
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
};
use axum::{
    Json,
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

const CODE_SECONDS: i64 = 600;
pub const ACCESS_SECONDS: i64 = 3600;
const REFRESH_SECONDS: i64 = 30 * 24 * 3600;
const GRANT_MAX_DAYS: i64 = 90;
/// A refresh token reused this soon after its first use is a client retry
/// or two requests of the client refreshing at once; it gets its own new
/// tokens (like Okta's grace period / Auth0's reuse interval).
const REFRESH_REUSE_GRACE_SECONDS: i64 = 30;
/// A consent screen must be answered within this window.
const CONSENT_SECONDS: i64 = 600;
/// CIMD documents are re-fetched after a day; a cached one serves up to a
/// week when the client's host is briefly unreachable.
const CIMD_FRESH_HOURS: i64 = 24;
const CIMD_STALE_DAYS: i64 = 7;
const CIMD_MAX_BYTES: usize = 64 * 1024;

fn enabled(s: &AppState) -> Result<&Settings> {
    if s.mcp.enabled {
        Ok(&s.mcp)
    } else {
        Err(Error::NotFound)
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

pub async fn protected_resource_metadata(State(s): State<AppState>) -> Result<Json<Value>> {
    enabled(&s)?;
    Ok(Json(json!({
        "resource": super::resource(&s),
        "authorization_servers": [super::issuer(&s)],
        "scopes_supported": SCOPES,
        "bearer_methods_supported": ["header"],
        "resource_name": "AUDENIQ",
    })))
}

pub async fn authorization_server_metadata(State(s): State<AppState>) -> Result<Json<Value>> {
    let settings = enabled(&s)?;
    let base = super::issuer(&s);
    let mut v = json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/api/oauth/authorize"),
        "token_endpoint": format!("{base}/api/oauth/token"),
        "revocation_endpoint": format!("{base}/api/oauth/revoke"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "device_authorization_endpoint": format!("{base}/api/oauth/device_authorization"),
        "grant_types_supported": ["authorization_code", "refresh_token", super::login::DEVICE_GRANT, super::login::TOKEN_EXCHANGE],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "revocation_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "scopes_supported": SCOPES,
        "client_id_metadata_document_supported": true,
        "authorization_response_iss_parameter_supported": true,
    });
    if settings.dynamic_registration {
        v["registration_endpoint"] = json!(format!("{base}/api/oauth/register"));
    }
    Ok(Json(v))
}

// ---------------------------------------------------------------------------
// Clients
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Client {
    pub pk: Uuid,
    pub client_id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub auth_method: String,
    secret_hash: Option<Vec<u8>>,
    /// CIMD, DCR or ADMIN (operator-registered).
    pub kind: String,
    /// An operator-registered agent that may send the artist's e-mail and
    /// password (`grant_type=password`); always a confidential client.
    pub password_grant: bool,
}

/// Redirect URI stored for clients that never use the browser redirect
/// (operator-registered agents without one); never redirected to.
pub const NO_REDIRECT: &str = "https://invalid.audeniq.local/none";

/// A redirect URI an AI client may use: HTTPS without fragment or
/// credentials, or an RFC 8252 loopback `http` URI for native clients.
pub fn valid_redirect_uri(raw: &str) -> bool {
    if raw.len() > 512 {
        return false;
    }
    let Ok(u) = url::Url::parse(raw) else {
        return false;
    };
    if u.fragment().is_some() || !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let loopback = match u.host() {
        Some(url::Host::Domain(d)) => d == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    match u.scheme() {
        "https" => u.host().is_some(),
        "http" => loopback,
        _ => false,
    }
}

/// Redirect URI match: exact, except that a loopback URI may use any port
/// (RFC 8252 §7.3; native clients pick a free port at runtime).
fn redirect_matches(registered: &[String], given: &str) -> bool {
    if registered.iter().any(|r| r == given) {
        return true;
    }
    let Ok(g) = url::Url::parse(given) else {
        return false;
    };
    if g.scheme() != "http" {
        return false;
    }
    registered.iter().any(|r| {
        url::Url::parse(r).is_ok_and(|mut r| {
            r.scheme() == "http" && r.host() == g.host() && r.set_port(g.port()).is_ok() && r == g
        })
    })
}

pub(super) fn row_client(r: &sqlx::postgres::PgRow) -> Client {
    Client {
        pk: r.get("id"),
        client_id: r.get("client_id"),
        name: r.get("client_name"),
        redirect_uris: r.get("redirect_uris"),
        auth_method: r.get("token_endpoint_auth_method"),
        secret_hash: r.get("secret_hash"),
        kind: r.get("kind"),
        password_grant: r.get("password_grant"),
    }
}

pub(super) const CLIENT_COLS: &str = "id, client_id, client_name, redirect_uris, token_endpoint_auth_method, secret_hash, kind, fetched_at, password_grant";

/// The registered client for `client_id`. A URL client id is a Client ID
/// Metadata Document: fetched (from allowlisted hosts only) and cached.
pub async fn client(s: &AppState, client_id: &str) -> Result<Option<Client>> {
    let settings = enabled(s)?;
    if client_id.is_empty() || client_id.len() > 512 {
        return Ok(None);
    }
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {CLIENT_COLS}, fetched_at > now() - make_interval(hours => {CIMD_FRESH_HOURS}) AS fresh,
                fetched_at > now() - make_interval(days => {CIMD_STALE_DAYS}) AS usable
         FROM identity.mcp_clients WHERE client_id=$1 AND disabled_at IS NULL AND kind<>'TOKEN'"
    )))
    .bind(client_id)
    .fetch_optional(&s.pool)
    .await?;
    if !client_id.starts_with("https://") {
        return Ok(row.as_ref().map(row_client));
    }
    if !cimd_allowed(settings, client_id) {
        return Ok(None);
    }
    if let Some(r) = &row
        && r.get::<Option<bool>, _>("fresh") == Some(true)
    {
        return Ok(Some(row_client(r)));
    }
    let cached = || {
        row.as_ref()
            .filter(|r| r.get::<Option<bool>, _>("usable") == Some(true))
            .map(row_client)
    };
    // A document that just failed is not fetched again for a few minutes
    // (every authorize/token call of that client would otherwise wait on it).
    if cimd_recently_failed(client_id) {
        return Ok(cached());
    }
    let started = std::time::Instant::now();
    let fetched = fetch_cimd(client_id).await;
    tracing::info!(
        ok = fetched.is_ok(),
        elapsed_ms = started.elapsed().as_millis(),
        "cimd_fetch"
    );
    match fetched {
        Ok(doc) => {
            let (name, uri, redirects) = match parse_cimd(client_id, &doc) {
                Ok(v) => v,
                Err(reason) => {
                    tracing::warn!(reason, "client metadata document rejected");
                    cimd_failed(client_id);
                    return Ok(cached());
                }
            };
            let r = sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO identity.mcp_clients(id,client_id,kind,client_name,client_uri,redirect_uris,token_endpoint_auth_method,fetched_at)
                 VALUES($1,$2,'CIMD',$3,$4,$5,'none',now())
                 ON CONFLICT(client_id) DO UPDATE SET client_name=EXCLUDED.client_name, client_uri=EXCLUDED.client_uri,
                   redirect_uris=EXCLUDED.redirect_uris, fetched_at=now()
                 WHERE identity.mcp_clients.kind='CIMD'
                 RETURNING {CLIENT_COLS}"
            )))
            .bind(Uuid::new_v4())
            .bind(client_id)
            .bind(&name)
            .bind(uri)
            .bind(&redirects)
            .fetch_optional(&s.pool)
            .await?;
            Ok(r.as_ref().map(row_client))
        }
        Err(e) => {
            tracing::warn!(error = %e, "client metadata document unavailable");
            cimd_failed(client_id);
            Ok(cached())
        }
    }
}

/// Client ID Metadata Documents that failed recently (per process).
fn cimd_failures()
-> &'static std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>> {
    static F: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    > = std::sync::OnceLock::new();
    F.get_or_init(Default::default)
}
const CIMD_RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(300);
fn cimd_recently_failed(client_id: &str) -> bool {
    cimd_failures()
        .lock()
        .map(|m| {
            m.get(client_id)
                .is_some_and(|t| t.elapsed() < CIMD_RETRY_AFTER)
        })
        .unwrap_or(false)
}
fn cimd_failed(client_id: &str) {
    if let Ok(mut m) = cimd_failures().lock() {
        m.retain(|_, t| t.elapsed() < CIMD_RETRY_AFTER);
        if m.len() < 1000 {
            m.insert(client_id.to_string(), std::time::Instant::now());
        }
    }
}

fn cimd_allowed(settings: &Settings, client_id: &str) -> bool {
    let Ok(u) = url::Url::parse(client_id) else {
        return false;
    };
    // A domain name (never an address literal, which would skip the
    // public-address check when fetched).
    let Some(url::Host::Domain(domain)) = u.host() else {
        return false;
    };
    u.scheme() == "https"
        && domain.contains('.')
        && u.port().is_none()
        && u.path().len() > 1
        && u.query().is_none()
        && u.fragment().is_none()
        && u.username().is_empty()
        && settings.cimd_hosts.iter().any(|allowed| {
            // An exact document URL pins one client; a bare host allows any
            // document path on it.
            if allowed == "*" {
                true
            } else if allowed.starts_with("https://") {
                allowed == client_id
            } else {
                u.host_str() == Some(allowed.as_str())
            }
        })
}

async fn fetch_cimd(url: &str) -> anyhow::Result<Value> {
    // Any host may be named (`*`): fetch only from public addresses.
    let client = crate::external_http::public_client_builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let mut res = client
        .get(url)
        .header(header::ACCEPT, "application/json")
        .send()
        .await?;
    anyhow::ensure!(res.status().is_success(), "status {}", res.status());
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await? {
        body.extend_from_slice(&chunk);
        anyhow::ensure!(body.len() <= CIMD_MAX_BYTES, "document too large");
    }
    Ok(serde_json::from_slice(&body)?)
}

/// Validates a Client ID Metadata Document: its `client_id` is exactly the
/// URL it came from, it names itself, lists only acceptable redirect URIs and
/// can authenticate as a public client (PKCE, `none`).
pub fn parse_cimd(
    url: &str,
    doc: &Value,
) -> std::result::Result<(String, Option<String>, Vec<String>), &'static str> {
    if doc.get("client_id").and_then(Value::as_str) != Some(url) {
        return Err("client_id does not match the document URL");
    }
    let name = doc
        .get("client_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or("client_name missing")?;
    let name: String = name.chars().filter(|c| !c.is_control()).take(200).collect();
    let redirects: Vec<String> = doc
        .get("redirect_uris")
        .and_then(Value::as_array)
        .ok_or("redirect_uris missing")?
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect::<Option<_>>()
        .ok_or("redirect_uris malformed")?;
    if redirects.is_empty()
        || redirects.len() > 10
        || !redirects.iter().all(|r| valid_redirect_uri(r))
    {
        return Err("redirect_uris invalid");
    }
    let methods: Vec<&str> = match (
        doc.get("token_endpoint_auth_methods_supported")
            .and_then(Value::as_array),
        doc.get("token_endpoint_auth_method")
            .and_then(Value::as_str),
    ) {
        (Some(list), _) => list.iter().filter_map(Value::as_str).collect(),
        (None, Some(m)) => vec![m],
        (None, None) => vec!["none"],
    };
    if !methods.contains(&"none") {
        return Err("client cannot authenticate as a public client");
    }
    let uri = doc
        .get("client_uri")
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("https://") && u.len() <= 512)
        .map(str::to_owned);
    Ok((name, uri, redirects))
}

/// RFC 7591 dynamic client registration. Unknown metadata is ignored.
pub async fn register(State(s): State<AppState>, h: HeaderMap, body: Bytes) -> Response {
    match register_inner(&s, &h, &body).await {
        Ok(v) => (StatusCode::CREATED, Json(v)).into_response(),
        Err(Reg::Oauth(code, desc)) => oauth_error(StatusCode::BAD_REQUEST, code, desc),
        Err(Reg::Api(e)) => e.into_response(),
    }
}

enum Reg {
    Oauth(&'static str, &'static str),
    Api(Error),
}
impl From<Error> for Reg {
    fn from(e: Error) -> Self {
        Reg::Api(e)
    }
}
impl From<sqlx::Error> for Reg {
    fn from(e: sqlx::Error) -> Self {
        Reg::Api(e.into())
    }
}

async fn register_inner(
    s: &AppState,
    h: &HeaderMap,
    body: &[u8],
) -> std::result::Result<Value, Reg> {
    if !enabled(s)?.dynamic_registration {
        return Err(Error::NotFound.into());
    }
    let source = auth::client_ip_key(h).unwrap_or_else(|| "unknown".into());
    auth::rate(&s.pool, &format!("mcp-register:src:{source}"), 20).await?;
    auth::rate(&s.pool, "mcp-register:all", 2000).await?;
    let meta: Value = serde_json::from_slice(body)
        .map_err(|_| Reg::Oauth("invalid_client_metadata", "Body must be a JSON object."))?;
    let bad = |d| Reg::Oauth("invalid_redirect_uri", d);
    let redirects: Vec<String> = meta
        .get("redirect_uris")
        .and_then(Value::as_array)
        .ok_or(bad("redirect_uris is required."))?
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect::<Option<_>>()
        .ok_or(bad("redirect_uris must be strings."))?;
    if redirects.is_empty()
        || redirects.len() > 10
        || !redirects.iter().all(|r| valid_redirect_uri(r))
    {
        return Err(bad(
            "Each redirect URI must be https (or http on a loopback address), without a fragment.",
        ));
    }
    let method = meta
        .get("token_endpoint_auth_method")
        .and_then(Value::as_str)
        .unwrap_or("client_secret_basic");
    if !matches!(
        method,
        "none" | "client_secret_post" | "client_secret_basic"
    ) {
        return Err(Reg::Oauth(
            "invalid_client_metadata",
            "token_endpoint_auth_method must be none, client_secret_post or client_secret_basic.",
        ));
    }
    // Clients list grants by habit (client_credentials, implicit, ...): the
    // registration answers with what this server grants (RFC 7591 §3.2.1)
    // and refuses only a client that cannot use the code flow at all.
    for (field, needed) in [
        ("grant_types", "authorization_code"),
        ("response_types", "code"),
    ] {
        if let Some(list) = meta.get(field) {
            let ok = list.as_array().is_some_and(|a| {
                a.iter().all(Value::is_string) && a.iter().any(|v| v.as_str() == Some(needed))
            });
            if !ok {
                return Err(Reg::Oauth(
                    "invalid_client_metadata",
                    "The authorization_code grant with response type code is required.",
                ));
            }
        }
    }
    let name: String = meta
        .get("client_name")
        .and_then(Value::as_str)
        .map(|n| {
            n.chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect::<String>()
        })
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Unnamed AI client".into());
    let client_uri = meta
        .get("client_uri")
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("https://") && u.len() <= 512);
    let client_id = format!("audeniq-mcp-{}", &auth::random_token()[..32]);
    let secret = (method != "none").then(auth::random_token);
    let mut tx = s.pool.begin().await?;
    sqlx::query("INSERT INTO identity.mcp_clients(id,client_id,kind,client_name,client_uri,redirect_uris,token_endpoint_auth_method,secret_hash) VALUES($1,$2,'DCR',$3,$4,$5,$6,$7)")
        .bind(Uuid::new_v4())
        .bind(&client_id)
        .bind(&name)
        .bind(client_uri)
        .bind(&redirects)
        .bind(method)
        .bind(secret.as_deref().map(auth::hash_token))
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        None,
        None,
        None,
        "mcp.client_registered",
        "DYNAMIC_REGISTRATION",
        auth::request_id(h),
    )
    .await?;
    tx.commit().await?;
    let mut out = json!({
        "client_id": client_id,
        "client_id_issued_at": chrono::Utc::now().timestamp(),
        "client_name": name,
        "redirect_uris": redirects,
        "token_endpoint_auth_method": method,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
    });
    if let Some(secret) = secret {
        out["client_secret"] = json!(secret);
        out["client_secret_expires_at"] = json!(0);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Authorization endpoint (consent screen)
// ---------------------------------------------------------------------------

pub(super) fn single_params(raw: &str) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (k, v) in url::form_urlencoded::parse(raw.as_bytes()) {
        // RFC 6749 §3.1: parameters must not repeat.
        if out.insert(k.into_owned(), v.into_owned()).is_some() {
            return None;
        }
    }
    Some(out)
}

/// The scopes a request names, defaulting to read + write. Identity scopes
/// some clients add by habit (openid, offline_access, ...) are ignored; any
/// other unknown scope is an error.
pub fn requested_scopes(raw: Option<&str>) -> Option<Vec<&'static str>> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Some(vec![SCOPE_READ, SCOPE_WRITE]);
    };
    let mut out = Vec::new();
    for s in raw.split(' ').filter(|s| !s.is_empty()) {
        match SCOPES.iter().find(|k| **k == s) {
            Some(k) => {
                if !out.contains(k) {
                    out.push(*k)
                }
            }
            None if matches!(s, "openid" | "email" | "profile" | "offline_access") => {}
            None => return None,
        }
    }
    if out.is_empty() {
        out = vec![SCOPE_READ, SCOPE_WRITE];
    }
    if !out.contains(&SCOPE_READ) {
        out.insert(0, SCOPE_READ);
    }
    Some(out)
}

/// RFC 8707 resource: this server's MCP endpoint (scheme/host compared
/// case-insensitively, trailing slash tolerated). Absent = this server.
pub(super) fn resource_ok(s: &AppState, given: Option<&str>) -> bool {
    let Some(given) = given else { return true };
    let ours = super::resource(s);
    let norm = |v: &str| -> Option<String> {
        let u = url::Url::parse(v).ok()?;
        if u.fragment().is_some() {
            return None;
        }
        Some(u.as_str().trim_end_matches('/').to_string())
    };
    let issuer = super::issuer(s);
    match norm(given) {
        Some(g) => g == norm(&ours).unwrap_or_default() || g == issuer,
        None => false,
    }
}

fn redirect_with(redirect_uri: &str, pairs: &[(&str, &str)]) -> Response {
    let Ok(mut u) = url::Url::parse(redirect_uri) else {
        return Error::Invalid.into_response();
    };
    {
        let mut q = u.query_pairs_mut();
        for (k, v) in pairs {
            q.append_pair(k, v);
        }
    }
    let mut r = StatusCode::SEE_OTHER.into_response();
    if let Ok(v) = HeaderValue::from_str(u.as_str()) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

fn redirect_error(
    s: &AppState,
    redirect_uri: &str,
    state: Option<&str>,
    code: &str,
    desc: &str,
) -> Response {
    let iss = super::issuer(s);
    let mut pairs = vec![
        ("error", code),
        ("error_description", desc),
        ("iss", iss.as_str()),
    ];
    if let Some(st) = state {
        pairs.push(("state", st));
    }
    redirect_with(redirect_uri, &pairs)
}

pub async fn authorize(State(s): State<AppState>, h: HeaderMap, RawQuery(q): RawQuery) -> Response {
    if enabled(&s).is_err() {
        return Error::NotFound.into_response();
    }
    let source = auth::client_ip_key(&h).unwrap_or_else(|| "unknown".into());
    if auth::rate(&s.pool, &format!("mcp-authorize:src:{source}"), 300)
        .await
        .is_err()
    {
        return page_error("요청이 너무 많아요. 잠시 뒤에 다시 해 주세요.");
    }
    let Some(p) = single_params(q.as_deref().unwrap_or("")) else {
        return page_error("요청 형식이 올바르지 않아요 (같은 항목이 두 번 들어 있어요).");
    };
    let client_id = p.get("client_id").map(String::as_str).unwrap_or("");
    let client = match client(&s, client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return page_error("등록되지 않았거나 허용되지 않은 AI 앱이에요."),
        Err(e) => return e.into_response(),
    };
    let redirect_uri = match p.get("redirect_uri") {
        Some(r) if redirect_matches(&client.redirect_uris, r) => r.clone(),
        None if client.redirect_uris.len() == 1 => client.redirect_uris[0].clone(),
        _ => return page_error("이 AI 앱에 등록된 돌아갈 주소가 아니에요. 연결을 중단했어요."),
    };
    if redirect_uri == NO_REDIRECT {
        return page_error(
            "이 AI 앱은 브라우저 연결을 쓰지 않아요. AI 앱에서 안내하는 방법으로 연결해 주세요.",
        );
    }
    let state = p.get("state").map(String::as_str);
    if state.is_some_and(|st| st.len() > 512) {
        return redirect_error(&s, &redirect_uri, None, "invalid_request", "state too long");
    }
    if p.get("response_type").map(String::as_str) != Some("code") {
        return redirect_error(
            &s,
            &redirect_uri,
            state,
            "unsupported_response_type",
            "response_type must be code",
        );
    }
    let challenge = p.get("code_challenge").map(String::as_str).unwrap_or("");
    if p.get("code_challenge_method").map(String::as_str) != Some("S256")
        || challenge.len() != 43
        || !challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return redirect_error(
            &s,
            &redirect_uri,
            state,
            "invalid_request",
            "PKCE with S256 is required",
        );
    }
    if !resource_ok(&s, p.get("resource").map(String::as_str)) {
        return redirect_error(
            &s,
            &redirect_uri,
            state,
            "invalid_target",
            "unknown resource",
        );
    }
    let Some(mut scopes) = requested_scopes(p.get("scope").map(String::as_str)) else {
        return redirect_error(&s, &redirect_uri, state, "invalid_scope", "unknown scope");
    };
    // Payouts through an assistant are switched off by a limit of 0.
    if s.mcp.payout_limit_krw == 0 {
        scopes.retain(|v| *v != SCOPE_PAYOUT);
    }
    let a = match auth::actor(&s.pool, &h, &s.config, false).await {
        Ok(a) => a,
        Err(Error::Unauthorized) => return page_sign_in(&s, "/api/oauth/authorize", &p),
        Err(e) => return e.into_response(),
    };
    let orgs = match user_orgs(&s, a.user).await {
        Ok(v) => v,
        Err(e) => return e.into_response(),
    };
    let request = super::seal(
        &s,
        "consent",
        &json!({
            "c": client.pk, "r": redirect_uri, "ch": challenge, "st": state,
            "sc": scopes, "sh": crate::hex::encode(&a.session_hash),
            "exp": chrono::Utc::now().timestamp() + CONSENT_SECONDS,
        }),
    );
    let redirect = url::Url::parse(&redirect_uri).ok();
    let host = redirect
        .as_ref()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    page_consent(
        &client,
        &ConsentTarget {
            action: "/api/oauth/authorize",
            note: format!("승인 후 돌아갈 곳: <code>{}</code>", escape(&host)),
            form_origin: redirect.map(|u| u.origin().ascii_serialization()),
        },
        &scopes,
        &orgs,
        &request,
        s.mcp.payout_limit_krw,
    )
}

/// The workspaces a user can connect, editable ones first.
pub(super) async fn user_orgs(s: &AppState, user: Uuid) -> Result<Vec<(Uuid, String, String)>> {
    Ok(sqlx::query_as(
        "SELECT o.id, o.name, m.role FROM identity.orgs o JOIN identity.memberships m ON m.org_id=o.id
          WHERE m.user_id=$1 AND m.status='ACTIVE' ORDER BY (m.role='VIEWER'), o.created_at, o.id LIMIT 100",
    )
    .bind(user)
    .fetch_all(&s.pool)
    .await?)
}

/// The scopes ticked on a consent form, among those offered. Read is always
/// included.
pub(super) fn scopes_from_form(form: &BTreeMap<String, String>, offered: &[String]) -> Vec<String> {
    let mut scopes = vec![SCOPE_READ.to_string()];
    for (field, scope) in [
        ("scope_write", SCOPE_WRITE),
        ("scope_submit", SCOPE_SUBMIT),
        ("scope_settlement", SCOPE_SETTLEMENT),
        ("scope_payout", SCOPE_PAYOUT),
    ] {
        if form.get(field).map(String::as_str) == Some("on") && offered.iter().any(|o| o == scope) {
            scopes.push(scope.to_string());
        }
    }
    scopes
}

/// A new grant for an active client. `replace`: one live grant per client,
/// user and workspace, so re-connecting the same app replaces the old one
/// (connection tokens are separate grants each).
#[allow(clippy::too_many_arguments)]
pub(super) async fn create_grant(
    c: &mut PgConnection,
    s: &AppState,
    client_pk: Uuid,
    user: Uuid,
    org: Uuid,
    scopes: &[String],
    method: &str,
    label: Option<&str>,
    replace: bool,
    request: Uuid,
) -> Result<Uuid> {
    let disabled: Option<bool> =
        sqlx::query_scalar("SELECT disabled_at IS NOT NULL FROM identity.mcp_clients WHERE id=$1")
            .bind(client_pk)
            .fetch_optional(&mut *c)
            .await?;
    if disabled != Some(false) {
        return Err(Error::Forbidden);
    }
    if replace {
        let replaced: Vec<Uuid> = sqlx::query_scalar("UPDATE identity.mcp_grants SET revoked_at=now(), revoke_reason='REPLACED' WHERE client_pk=$1 AND user_id=$2 AND org_id=$3 AND revoked_at IS NULL RETURNING id")
            .bind(client_pk).bind(user).bind(org).fetch_all(&mut *c).await?;
        if !replaced.is_empty() {
            sqlx::query("DELETE FROM identity.mcp_tokens WHERE grant_id = ANY($1)")
                .bind(&replaced)
                .execute(&mut *c)
                .await?;
        }
    }
    let grant = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.mcp_grants(id,client_pk,user_id,org_id,scopes,resource,method,label) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(grant).bind(client_pk).bind(user).bind(org).bind(scopes).bind(super::resource(s)).bind(method).bind(label)
        .execute(&mut *c).await?;
    // Tell the user (Studio notice + e-mail): a connection they did not make
    // is noticed at once.
    let client: String =
        sqlx::query_scalar("SELECT client_name FROM identity.mcp_clients WHERE id=$1")
            .bind(client_pk)
            .fetch_one(&mut *c)
            .await?;
    let (title, detail) = if method == "TOKEN" {
        (
            "연결 토큰을 만들었어요".to_string(),
            format!(
                "‘{}’ 연결 토큰을 만들었어요. 내가 만든 게 아니라면 프로필 → 연결된 AI 앱에서 바로 끊고 비밀번호를 바꿔 주세요.",
                label.unwrap_or("")
            ),
        )
    } else {
        (
            "새 AI 앱이 연결됐어요".to_string(),
            format!(
                "{client}이(가) 어데닉 계정에 연결됐어요. 내가 연결하지 않았다면 프로필 → 연결된 AI 앱에서 바로 끊고 비밀번호를 바꿔 주세요."
            ),
        )
    };
    sqlx::query("SELECT identity.mcp_notice($1,$2,'ACCOUNT',$3,$4,'/profile?open=ai')")
        .bind(org)
        .bind(user)
        .bind(&title)
        .bind(&detail)
        .execute(&mut *c)
        .await?;
    operations::audit(
        c,
        Some(user),
        Some(org),
        Some(grant),
        "mcp.grant_approved",
        &format!("{method}:{}", scopes.join(",")),
        request,
    )
    .await?;
    Ok(grant)
}

/// The consent form's answer. Bound to the signed request and the session
/// that saw the screen; the boundary has already required our Origin.
pub async fn decide(State(s): State<AppState>, h: HeaderMap, body: Bytes) -> Response {
    if enabled(&s).is_err() {
        return Error::NotFound.into_response();
    }
    match decide_inner(&s, &h, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn decide_inner(s: &AppState, h: &HeaderMap, body: &[u8]) -> Result<Response> {
    auth::origin(h, &s.config)?;
    let a = auth::actor(&s.pool, h, &s.config, false).await?;
    let form = single_params(std::str::from_utf8(body).map_err(|_| Error::Invalid)?)
        .ok_or(Error::Invalid)?;
    let req = super::open(
        s,
        "consent",
        form.get("request").map(String::as_str).unwrap_or(""),
    )
    .ok_or(Error::InvalidCode("CONSENT_EXPIRED"))?;
    if req["sh"].as_str() != Some(crate::hex::encode(&a.session_hash).as_str()) {
        return Err(Error::Forbidden);
    }
    let client_pk: Uuid = req["c"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Invalid)?;
    let redirect_uri = req["r"].as_str().ok_or(Error::Invalid)?.to_string();
    let state = req["st"].as_str().map(str::to_owned);
    let iss = super::issuer(s);
    let with_state = |mut pairs: Vec<(&'static str, String)>| {
        pairs.push(("iss", iss.clone()));
        if let Some(st) = &state {
            pairs.push(("state", st.clone()));
        }
        pairs
    };
    if form.get("decision").map(String::as_str) != Some("approve") {
        let mut tx = s.pool.begin().await?;
        operations::audit(
            &mut tx,
            Some(a.user),
            None,
            Some(client_pk),
            "mcp.consent_denied",
            "USER_DENIED",
            a.request,
        )
        .await?;
        tx.commit().await?;
        let pairs = with_state(vec![
            ("error", "access_denied".into()),
            ("error_description", "The user denied access.".into()),
        ]);
        let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        return Ok(redirect_with(&redirect_uri, &refs));
    }
    let org: Uuid = form
        .get("org")
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Invalid)?;
    let offered: Vec<String> =
        serde_json::from_value(req["sc"].clone()).map_err(|_| Error::Invalid)?;
    let scopes = scopes_from_form(&form, &offered);
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, &a, org, false).await?;
    let grant = create_grant(
        &mut tx, s, client_pk, a.user, org, &scopes, "CONSENT", None, true, a.request,
    )
    .await?;
    let code = auth::random_token();
    sqlx::query("INSERT INTO identity.mcp_codes(code_hash,grant_id,redirect_uri,code_challenge,expires_at) VALUES($1,$2,$3,$4,now()+make_interval(secs=>$5))")
        .bind(auth::hash_token(&code)).bind(grant).bind(&redirect_uri)
        .bind(req["ch"].as_str().ok_or(Error::Invalid)?).bind(CODE_SECONDS as f64)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    let pairs = with_state(vec![("code", code)]);
    let refs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    Ok(redirect_with(&redirect_uri, &refs))
}

// ---------------------------------------------------------------------------
// Token and revocation endpoints
// ---------------------------------------------------------------------------

pub fn oauth_error(status: StatusCode, code: &str, desc: &str) -> Response {
    let mut r = (
        status,
        Json(json!({"error": code, "error_description": desc})),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        r.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"audeniq\""),
        );
    }
    r
}

/// Client authentication at the token/revocation endpoints, by the method
/// the client registered. Returns the client or an OAuth error response.
pub(super) async fn authenticate_client(
    s: &AppState,
    h: &HeaderMap,
    form: &BTreeMap<String, String>,
) -> std::result::Result<Client, Box<Response>> {
    let invalid = || {
        Box::new(oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "Client authentication failed.",
        ))
    };
    let basic = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        .and_then(|v| {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .decode(v.trim())
                .ok()
        })
        .and_then(|v| String::from_utf8(v).ok())
        .and_then(|v| {
            let (id, secret) = v.split_once(':')?;
            let dec = |x: &str| {
                url::form_urlencoded::parse(format!("v={x}").as_bytes())
                    .next()
                    .map(|(_, v)| v.into_owned())
            };
            Some((dec(id)?, dec(secret)?))
        });
    let (client_id, secret, method) =
        match (&basic, form.get("client_id"), form.get("client_secret")) {
            (Some((id, secret)), body_id, None) if body_id.is_none_or(|b| b == id) => {
                (id.clone(), Some(secret.clone()), "client_secret_basic")
            }
            (None, Some(id), Some(secret)) => {
                (id.clone(), Some(secret.clone()), "client_secret_post")
            }
            (None, Some(id), None) => (id.clone(), None, "none"),
            _ => return Err(invalid()),
        };
    let c = match client(s, &client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return Err(invalid()),
        Err(e) => return Err(Box::new(token_api_error(e))),
    };
    if c.auth_method != method {
        return Err(invalid());
    }
    if let (Some(expected), Some(secret)) = (&c.secret_hash, &secret) {
        use subtle::ConstantTimeEq;
        if !bool::from(auth::hash_token(secret).ct_eq(expected)) {
            return Err(invalid());
        }
    }
    Ok(c)
}

fn verifier_ok(v: &str) -> bool {
    (43..=128).contains(&v.len())
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
}

pub fn pkce_s256(verifier: &str) -> String {
    use sha2::Digest;
    super::b64(&sha2::Sha256::digest(verifier.as_bytes()))
}

pub async fn token(State(s): State<AppState>, h: HeaderMap, body: Bytes) -> Response {
    if enabled(&s).is_err() {
        return Error::NotFound.into_response();
    }
    let Some(form) = std::str::from_utf8(&body).ok().and_then(single_params) else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Malformed form body.",
        );
    };
    // Only failed requests count: one AI platform's servers refresh tokens
    // for all of its users from a few addresses, so counting every request
    // (or every request of a public client) would lock all of them out.
    // Codes and tokens are 256-bit random; the limit stops floods of bad ones.
    let source = auth::client_ip_key(&h).unwrap_or_else(|| "unknown".into());
    let failures = format!("mcp-token-fail:src:{source}");
    match auth::over_limit(&s.pool, &failures, TOKEN_FAILURES_PER_SOURCE).await {
        Ok(false) => {}
        Ok(true) => return token_api_error(Error::RateLimited),
        Err(e) => return token_api_error(e),
    }
    let failed = || async {
        let _ = auth::rate(&s.pool, &failures, i32::MAX).await;
    };
    // Token exchange authenticates with the agent key itself (no client).
    if form.get("grant_type").map(String::as_str) == Some(super::login::TOKEN_EXCHANGE) {
        if !resource_ok(&s, form.get("resource").map(String::as_str)) {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "invalid_target",
                "Unknown resource.",
            );
        }
        return match super::login::exchange_agent_key(&s, &form).await {
            Ok(v) => {
                let mut r = Json(v).into_response();
                r.headers_mut()
                    .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
                r.headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                r
            }
            Err(TokenError::Oauth(code, desc)) => {
                failed().await;
                oauth_error(StatusCode::BAD_REQUEST, code, desc)
            }
            Err(TokenError::Client(code, desc)) => {
                failed().await;
                oauth_error(StatusCode::UNAUTHORIZED, code, desc)
            }
            Err(TokenError::Api(e)) => token_api_error(e),
        };
    }
    let c = match authenticate_client(&s, &h, &form).await {
        Ok(c) => c,
        Err(r) => {
            failed().await;
            return *r;
        }
    };
    let grant_type = form.get("grant_type").map(String::as_str);
    if !resource_ok(&s, form.get("resource").map(String::as_str)) {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_target",
            "Unknown resource.",
        );
    }
    let result = match grant_type {
        Some("authorization_code") => exchange_code(&s, &h, &c, &form).await,
        Some("refresh_token") => refresh(&s, &h, &c, &form).await,
        Some(super::login::DEVICE_GRANT) => super::login::exchange_device_code(&s, &c, &form).await,
        Some("password") => super::login::password_grant(&s, &h, &c, &form).await,
        _ => Err(TokenError::Oauth(
            "unsupported_grant_type",
            "Use authorization_code, refresh_token or the device_code grant.",
        )),
    };
    match result {
        Ok(v) => {
            let mut r = Json(v).into_response();
            r.headers_mut()
                .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
            r.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            r
        }
        Err(TokenError::Oauth(code, desc)) => {
            if !matches!(code, "authorization_pending" | "slow_down") {
                failed().await;
            }
            oauth_error(StatusCode::BAD_REQUEST, code, desc)
        }
        Err(TokenError::Client(code, desc)) => {
            failed().await;
            oauth_error(StatusCode::UNAUTHORIZED, code, desc)
        }
        Err(TokenError::Api(e)) => token_api_error(e),
    }
}

/// Failed token requests per client address per 15 minutes.
const TOKEN_FAILURES_PER_SOURCE: i32 = 300;

/// Server-side failures at the token endpoint in OAuth's shape (`error` is a
/// string), so client libraries retry instead of dropping the connection.
pub(super) fn token_api_error(e: Error) -> Response {
    let (status, code, desc, retry) = match crate::mcp::protocol::error_code(&e) {
        "RATE_LIMITED" => (
            StatusCode::TOO_MANY_REQUESTS,
            "temporarily_unavailable",
            "Too many requests. Retry later.",
            Some("60"),
        ),
        "DATABASE_UNAVAILABLE" | "INVARIANT_CONFLICT" => (
            StatusCode::SERVICE_UNAVAILABLE,
            "temporarily_unavailable",
            "Temporarily unavailable. Retry shortly.",
            Some("5"),
        ),
        "INTERNAL_ERROR" => {
            tracing::warn!(error = %e, "mcp token endpoint failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "The server could not complete the request.",
                None,
            )
        }
        _ => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The request was refused.",
            None,
        ),
    };
    let mut r = oauth_error(status, code, desc);
    if let Some(v) = retry {
        r.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static(v));
    }
    r
}

pub(super) enum TokenError {
    Oauth(&'static str, &'static str),
    /// 401: the client may not use this grant.
    Client(&'static str, &'static str),
    Api(Error),
}
impl From<Error> for TokenError {
    fn from(e: Error) -> Self {
        TokenError::Api(e)
    }
}
impl From<sqlx::Error> for TokenError {
    fn from(e: sqlx::Error) -> Self {
        TokenError::Api(e.into())
    }
}
pub(super) const BAD_GRANT: TokenError =
    TokenError::Oauth("invalid_grant", "The grant is invalid, expired or revoked.");

/// The grant is still usable: not revoked, inside its lifetime, its user and
/// membership still active, its client not disabled.
pub(super) async fn live_grant(
    c: &mut PgConnection,
    grant: Uuid,
) -> std::result::Result<Vec<String>, TokenError> {
    let scopes: Option<Vec<String>> = sqlx::query_scalar(
        "SELECT g.scopes FROM identity.mcp_grants g
           JOIN identity.users u ON u.id=g.user_id AND u.status='ACTIVE'
           JOIN identity.memberships m ON m.org_id=g.org_id AND m.user_id=g.user_id AND m.status='ACTIVE'
           JOIN identity.mcp_clients cl ON cl.id=g.client_pk AND cl.disabled_at IS NULL
          WHERE g.id=$1 AND g.revoked_at IS NULL AND g.created_at > now() - make_interval(days => $2)
          FOR UPDATE OF g",
    )
    .bind(grant)
    .bind(GRANT_MAX_DAYS as i32)
    .fetch_optional(&mut *c)
    .await?;
    scopes.ok_or(BAD_GRANT)
}

pub(super) async fn issue_tokens(
    c: &mut PgConnection,
    grant: Uuid,
    scopes: &[String],
) -> Result<Value> {
    let access = auth::random_token();
    let refresh = auth::random_token();
    sqlx::query("INSERT INTO identity.mcp_tokens(token_hash,grant_id,kind,expires_at) VALUES($1,$3,'ACCESS',now()+make_interval(secs=>$4)),($2,$3,'REFRESH',now()+make_interval(secs=>$5))")
        .bind(auth::hash_token(&access)).bind(auth::hash_token(&refresh)).bind(grant)
        .bind(ACCESS_SECONDS as f64).bind(REFRESH_SECONDS as f64)
        .execute(&mut *c).await?;
    // Housekeeping: this grant's spent and expired tokens.
    sqlx::query("DELETE FROM identity.mcp_tokens WHERE grant_id=$1 AND (expires_at < now() - interval '1 day' OR (kind='REFRESH' AND used_at < now() - interval '1 day'))")
        .bind(grant).execute(&mut *c).await?;
    Ok(json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_SECONDS,
        "refresh_token": refresh,
        "scope": scopes.join(" "),
    }))
}

pub(super) async fn revoke_grant(c: &mut PgConnection, grant: Uuid, reason: &str) -> Result<()> {
    sqlx::query("UPDATE identity.mcp_grants SET revoked_at=COALESCE(revoked_at,now()), revoke_reason=COALESCE(revoke_reason,$2) WHERE id=$1")
        .bind(grant).bind(reason).execute(&mut *c).await?;
    sqlx::query("DELETE FROM identity.mcp_tokens WHERE grant_id=$1")
        .bind(grant)
        .execute(&mut *c)
        .await?;
    Ok(())
}

async fn exchange_code(
    s: &AppState,
    h: &HeaderMap,
    client: &Client,
    form: &BTreeMap<String, String>,
) -> std::result::Result<Value, TokenError> {
    let code = form.get("code").map(String::as_str).unwrap_or("");
    let verifier = form.get("code_verifier").map(String::as_str).unwrap_or("");
    if code.len() != 64 || !verifier_ok(verifier) {
        return Err(TokenError::Oauth(
            "invalid_request",
            "code and a valid code_verifier are required.",
        ));
    }
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT k.grant_id, k.redirect_uri, k.code_challenge, k.expires_at > now() AS live, k.used_at IS NOT NULL AS used, g.client_pk, g.user_id, g.org_id
           FROM identity.mcp_codes k JOIN identity.mcp_grants g ON g.id=k.grant_id
          WHERE k.code_hash=$1 FOR UPDATE OF k",
    )
    .bind(auth::hash_token(code))
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(BAD_GRANT)?;
    let grant: Uuid = row.get("grant_id");
    if row.get::<Uuid, _>("client_pk") != client.pk {
        return Err(BAD_GRANT);
    }
    if row.get::<bool, _>("used") {
        // A replayed code: the first exchange may have leaked (RFC 9700 §4.2.4).
        revoke_grant(&mut tx, grant, "CODE_REPLAYED").await?;
        operations::audit(
            &mut tx,
            Some(row.get("user_id")),
            Some(row.get("org_id")),
            Some(grant),
            "mcp.grant_revoked",
            "CODE_REPLAYED",
            auth::request_id(h),
        )
        .await?;
        tx.commit().await?;
        return Err(BAD_GRANT);
    }
    if !row.get::<bool, _>("live") {
        return Err(BAD_GRANT);
    }
    let redirect: String = row.get("redirect_uri");
    if form.get("redirect_uri").is_some_and(|r| *r != redirect) {
        return Err(BAD_GRANT);
    }
    use subtle::ConstantTimeEq;
    let challenge: String = row.get("code_challenge");
    if !bool::from(pkce_s256(verifier).as_bytes().ct_eq(challenge.as_bytes())) {
        return Err(BAD_GRANT);
    }
    sqlx::query("UPDATE identity.mcp_codes SET used_at=now() WHERE code_hash=$1")
        .bind(auth::hash_token(code))
        .execute(&mut *tx)
        .await?;
    let scopes = live_grant(&mut tx, grant).await?;
    let v = issue_tokens(&mut tx, grant, &scopes).await?;
    tx.commit().await?;
    Ok(v)
}

async fn refresh(
    s: &AppState,
    h: &HeaderMap,
    client: &Client,
    form: &BTreeMap<String, String>,
) -> std::result::Result<Value, TokenError> {
    let token = form.get("refresh_token").map(String::as_str).unwrap_or("");
    if token.len() != 64 {
        return Err(BAD_GRANT);
    }
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT t.grant_id, t.expires_at > now() AS live, t.used_at IS NOT NULL AS used,
                COALESCE(t.used_at > now() - make_interval(secs => $2), false) AS just_used, g.client_pk, g.user_id, g.org_id
           FROM identity.mcp_tokens t JOIN identity.mcp_grants g ON g.id=t.grant_id
          WHERE t.token_hash=$1 AND t.kind='REFRESH' FOR UPDATE OF t",
    )
    .bind(auth::hash_token(token))
    .bind(REFRESH_REUSE_GRACE_SECONDS as f64)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(BAD_GRANT)?;
    let grant: Uuid = row.get("grant_id");
    if row.get::<Uuid, _>("client_pk") != client.pk {
        return Err(BAD_GRANT);
    }
    // The same refresh token twice within seconds is a client racing itself
    // (parallel calls, a retry after a lost response), not theft. Refusing it
    // is not safe either: MCP clients drop their stored tokens on
    // invalid_grant, which can also drop the ones the first request just
    // saved and disconnect the user. Answer it with new tokens of its own.
    let just_used = row.get::<bool, _>("just_used");
    if row.get::<bool, _>("used") && !just_used {
        revoke_grant(&mut tx, grant, "REFRESH_REUSED").await?;
        operations::audit(
            &mut tx,
            Some(row.get("user_id")),
            Some(row.get("org_id")),
            Some(grant),
            "mcp.grant_revoked",
            "REFRESH_REUSED",
            auth::request_id(h),
        )
        .await?;
        tx.commit().await?;
        return Err(BAD_GRANT);
    }
    if !row.get::<bool, _>("live") {
        return Err(BAD_GRANT);
    }
    let scopes = live_grant(&mut tx, grant).await?;
    if let Some(asked) = form.get("scope")
        && !asked
            .split(' ')
            .filter(|v| !v.is_empty())
            .all(|v| scopes.iter().any(|g| g == v))
    {
        return Err(TokenError::Oauth(
            "invalid_scope",
            "A refresh cannot add scopes.",
        ));
    }
    // A retry keeps the first use's time: the grace window does not move.
    sqlx::query(
        "UPDATE identity.mcp_tokens SET used_at=COALESCE(used_at, now()) WHERE token_hash=$1",
    )
    .bind(auth::hash_token(token))
    .execute(&mut *tx)
    .await?;
    let v = issue_tokens(&mut tx, grant, &scopes).await?;
    tx.commit().await?;
    Ok(v)
}

/// RFC 7009. Revoking either token ends the whole grant. Always 200 for a
/// well-formed request, whether or not the token was known.
pub async fn revoke(State(s): State<AppState>, h: HeaderMap, body: Bytes) -> Response {
    if enabled(&s).is_err() {
        return Error::NotFound.into_response();
    }
    let Some(form) = std::str::from_utf8(&body).ok().and_then(single_params) else {
        return oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Malformed form body.",
        );
    };
    let source = auth::client_ip_key(&h).unwrap_or_else(|| "unknown".into());
    if let Err(e) = auth::rate(&s.pool, &format!("mcp-revoke:src:{source}"), 600).await {
        return token_api_error(e);
    }
    let c = match authenticate_client(&s, &h, &form).await {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let token = form.get("token").map(String::as_str).unwrap_or("");
    if token.len() == 64 {
        let r: Result<()> = async {
            let mut tx = s.pool.begin().await?;
            let row: Option<(Uuid, Uuid, Uuid)> = sqlx::query_as(
                "SELECT g.id, g.user_id, g.org_id FROM identity.mcp_tokens t JOIN identity.mcp_grants g ON g.id=t.grant_id WHERE t.token_hash=$1 AND g.client_pk=$2",
            )
            .bind(auth::hash_token(token))
            .bind(c.pk)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some((grant, user, org)) = row {
                revoke_grant(&mut tx, grant, "CLIENT_REVOKED").await?;
                operations::audit(&mut tx, Some(user), Some(org), Some(grant), "mcp.grant_revoked", "CLIENT_REVOKED", auth::request_id(&h)).await?;
            }
            tx.commit().await?;
            Ok(())
        }
        .await;
        if let Err(e) = r {
            return e.into_response();
        }
    }
    Json(json!({})).into_response()
}

// ---------------------------------------------------------------------------
// Bearer authentication for the MCP endpoint
// ---------------------------------------------------------------------------

/// Who an MCP request acts for. `actor` is what the existing services take;
/// `org` is the only workspace the token reaches.
pub struct Principal {
    pub actor: Actor,
    pub org: Uuid,
    pub scopes: Vec<String>,
    pub grant: Uuid,
    pub client_name: String,
}
impl Principal {
    pub fn has(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

pub enum BearerError {
    /// No token, unknown, expired or revoked: 401 + challenge.
    Invalid,
    Api(Error),
}

pub async fn principal(s: &AppState, h: &HeaderMap) -> std::result::Result<Principal, BearerError> {
    let token = h
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .map(str::trim)
        .ok_or(BearerError::Invalid)?;
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(BearerError::Invalid);
    }
    let hash = auth::hash_token(token);
    let row = sqlx::query(
        "SELECT g.id, g.user_id, g.org_id, g.resource, u.party_id, c.client_name,
                CASE WHEN t.scopes IS NULL THEN g.scopes
                     ELSE ARRAY(SELECT x FROM unnest(t.scopes) x WHERE x = ANY(g.scopes)) END AS scopes,
                (g.last_used_at IS NULL OR g.last_used_at < now() - interval '1 minute') AS touch
           FROM identity.mcp_tokens t
           JOIN identity.mcp_grants g ON g.id=t.grant_id
           JOIN identity.users u ON u.id=g.user_id
           JOIN identity.mcp_clients c ON c.id=g.client_pk
          WHERE t.token_hash=$1 AND t.kind='ACCESS' AND (t.expires_at IS NULL OR t.expires_at > now())
            AND g.revoked_at IS NULL AND u.status='ACTIVE' AND c.disabled_at IS NULL",
    )
    .bind(&hash)
    .fetch_optional(&s.pool)
    .await
    .map_err(|e| BearerError::Api(e.into()))?
    .ok_or(BearerError::Invalid)?;
    // Audience (RFC 8707): only tokens issued for this MCP resource.
    if row.get::<String, _>("resource") != super::resource(s) {
        return Err(BearerError::Invalid);
    }
    let grant: Uuid = row.get("id");
    if row.get::<bool, _>("touch") {
        let _ = sqlx::query("UPDATE identity.mcp_grants SET last_used_at=now() WHERE id=$1")
            .bind(grant)
            .execute(&s.pool)
            .await;
    }
    Ok(Principal {
        actor: Actor {
            user: row.get("user_id"),
            party: row.get("party_id"),
            // Never a browser session: nothing keyed on it (session
            // management, MFA) matches an MCP token.
            session_hash: hash,
            request: auth::request_id(h),
        },
        org: row.get("org_id"),
        scopes: row.get("scopes"),
        grant,
        client_name: row.get("client_name"),
    })
}

/// Ends every AI connection of a user (password change, log out
/// everywhere, password reset).
pub async fn revoke_user_grants(c: &mut PgConnection, user: Uuid, reason: &str) -> Result<()> {
    sqlx::query("DELETE FROM identity.mcp_tokens WHERE grant_id IN (SELECT id FROM identity.mcp_grants WHERE user_id=$1 AND revoked_at IS NULL)")
        .bind(user).execute(&mut *c).await?;
    sqlx::query("UPDATE identity.mcp_grants SET revoked_at=now(), revoke_reason=$2 WHERE user_id=$1 AND revoked_at IS NULL")
        .bind(user).bind(reason).execute(&mut *c).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The user's view of connected AI apps (Studio session)
// ---------------------------------------------------------------------------

pub async fn connections(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',g.id,'client_name',c.client_name,'client_id',c.client_id,'org_id',g.org_id,'org_name',o.name,
                'scopes',g.scopes,'created_at',g.created_at,'last_used_at',g.last_used_at,'method',g.method,'label',g.label,
                'expires_at',CASE WHEN g.method='TOKEN' THEN (SELECT max(t.expires_at) FROM identity.mcp_tokens t WHERE t.grant_id=g.id) END,
                'no_expiry',g.method='TOKEN' AND EXISTS (SELECT 1 FROM identity.mcp_tokens t WHERE t.grant_id=g.id AND t.expires_at IS NULL))
           FROM identity.mcp_grants g JOIN identity.mcp_clients c ON c.id=g.client_pk JOIN identity.orgs o ON o.id=g.org_id
          WHERE g.user_id=$1 AND g.revoked_at IS NULL
            AND (g.method<>'TOKEN' OR EXISTS (SELECT 1 FROM identity.mcp_tokens t WHERE t.grant_id=g.id AND (t.expires_at IS NULL OR t.expires_at > now())))
          ORDER BY g.created_at DESC LIMIT 100",
    )
    .bind(a.user)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!({"items": items})))
}

pub async fn revoke_connection(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    let org: Option<Uuid> = sqlx::query_scalar(
        "SELECT org_id FROM identity.mcp_grants WHERE id=$1 AND user_id=$2 AND revoked_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?;
    let org = org.ok_or(Error::NotFound)?;
    revoke_grant(&mut tx, id, "USER_REVOKED").await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "mcp.grant_revoked",
        "USER_REVOKED",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"revoked": true})))
}

// ---------------------------------------------------------------------------
// Server-rendered pages (no script; every value is HTML-escaped)
// ---------------------------------------------------------------------------

pub fn escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

const STYLE: &str = "body{margin:0;background:#f6f6f4;color:#151515;font:16px/1.55 -apple-system,BlinkMacSystemFont,'Apple SD Gothic Neo','Noto Sans KR',sans-serif}
main{max-width:480px;margin:0 auto;padding:32px 16px}h1{font-size:22px;margin:0 0 8px}.card{background:#fff;border:1px solid #e3e3df;border-radius:14px;padding:20px;margin:16px 0}
.muted{color:#5c5c58;font-size:14px}label{display:flex;gap:10px;align-items:flex-start;padding:8px 0}select{width:100%;padding:10px;font:inherit;border-radius:10px;border:1px solid #ccc}
.row{display:flex;gap:10px;margin-top:20px}button,a.btn{flex:1;padding:12px;border-radius:10px;border:1px solid #151515;font:inherit;font-weight:600;text-align:center;text-decoration:none;cursor:pointer}
.primary{background:#151515;color:#fff}.secondary{background:#fff;color:#151515}code{word-break:break-all;font-size:13px}.warn{background:#fff7e6;border-color:#f0d9a8}
@media (prefers-color-scheme:dark){body{background:#111;color:#eee}.card{background:#1b1b1b;border-color:#333}.muted{color:#aaa}.primary{background:#eee;color:#111}.secondary{background:#1b1b1b;color:#eee;border-color:#eee}.warn{background:#2a2316;border-color:#5a4a26}select{background:#1b1b1b;color:#eee}}";

pub(super) fn page(title: &str, body: &str, form_action_extra: Option<&str>) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"ko\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta name=\"robots\" content=\"noindex\"><title>{}</title><style>{STYLE}</style></head><body><main>{body}</main></body></html>",
        escape(title)
    );
    let mut r = (StatusCode::OK, html).into_response();
    let csp = format!(
        "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'{}; frame-ancestors 'none'; base-uri 'none'",
        form_action_extra
            .map(|o| format!(" {o}"))
            .unwrap_or_default()
    );
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    if let Ok(v) = HeaderValue::from_str(&csp) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    // Not `no-referrer`: with it browsers send `Origin: null` on the form's
    // POST, and the boundary refuses it (403 FORBIDDEN after "승인").
    // `same-origin` still sends nothing to the client's redirect host.
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    r
}

pub(super) fn page_error(message: &str) -> Response {
    let mut r = page(
        "어데닉 연결",
        &format!(
            "<h1>AI 앱을 연결할 수 없어요</h1><div class=\"card\">{}</div><p class=\"muted\">이 창을 닫고 AI 앱에서 연결을 다시 시작해 주세요.</p>",
            escape(message)
        ),
        None,
    );
    *r.status_mut() = StatusCode::BAD_REQUEST;
    r
}

/// Sign-in first. The login link opens in the same window and brings the
/// user back here afterwards (`next`, read by the Studio's login screen):
/// the in-app browsers of the AI apps cannot open a second window.
/// `back` is this page's path with its parameters, e.g. `/api/oauth/authorize`.
pub(super) fn page_sign_in(
    s: &AppState,
    back: &str,
    params: &BTreeMap<String, String>,
) -> Response {
    let mut next = back.to_string();
    if !params.is_empty() {
        next.push('?');
        next.push_str(
            &url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(params)
                .finish(),
        );
    }
    let login = format!(
        "{}/login?next={}",
        super::issuer(s),
        url::form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>()
    );
    page(
        "어데닉 로그인 필요",
        &format!(
            "<h1>먼저 어데닉에 로그인해 주세요</h1><div class=\"card\"><p>AI 앱 연결은 어데닉 계정으로 승인해요. 로그인하면 이 화면으로 돌아와요. 비밀번호는 AI 앱에 전달되지 않아요.</p>\
             <div class=\"row\"><a class=\"btn secondary\" href=\"\">로그인했어요 · 계속</a><a class=\"btn primary\" href=\"{}\">로그인하기</a></div></div>",
            escape(&login)
        ),
        None,
    )
}

pub(super) fn scope_label(scope: &str, payout_limit: i64) -> (&'static str, String) {
    let (title, desc): (&'static str, &str) = match scope {
        SCOPE_READ => ("발매 정보 보기", "발매·트랙·검사 결과·접수 상태를 읽어요."),
        SCOPE_WRITE => (
            "발매 초안 작성·수정",
            "임시 저장 상태의 발매 정보·트랙·크레딧을 만들고 고쳐요. 접수된 발매는 바꾸지 못해요.",
        ),
        SCOPE_SUBMIT => (
            "발매 신청 접수",
            "내가 대화에서 최종 확인한 내용 그대로 배급 신청을 접수해요. 대화에서 확인한 권리·성인 확인 선언은 내 선언으로 기록돼요.",
        ),
        SCOPE_SETTLEMENT => (
            "정산 보기",
            "정산 잔액·내역·플랫폼별 수익 보고·지급 신청 현황을 읽어요. 계좌번호는 보이지 않아요.",
        ),
        _ => {
            return (
                "소액 지급 신청",
                format!(
                    "등록해 둔 계좌로 {}원 이하(24시간 합계)의 지급 신청과 취소를 해요. 대화에서 금액을 확인한 뒤에만 신청되고, 그보다 큰 금액과 계좌 변경은 어데닉 스튜디오에서만 할 수 있어요.",
                    won(payout_limit)
                ),
            );
        }
    };
    (title, desc.to_string())
}

/// 300000 → "300,000"
pub(super) fn won(v: i64) -> String {
    let digits = v.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Where a consent form posts and what it tells the artist about the
/// destination.
pub(super) struct ConsentTarget {
    pub action: &'static str,
    /// HTML (escaped by the caller): where the approval goes.
    pub note: String,
    /// An extra origin the form may navigate to (the client's redirect).
    pub form_origin: Option<String>,
}

pub(super) fn page_consent(
    client: &Client,
    target: &ConsentTarget,
    scopes: &[&str],
    orgs: &[(Uuid, String, String)],
    request: &str,
    payout_limit: i64,
) -> Response {
    let mut org_options = String::new();
    for (id, name, role) in orgs {
        let role = match role.as_str() {
            "OWNER" => "소유자",
            "EDITOR" => "편집자",
            _ => "보기 전용",
        };
        let name = if name == "Personal workspace" {
            "내 작업 공간"
        } else {
            name.as_str()
        };
        org_options.push_str(&format!(
            "<option value=\"{id}\">{} · {role}</option>",
            escape(name)
        ));
    }
    let mut scope_rows = String::new();
    for scope in scopes {
        let (title, desc) = scope_label(scope, payout_limit);
        let control = match *scope {
            SCOPE_READ => "<input type=\"checkbox\" checked disabled>",
            SCOPE_WRITE => "<input type=\"checkbox\" name=\"scope_write\" checked>",
            // Submitting, settlement and payouts are never pre-selected.
            SCOPE_SUBMIT => "<input type=\"checkbox\" name=\"scope_submit\">",
            SCOPE_SETTLEMENT => "<input type=\"checkbox\" name=\"scope_settlement\">",
            _ => "<input type=\"checkbox\" name=\"scope_payout\">",
        };
        let desc = escape(&desc);
        scope_rows.push_str(&format!("<label>{control}<span><b>{title}</b><br><span class=\"muted\">{desc}</span></span></label>"));
    }
    let cimd = client.client_id.starts_with("https://");
    let body = format!(
        "<h1>{name}이(가) 어데닉 계정에 연결하려고 해요</h1>\
         <p class=\"muted\">승인하면 이 AI 앱이 아래 작업 공간에서 선택한 일만 할 수 있어요. 지급 계좌 변경, 계약 서명, 계정 설정과 고액 지급은 어떤 경우에도 할 수 없어요.</p>\
         <form method=\"post\" action=\"{action}\">\
         <div class=\"card\"><b>작업 공간</b><select name=\"org\" required>{org_options}</select></div>\
         <div class=\"card\">{scope_rows}</div>\
         <div class=\"card warn muted\">{note}{cimd_note}<br>연결은 언제든 끊을 수 있고, 비밀번호를 바꾸거나 모든 기기에서 로그아웃하면 함께 끊겨요.</div>\
         <input type=\"hidden\" name=\"request\" value=\"{request}\">\
         <div class=\"row\"><button class=\"secondary\" name=\"decision\" value=\"deny\">거부</button><button class=\"primary\" name=\"decision\" value=\"approve\">승인</button></div>\
         </form>",
        name = escape(&client.name),
        action = target.action,
        note = target.note,
        cimd_note = if cimd {
            format!(
                "<br>앱 확인 주소: <code>{}</code>",
                escape(&client.client_id)
            )
        } else {
            String::new()
        },
        request = escape(request),
    );
    page("어데닉에 AI 앱 연결", &body, target.form_origin.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consent_page_keeps_the_form_origin() {
        let r = page("t", "", None);
        assert_eq!(r.headers()[header::REFERRER_POLICY], "same-origin");
    }
    #[test]
    fn redirect_uris_are_https_or_loopback() {
        for ok in [
            "https://chatgpt.com/connector_platform_oauth_redirect",
            "https://claude.ai/api/mcp/auth_callback",
            "http://127.0.0.1:33418/callback",
            "http://localhost:6274/oauth/callback",
            "http://[::1]:9000/cb",
        ] {
            assert!(valid_redirect_uri(ok), "{ok}");
        }
        for bad in [
            "http://example.com/cb",
            "https://example.com/cb#frag",
            "https://user:pw@example.com/cb",
            "javascript:alert(1)",
            "cursor://callback",
            "http://127.0.0.1.evil.test/cb",
            "",
        ] {
            assert!(!valid_redirect_uri(bad), "{bad}");
        }
    }
    #[test]
    fn loopback_redirects_may_change_port_others_must_match() {
        let reg = vec![
            "http://127.0.0.1:3000/callback".to_string(),
            "https://claude.ai/api/mcp/auth_callback".to_string(),
        ];
        assert!(redirect_matches(&reg, "http://127.0.0.1:51234/callback"));
        assert!(!redirect_matches(&reg, "http://127.0.0.1:51234/other"));
        assert!(!redirect_matches(&reg, "http://localhost:3000/callback"));
        assert!(redirect_matches(
            &reg,
            "https://claude.ai/api/mcp/auth_callback"
        ));
        assert!(!redirect_matches(
            &reg,
            "https://claude.ai/api/mcp/auth_callback/x"
        ));
        assert!(!redirect_matches(
            &reg,
            "https://claude.ai:444/api/mcp/auth_callback"
        ));
    }
    #[test]
    fn scopes_default_to_read_write_and_reject_unknown() {
        assert_eq!(
            requested_scopes(None).unwrap(),
            vec![SCOPE_READ, SCOPE_WRITE]
        );
        assert_eq!(
            requested_scopes(Some("openid releases:submit")).unwrap(),
            vec![SCOPE_READ, SCOPE_SUBMIT]
        );
        assert!(requested_scopes(Some("payouts:write")).is_none());
    }
    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636 Appendix B.
        assert_eq!(
            pkce_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
    #[test]
    fn cimd_document_must_name_its_own_url_and_allow_public_clients() {
        let url = "https://chatgpt.com/oauth/client.json";
        let doc = json!({"client_id": url, "client_name": "ChatGPT", "redirect_uris": ["https://chatgpt.com/connector_platform_oauth_redirect"],
                         "token_endpoint_auth_methods_supported": ["private_key_jwt", "none"]});
        assert_eq!(parse_cimd(url, &doc).unwrap().0, "ChatGPT");
        let mut other = doc.clone();
        other["client_id"] = json!("https://evil.test/client.json");
        assert!(parse_cimd(url, &other).is_err());
        let mut jwt_only = doc.clone();
        jwt_only["token_endpoint_auth_methods_supported"] = json!(["private_key_jwt"]);
        assert!(parse_cimd(url, &jwt_only).is_err());
        let mut bad_redirect = doc;
        bad_redirect["redirect_uris"] = json!(["http://evil.test/cb"]);
        assert!(parse_cimd(url, &bad_redirect).is_err());
    }
    #[test]
    fn cimd_hosts_are_allowlisted() {
        let s = Settings {
            enabled: true,
            cimd_hosts: vec!["chatgpt.com".into()],
            dynamic_registration: true,
            payout_limit_krw: 0,
            public_daily_user: 150,
        };
        assert!(cimd_allowed(&s, "https://chatgpt.com/oauth/client.json"));
        assert!(!cimd_allowed(
            &s,
            "https://chatgpt.com.evil.test/oauth/client.json"
        ));
        assert!(!cimd_allowed(
            &s,
            "https://chatgpt.com:8443/oauth/client.json"
        ));
        assert!(!cimd_allowed(&s, "https://chatgpt.com/"));
        assert!(!cimd_allowed(&s, "https://169.254.169.254/latest"));
        let pinned = Settings {
            cimd_hosts: vec!["https://claude.ai/oauth/mcp-oauth-client-metadata".into()],
            ..s
        };
        assert!(cimd_allowed(
            &pinned,
            "https://claude.ai/oauth/mcp-oauth-client-metadata"
        ));
        assert!(!cimd_allowed(
            &pinned,
            "https://claude.ai/other/client.json"
        ));
        assert!(!cimd_allowed(
            &pinned,
            "https://chatgpt.com/oauth/client.json"
        ));
        // `*`: any public HTTPS host, but never an address literal or a bare name.
        let any = Settings {
            cimd_hosts: vec!["*".into()],
            ..pinned
        };
        assert!(cimd_allowed(&any, "https://manus.im/oauth/client.json"));
        assert!(cimd_allowed(&any, "https://x.ai/mcp/client-metadata.json"));
        for bad in [
            "https://169.254.169.254/latest",
            "https://[::1]/client.json",
            "https://localhost/client.json",
            "http://manus.im/client.json",
            "https://manus.im:8443/client.json",
            "https://manus.im/client.json?x=1",
        ] {
            assert!(!cimd_allowed(&any, bad), "{bad}");
        }
    }
    #[test]
    fn escape_neutralizes_markup() {
        assert_eq!(
            escape("<script>\"x\"&'"),
            "&lt;script&gt;&quot;x&quot;&amp;&#39;"
        );
        assert_eq!(escape("a\u{0}\nb"), "ab");
    }
}
