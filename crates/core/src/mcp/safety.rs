//! Safety around what an AI assistant does for the artist (migration 0135).
//!
//! - Activity notices: a new AI connection, a submission or a payout request
//!   made through an assistant becomes a Studio notice to that user (and is
//!   mailed like any other notice), so a connection they did not make is
//!   noticed at once.
//! - Retry-safe creates: tools that create something accept a
//!   `client_request_id`; the same id on the same connection returns the
//!   first result instead of a duplicate.
//! - Studio approval (opt-in per user): submissions made through an
//!   assistant wait until the artist approves them in the Studio, so nothing
//!   the model says can submit on its own.
use super::tools;
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    operations, submission,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// A pending AI submission must be approved within this many days.
const PENDING_DAYS: i32 = 7;

// ---------------------------------------------------------------------------
// Activity notices
// ---------------------------------------------------------------------------

/// A Studio notice to one user (best effort: a failed notice never fails
/// the action it reports).
pub async fn notice(pool: &PgPool, org: Uuid, user: Uuid, title: &str, detail: &str, link: &str) {
    let r = sqlx::query("SELECT identity.mcp_notice($1,$2,'ACCOUNT',$3,$4,$5)")
        .bind(org)
        .bind(user)
        .bind(title)
        .bind(detail)
        .bind(link)
        .execute(pool)
        .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "mcp notice failed");
    }
}

pub async fn release_title(pool: &PgPool, org: Uuid, release: Uuid) -> String {
    sqlx::query_scalar::<_, String>("SELECT title FROM catalog.releases WHERE org_id=$1 AND id=$2")
        .bind(org)
        .bind(release)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "발매".into())
}

// ---------------------------------------------------------------------------
// Retry-safe creates
// ---------------------------------------------------------------------------

pub enum Reserved {
    /// First call with this id: go ahead, then [`finish`].
    New,
    /// Already done: the id of what the first call created.
    Done(Uuid),
    /// The first call is still running.
    Running,
}

pub async fn reserve(pool: &PgPool, grant: Uuid, tool: &str, key: &str) -> Result<Reserved> {
    // A first call that never finished (client gone, process restarted)
    // leaves its row without a result: after two minutes it is reclaimed.
    let inserted = sqlx::query(
        "INSERT INTO identity.mcp_idempotency(grant_id,tool,request_key) VALUES($1,$2,$3)
         ON CONFLICT (grant_id,tool,request_key) DO UPDATE SET created_at=now()
         WHERE identity.mcp_idempotency.result_id IS NULL AND identity.mcp_idempotency.created_at < now()-interval '2 minutes'",
    )
    .bind(grant)
    .bind(tool)
    .bind(key)
    .execute(pool)
    .await?
    .rows_affected();
    if inserted == 1 {
        return Ok(Reserved::New);
    }
    let done: Option<Option<Uuid>> = sqlx::query_scalar(
        "SELECT result_id FROM identity.mcp_idempotency WHERE grant_id=$1 AND tool=$2 AND request_key=$3",
    )
    .bind(grant)
    .bind(tool)
    .bind(key)
    .fetch_optional(pool)
    .await?;
    Ok(match done.flatten() {
        Some(id) => Reserved::Done(id),
        None => Reserved::Running,
    })
}

pub async fn finish(pool: &PgPool, grant: Uuid, tool: &str, key: &str, result: Option<Uuid>) {
    let q = match result {
        Some(id) => sqlx::query("UPDATE identity.mcp_idempotency SET result_id=$4 WHERE grant_id=$1 AND tool=$2 AND request_key=$3")
            .bind(grant).bind(tool).bind(key).bind(id),
        // Failed: free the id so a corrected retry can run.
        None => sqlx::query("DELETE FROM identity.mcp_idempotency WHERE grant_id=$1 AND tool=$2 AND request_key=$3 AND result_id IS NULL")
            .bind(grant).bind(tool).bind(key),
    };
    if let Err(e) = q.execute(pool).await {
        tracing::warn!(error = %e, "mcp idempotency update failed");
    }
}

