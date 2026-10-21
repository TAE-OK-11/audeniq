//! More permissions for a connection, asked by the assistant and approved by
//! the artist in the Studio (migration 0141).
//!
//! - `request_permission` (any connection): the assistant names the scopes it
//!   needs and why. It gets an approval link to show the artist; the artist
//!   gets a Studio notice with the same link.
//! - The artist opens the link while signed in to the Studio, sees which app
//!   asks for what, and approves or rejects. Approved scopes are added to the
//!   connection's grant and stay until the connection is revoked; tokens the
//!   connection already holds get them at once (scopes are read from the
//!   grant on every call).
//! - `get_permission_request` lets the assistant check the outcome.
//!
//! A request lapses after [`REQUEST_MINUTES`]. Payouts cannot be asked for
//! while the assistant payout limit is 0.
use super::{
    SCOPE_PAYOUT, SCOPES,
    oauth::Principal,
    tools::{ToolError, tool_error},
};
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    operations,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

/// The artist must answer a request within this many minutes.
pub const REQUEST_MINUTES: i64 = 60;
const REASON_MAX: usize = 500;

/// The Studio page where the artist answers a request.
pub fn approve_url(s: &AppState, id: Uuid) -> String {
    format!("{}/ai/approve/{id}", super::issuer(s))
}

fn request_view(s: &AppState, r: &sqlx::postgres::PgRow) -> Value {
    let id: Uuid = r.get("id");
    json!({
        "view": "permission_request",
        "request_id": id,
        "status": r.get::<String, _>("status"),
        "scopes": r.get::<Vec<String>, _>("scopes"),
        "approve_url": approve_url(s, id),
        "expires_at": r.get::<chrono::DateTime<chrono::Utc>, _>("expires_at"),
    })
}

/// `request_permission`: asks the artist for `scopes` on this connection.
pub(super) async fn request(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let asked: Vec<String> = args
        .get("scopes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    if asked.is_empty() || asked.iter().any(|a| !SCOPES.contains(&a.as_str())) {
        return Err(tool_error(
            "SCOPE_UNKNOWN",
            &format!("Ask for one or more of: {}.", SCOPES.join(", ")),
        ));
    }
    if s.mcp.payout_limit_krw == 0 && asked.iter().any(|a| a == SCOPE_PAYOUT) {
        return Err(tool_error(
            "SCOPE_UNAVAILABLE",
            "Payouts through an assistant are switched off for this service.",
        ));
    }
    let mut missing: Vec<String> = asked.into_iter().filter(|a| !p.has(a)).collect();
    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        return Ok(json!({
            "view": "permission_request",
            "status": "ALREADY_GRANTED",
            "scopes": p.scopes,
            "note": "This connection already has these permissions.",
        }));
    }
    let reason: String = args
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(REASON_MAX)
        .collect();
    // One open request per connection and set of scopes: asking again
    // returns it instead of piling up approvals.
    if let Some(r) = sqlx::query(
        "SELECT id,status,scopes,expires_at FROM identity.mcp_permission_requests
          WHERE grant_id=$1 AND status='PENDING' AND expires_at > now() AND scopes=$2",
    )
    .bind(p.grant)
    .bind(&missing)
    .fetch_optional(&s.pool)
    .await
    .map_err(Error::from)?
    {
        return Ok(with_instruction(request_view(s, &r)));
    }
    auth::rate(&s.pool, &format!("mcp-permission:{}", p.grant), 10).await?;
    let id = Uuid::new_v4();
    let r = sqlx::query(
        "INSERT INTO identity.mcp_permission_requests(id,grant_id,user_id,org_id,client_name,scopes,reason,expires_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,now()+make_interval(mins => $8))
         RETURNING id,status,scopes,expires_at",
    )
    .bind(id)
    .bind(p.grant)
    .bind(p.actor.user)
    .bind(p.org)
    .bind(&p.client_name)
    .bind(&missing)
    .bind(&reason)
    .bind(REQUEST_MINUTES as i32)
    .fetch_one(&s.pool)
    .await
    .map_err(Error::from)?;
    super::safety::notice(
        &s.pool,
        p.org,
        p.actor.user,
        "AI 앱이 권한을 더 요청했어요",
        &format!(
            "{}이(가) {} 권한을 요청했어요. {}분 안에 승인하거나 거절해 주세요. 내가 요청한 게 아니라면 거절하고 연결을 끊어 주세요.",
            p.client_name,
            scope_labels(&missing),
            REQUEST_MINUTES
        ),
        &format!("/ai/approve/{id}"),
    )
    .await;
    Ok(with_instruction(request_view(s, &r)))
}

fn with_instruction(mut v: Value) -> Value {
    v["instruction"] = json!(
        "Show approve_url to the artist and ask them to open it in AUDENIQ Studio and approve. Call get_permission_request (or retry the tool) after they say they approved. Nothing changes until they do."
    );
    v
}

/// `get_permission_request`: the outcome of this connection's request.
pub(super) async fn status(
    s: &AppState,
    p: &Principal,
    args: &Value,
) -> std::result::Result<Value, ToolError> {
    let id = args
        .get("request_id")
        .and_then(Value::as_str)
        .and_then(|v| v.parse::<Uuid>().ok())
        .ok_or_else(|| tool_error("INVALID_INPUT", "request_id must be a UUID."))?;
    expire(s).await?;
    let r = sqlx::query(
        "SELECT id,status,scopes,expires_at FROM identity.mcp_permission_requests WHERE id=$1 AND grant_id=$2",
    )
    .bind(id)
    .bind(p.grant)
    .fetch_optional(&s.pool)
    .await
    .map_err(Error::from)?
    .ok_or_else(|| tool_error("NOT_FOUND", "No such request on this connection."))?;
    let mut v = request_view(s, &r);
    v["connection_scopes"] = json!(p.scopes);
    Ok(v)
}

