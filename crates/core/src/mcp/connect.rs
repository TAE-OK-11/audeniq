//! The Studio's consent page (`/ai/connect`) for connecting an AI app.
//!
//! `/api/oauth/authorize` checks the client and seals the request, then sends
//! the signed-in browser to `/ai/connect?request=…`; `/api/oauth/device`
//! sends it to `/ai/connect?user_code=…`. The page reads the request here,
//! shows who asks for what, and posts the artist's decision back:
//!
//! - `GET  /api/auth/ai-connect?request=` — what the page shows.
//! - `POST /api/auth/ai-connect/device` — a device code becomes a request.
//! - `POST /api/auth/ai-connect` — approve or deny. A browser connection
//!   answers with the client's redirect URL (the page navigates there; the
//!   Studio's CSP keeps forms on its own origin), a device connection with
//!   its status.
//!
//! Requests are bound to the session that opened them, as the server forms
//! (`oauth::decide`, `login::device_decide`) are; those stay for clients
//! that post them directly.
use super::{
    SCOPE_PAYOUT, SCOPE_READ, SCOPE_SUBMIT, SCOPE_WRITE, SCOPES,
    login::{self, DeviceOutcome},
    oauth,
};
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct RequestQuery {
    pub request: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeInput {
    pub user_code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInput {
    pub request: String,
    pub decision: String,
    #[serde(default)]
    pub org: Option<Uuid>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

enum Kind {
    Browser,
    Device,
}

/// The sealed request, its kind, after checking it belongs to this session
/// and has not expired.
fn open(s: &AppState, a: &Actor, sealed: &str) -> Result<(Kind, Value)> {
    let (kind, req) = match super::open(s, "consent", sealed) {
        Some(v) => (Kind::Browser, v),
        None => (
            Kind::Device,
            super::open(s, "device-consent", sealed)
                .ok_or(Error::InvalidCode("AI_CONNECT_EXPIRED"))?,
        ),
    };
    if req["exp"].as_i64().unwrap_or(0) < chrono::Utc::now().timestamp() {
        return Err(Error::InvalidCode("AI_CONNECT_EXPIRED"));
    }
    if req["sh"].as_str() != Some(crate::hex::encode(&a.session_hash).as_str()) {
        return Err(Error::InvalidCode("AI_CONNECT_OTHER_SESSION"));
    }
    Ok((kind, req))
}

fn offered(req: &Value) -> Vec<String> {
    serde_json::from_value(req["sc"].clone()).unwrap_or_default()
}

/// Read, plus the ticked scopes among those the app asked for.
fn chosen(req: &Value, picked: &[String]) -> Vec<String> {
    let offered = offered(req);
    let mut out = vec![SCOPE_READ.to_string()];
    for scope in SCOPES {
        if scope != SCOPE_READ
            && offered.iter().any(|o| o == scope)
            && picked.iter().any(|p| p == scope)
        {
            out.push(scope.to_string());
        }
    }
    out
}

fn host(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()?
        .host_str()
        .map(|h| h.trim_start_matches("www.").to_string())
}

/// `GET /api/auth/ai-connect?request=`.
pub async fn details(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<RequestQuery>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let (kind, req) = open(&s, &a, &q.request)?;
    let client_pk: Uuid = req["c"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or(Error::Invalid)?;
    let client = sqlx::query(
        "SELECT client_id, client_name, kind, client_uri, description, visibility, owner_user_id
           FROM identity.mcp_clients WHERE id=$1 AND disabled_at IS NULL",
    )
    .bind(client_pk)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::InvalidCode("AI_CONNECT_CLIENT_UNKNOWN"))?;
    let client_id: String = client.get("client_id");
    // A developer app stays with its owner until an operator approves it.
    if client.get::<String, _>("visibility") != "PUBLIC"
        && client.get::<Option<Uuid>, _>("owner_user_id") != Some(a.user)
    {
        return Err(Error::InvalidCode("AI_CONNECT_CLIENT_PRIVATE"));
    }
    let developer = client.get::<String, _>("kind") == "DEV";
    let offered = offered(&req);
    let scopes: Vec<Value> = SCOPES
        .iter()
        .filter(|scope| offered.iter().any(|o| o == *scope))
        .map(|scope| {
            let (title, description) = oauth::scope_label(scope, s.mcp.payout_limit_krw);
            json!({
                "scope": scope,
                "title": title,
                "description": description,
                // Read is always granted; writing drafts is suggested;
                // submitting, settlement and payouts start off.
                "locked": *scope == SCOPE_READ,
                "checked": *scope == SCOPE_READ || *scope == SCOPE_WRITE,
                "risk": match *scope {
                    SCOPE_SUBMIT => "submit",
                    SCOPE_PAYOUT => "money",
                    _ => "none",
                },
            })
        })
        .collect();
    let workspaces: Vec<Value> = oauth::user_orgs(&s, a.user)
        .await?
        .into_iter()
        .map(|(id, name, role)| json!({"id": id, "name": name, "role": role}))
        .collect();
    let (requested_from, return_to, device_code) = match kind {
        Kind::Browser => {
            let to = req["r"].as_str().and_then(host);
            (to.clone(), to, None)
        }
        Kind::Device => (None, None, req["u"].as_str().map(str::to_owned)),
    };
    Ok(Json(json!({
        "kind": match kind { Kind::Browser => "browser", Kind::Device => "device" },
        // "api": an app asks for the REST API; "mcp": an AI assistant.
        "audience": super::Audience::from_code(req["rs"].as_str()).code(),
        "client": {
            "name": client.get::<String, _>("client_name"),
            "client_id": client_id,
            // A metadata document at an https URL names the app's own site.
            "verified_host": client_id.starts_with("https://").then(|| host(&client_id)).flatten(),
            "registered": client.get::<String, _>("kind") == "ADMIN",
            // An app a Studio user registered: its own site (if given) and
            // whether it is still only for its owner.
            "developer": developer,
            "website": developer.then(|| client.get::<Option<String>, _>("client_uri")).flatten(),
            "description": developer.then(|| client.get::<Option<String>, _>("description")).flatten(),
            "private": client.get::<String, _>("visibility") != "PUBLIC",
        },
        "requested_from": requested_from,
        "return_to": return_to,
        "device_code": device_code,
        "scopes": scopes,
        "workspaces": workspaces,
        "expires_at": chrono::DateTime::from_timestamp(req["exp"].as_i64().unwrap_or(0), 0),
    })))
}

/// `POST /api/auth/ai-connect/device`: the code the AI app showed.
pub async fn device_lookup(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CodeInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("mcp-device-lookup:{}", a.user), 20).await?;
    let code = login::normalize_user_code(&i.user_code)
        .ok_or(Error::InvalidCode("AI_CONNECT_CODE_FORMAT"))?;
    let row = sqlx::query(
        "SELECT device_code_hash, client_pk, scopes FROM identity.mcp_device_codes
          WHERE user_code_hash=$1 AND expires_at > now() AND grant_id IS NULL AND denied_at IS NULL AND used_at IS NULL",
    )
    .bind(auth::hash_token(&code))
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::InvalidCode("AI_CONNECT_CODE_UNKNOWN"))?;
    let client = login::client_by_pk(&s, row.get("client_pk"))
        .await?
        .ok_or(Error::InvalidCode("AI_CONNECT_CLIENT_UNKNOWN"))?;
    let stored: Vec<String> = row.get("scopes");
    let scopes: Vec<&str> = SCOPES
        .iter()
        .copied()
        .filter(|k| stored.iter().any(|v| v == k))
        .collect();
    let request = super::seal(
        &s,
        "device-consent",
        &json!({
            "d": crate::hex::encode(row.get::<Vec<u8>, _>("device_code_hash")),
            "c": client.pk, "sc": scopes, "sh": crate::hex::encode(&a.session_hash),
            "u": login::display_code(&code),
            "exp": chrono::Utc::now().timestamp() + login::DEVICE_SECONDS,
        }),
    );
    Ok(Json(json!({"request": request})))
}

/// `POST /api/auth/ai-connect`: the artist's answer.
pub async fn decide(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let (kind, req) = open(&s, &a, &i.request)?;
    let approve = match i.decision.as_str() {
        "approve" => true,
        "deny" => false,
        _ => return Err(Error::Invalid),
    };
    if approve && i.org.is_none() {
        return Err(Error::InvalidCode("AI_CONNECT_WORKSPACE_REQUIRED"));
    }
    let scopes = chosen(&req, &i.scopes);
    let status = if approve { "APPROVED" } else { "DENIED" };
    Ok(Json(match kind {
        Kind::Browser => {
            let url = oauth::consent_outcome(&s, &a, &req, approve, i.org, &scopes).await?;
            json!({"kind": "browser", "status": status, "redirect": url, "scopes": scopes})
        }
        Kind::Device => match login::device_outcome(&s, &a, &req, approve, i.org, &scopes).await? {
            DeviceOutcome::Gone => return Err(Error::InvalidCode("AI_CONNECT_EXPIRED")),
            _ => json!({"kind": "device", "status": status, "scopes": scopes}),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_offered_scopes_are_granted_and_read_always() {
        let req = json!({"sc": ["releases:read", "releases:write", "settlement:read"]});
        let picked = vec![
            "releases:write".to_string(),
            "payouts:request".to_string(),
            "admin".to_string(),
        ];
        assert_eq!(
            chosen(&req, &picked),
            vec!["releases:read", "releases:write"]
        );
        assert_eq!(chosen(&req, &[]), vec!["releases:read"]);
    }
}