// ---------------------------------------------------------------------------
// Studio approval of AI submissions
// ---------------------------------------------------------------------------

pub async fn studio_approval(pool: &PgPool, user: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar::<_, bool>(
        "SELECT studio_approval FROM identity.mcp_user_settings WHERE user_id=$1",
    )
    .bind(user)
    .fetch_optional(pool)
    .await?
    .unwrap_or(false))
}

pub struct Pending<'a> {
    pub org: Uuid,
    pub release: Uuid,
    pub user: Uuid,
    pub grant: Uuid,
    pub client_name: &'a str,
    pub declarations: Value,
    pub scope_hash: &'a str,
    pub idempotency_key: &'a str,
}

/// Records a submission for Studio approval (once per confirmation).
/// Returns the pending id and whether it was new.
pub async fn hold(pool: &PgPool, p: Pending<'_>) -> Result<(Uuid, bool)> {
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO identity.mcp_pending_submissions(id,org_id,release_id,user_id,grant_id,client_name,declarations,scope_hash,idempotency_key,expires_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,now()+make_interval(days=>$10))
         ON CONFLICT(idempotency_key) DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(p.org)
    .bind(p.release)
    .bind(p.user)
    .bind(p.grant)
    .bind(p.client_name)
    .bind(&p.declarations)
    .bind(p.scope_hash)
    .bind(p.idempotency_key)
    .bind(PENDING_DAYS)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = id {
        return Ok((id, true));
    }
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM identity.mcp_pending_submissions WHERE idempotency_key=$1",
    )
    .bind(p.idempotency_key)
    .fetch_one(pool)
    .await?;
    Ok((id, false))
}

/// The pending approval for a confirmation, if one was recorded.
pub async fn pending_for(pool: &PgPool, key: &str) -> Result<Option<(Uuid, String)>> {
    Ok(sqlx::query_as(
        "SELECT id, status FROM identity.mcp_pending_submissions WHERE idempotency_key=$1",
    )
    .bind(key)
    .fetch_optional(pool)
    .await?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsInput {
    pub studio_approval: bool,
}

pub async fn get_settings(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    // What the Studio may offer: connection tokens only while MCP is on, the
    // payout permission only while the assistant payout limit is above 0.
    Ok(Json(json!({
        "studio_approval": studio_approval(&s.pool, a.user).await?,
        "mcp_enabled": s.mcp.enabled,
        "payout_scope": s.mcp.payout_limit_krw > 0,
    })))
}

pub async fn put_settings(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<SettingsInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO identity.mcp_user_settings(user_id,studio_approval) VALUES($1,$2)
         ON CONFLICT(user_id) DO UPDATE SET studio_approval=EXCLUDED.studio_approval, updated_at=now()",
    )
    .bind(a.user)
    .bind(i.studio_approval)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "mcp.settings_changed",
        if i.studio_approval {
            "STUDIO_APPROVAL_ON"
        } else {
            "STUDIO_APPROVAL_OFF"
        },
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"studio_approval": i.studio_approval})))
}

/// The user's AI submissions waiting for approval.
pub async fn list_pending(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    sqlx::query("UPDATE identity.mcp_pending_submissions SET status='EXPIRED', decided_at=now() WHERE user_id=$1 AND status='PENDING' AND expires_at < now()")
        .bind(a.user).execute(&s.pool).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',p.id,'org_id',p.org_id,'org_name',o.name,'release_id',p.release_id,'title',r.title,
                'client_name',p.client_name,'declarations',p.declarations,'created_at',p.created_at,'expires_at',p.expires_at)
           FROM identity.mcp_pending_submissions p
           JOIN catalog.releases r ON r.org_id=p.org_id AND r.id=p.release_id
           JOIN identity.orgs o ON o.id=p.org_id
          WHERE p.user_id=$1 AND p.status='PENDING' AND p.decided_at IS NULL ORDER BY p.created_at DESC LIMIT 50",
    )
    .bind(a.user)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!({"items": items})))
}