async fn expire(s: &AppState) -> Result<()> {
    sqlx::query("UPDATE identity.mcp_permission_requests SET status='EXPIRED', decided_at=now() WHERE status='PENDING' AND expires_at < now()")
        .execute(&s.pool)
        .await?;
    Ok(())
}

/// Korean names of scopes for notices.
fn scope_labels(scopes: &[String]) -> String {
    scopes
        .iter()
        .map(|s| match s.as_str() {
            "releases:read" => "발매 정보 보기",
            "releases:write" => "발매 초안 작성·수정",
            "releases:submit" => "발매 신청",
            "settlement:read" => "정산 보기",
            "payouts:request" => "지급 요청",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("·")
}

// ---------------------------------------------------------------------------
// Studio (session + CSRF)
// ---------------------------------------------------------------------------

const REQUEST_JSON: &str = "jsonb_build_object('id',q.id,'status',q.status,'scopes',q.scopes,'reason',q.reason,
    'client_name',q.client_name,'org_id',q.org_id,'org_name',o.name,'created_at',q.created_at,'expires_at',q.expires_at,
    'decided_at',q.decided_at,'connection_scopes',g.scopes,'connection_label',g.label,'connection_revoked',g.revoked_at IS NOT NULL)";

/// The user's open requests (Studio: Profile → Connected AI apps).
pub async fn list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    expire(&s).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REQUEST_JSON} FROM identity.mcp_permission_requests q
           JOIN identity.mcp_grants g ON g.id=q.grant_id JOIN identity.orgs o ON o.id=q.org_id
          WHERE q.user_id=$1 AND q.status='PENDING' AND g.revoked_at IS NULL ORDER BY q.created_at DESC LIMIT 50"
    )))
    .bind(a.user)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!({"items": items})))
}

/// One request (the approval page), only the signed-in user's own.
pub async fn get(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    expire(&s).await?;
    let v: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REQUEST_JSON} FROM identity.mcp_permission_requests q
           JOIN identity.mcp_grants g ON g.id=q.grant_id JOIN identity.orgs o ON o.id=q.org_id
          WHERE q.id=$1 AND q.user_id=$2"
    )))
    .bind(id)
    .bind(a.user)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(Json(v))
}

pub async fn approve(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    decide(&s, &h, id, true).await
}

pub async fn reject(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    decide(&s, &h, id, false).await
}

async fn decide(s: &AppState, h: &HeaderMap, id: Uuid, approve: bool) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    // The request and its connection, locked: a revoke, another tab or the
    // expiry cannot race the decision.
    let row = sqlx::query(
        "SELECT q.grant_id, q.org_id, q.scopes, q.status, q.expires_at < now() AS expired, g.revoked_at IS NOT NULL AS revoked
           FROM identity.mcp_permission_requests q JOIN identity.mcp_grants g ON g.id=q.grant_id
          WHERE q.id=$1 AND q.user_id=$2 FOR UPDATE OF q, g",
    )
    .bind(id)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if row.get::<String, _>("status") != "PENDING" {
        return Err(Error::InvalidCode("AI_REQUEST_DECIDED"));
    }
    if row.get::<bool, _>("expired") {
        sqlx::query("UPDATE identity.mcp_permission_requests SET status='EXPIRED', decided_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(Error::InvalidCode("AI_REQUEST_EXPIRED"));
    }
    let grant: Uuid = row.get("grant_id");
    let org: Uuid = row.get("org_id");
    if row.get::<bool, _>("revoked") {
        sqlx::query("UPDATE identity.mcp_permission_requests SET status='REVOKED', decided_at=now() WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Err(Error::InvalidCode("AI_CONNECTION_REVOKED"));
    }
    let scopes: Vec<String> = row.get("scopes");
    if approve && s.mcp.payout_limit_krw == 0 && scopes.iter().any(|v| v == SCOPE_PAYOUT) {
        return Err(Error::PolicyGate("AI_PAYOUT_SCOPE_OFF"));
    }
    let status = if approve { "APPROVED" } else { "REJECTED" };
    sqlx::query(
        "UPDATE identity.mcp_permission_requests SET status=$2, decided_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    let now: Vec<String> = if approve {
        sqlx::query_scalar(
            "UPDATE identity.mcp_grants SET scopes=ARRAY(SELECT DISTINCT unnest(scopes || $2::text[]) ORDER BY 1)
              WHERE id=$1 RETURNING scopes",
        )
        .bind(grant)
        .bind(&scopes)
        .fetch_one(&mut *tx)
        .await?
    } else {
        sqlx::query_scalar("SELECT scopes FROM identity.mcp_grants WHERE id=$1")
            .bind(grant)
            .fetch_one(&mut *tx)
            .await?
    };
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(grant),
        if approve {
            "mcp.scopes_added"
        } else {
            "mcp.scopes_rejected"
        },
        &scopes.join(" "),
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id": id, "status": status, "connection_scopes": now}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_labels_are_korean() {
        assert_eq!(
            scope_labels(&["releases:submit".into(), "settlement:read".into()]),
            "발매 신청·정산 보기"
        );
    }
}