/// Claims a pending submission for one decision: the first approve or
/// reject marks it (decided_at), so a second click or tab cannot also act.
async fn take(s: &AppState, user: Uuid, id: Uuid) -> Result<(Uuid, Uuid, Value, String, String)> {
    let claimed = sqlx::query(
        "UPDATE identity.mcp_pending_submissions SET decided_at=now()
          WHERE id=$1 AND user_id=$2 AND status='PENDING' AND decided_at IS NULL AND expires_at >= now()
          RETURNING org_id, release_id, declarations, scope_hash, idempotency_key",
    )
    .bind(id)
    .bind(user)
    .fetch_optional(&s.pool)
    .await?;
    if let Some(row) = claimed {
        return Ok((
            row.get("org_id"),
            row.get("release_id"),
            row.get("declarations"),
            row.get("scope_hash"),
            row.get("idempotency_key"),
        ));
    }
    let row = sqlx::query(
        "SELECT status, expires_at < now() AS expired FROM identity.mcp_pending_submissions WHERE id=$1 AND user_id=$2",
    )
    .bind(id)
    .bind(user)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    if row.get::<String, _>("status") == "PENDING" && row.get::<bool, _>("expired") {
        decide(&s.pool, id, "EXPIRED", None).await?;
        return Err(Error::PolicyGate("AI_SUBMISSION_EXPIRED"));
    }
    Err(Error::PolicyGate("AI_SUBMISSION_DECIDED"))
}

/// The final state of a submission.
async fn decide(pool: &PgPool, id: Uuid, status: &str, revision: Option<Uuid>) -> Result<()> {
    sqlx::query("UPDATE identity.mcp_pending_submissions SET status=$2, revision_id=$3, decided_at=now() WHERE id=$1 AND status='PENDING'")
        .bind(id).bind(status).bind(revision).execute(pool).await?;
    Ok(())
}

/// Gives a claimed submission back when approving failed for a reason the
/// artist can retry (e.g. the database was briefly unavailable).
async fn release_claim(pool: &PgPool, id: Uuid) {
    let _ = sqlx::query("UPDATE identity.mcp_pending_submissions SET decided_at=NULL WHERE id=$1 AND status='PENDING'")
        .bind(id)
        .execute(pool)
        .await;
}

/// The artist approves in the Studio: the release must still be exactly
/// what they confirmed in the chat; then it is submitted in their name.
pub async fn approve(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let (org, release, declarations, scope_hash, key) = take(&s, a.user, id).await?;
    let d = tools::declarations_from(&declarations);
    let review = match Box::pin(submission::draft_review(&s, &a, org, release, &d)).await {
        Ok(v) => v,
        Err(e) => {
            release_claim(&s.pool, id).await;
            return Err(e);
        }
    };
    if review["scope_hash"].as_str() != Some(scope_hash.as_str()) {
        decide(&s.pool, id, "STALE", None).await?;
        return Err(Error::PolicyGate("AI_SUBMISSION_STALE"));
    }
    let result = match Box::pin(tools::submit_now(&s, &a, org, release, d, key, &scope_hash)).await
    {
        Ok(v) => v,
        Err(Error::PolicyGate("RELEASE_CHANGED" | "CONSENT_SCOPE_MISMATCH")) => {
            decide(&s.pool, id, "STALE", None).await?;
            return Err(Error::PolicyGate("AI_SUBMISSION_STALE"));
        }
        Err(e) => {
            release_claim(&s.pool, id).await;
            return Err(e);
        }
    };
    let revision = result["revision_id"].as_str().and_then(|v| v.parse().ok());
    decide(&s.pool, id, "APPROVED", revision).await?;
    Ok(Json(json!({
        "id": id, "status": "APPROVED", "release_id": release,
        "application_id": result["revision_id"], "release_status": result["status"],
    })))
}

pub async fn reject(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let (org, release, ..) = take(&s, a.user, id).await?;
    decide(&s.pool, id, "REJECTED", None).await?;
    let mut tx = s.pool.begin().await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "mcp.submission_rejected",
        "USER_REJECTED",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"id": id, "status": "REJECTED"})))
}
