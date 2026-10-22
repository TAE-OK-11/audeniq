//! Developer apps: OAuth clients an AUDENIQ user registers in the Studio to
//! use the REST API ([`super::rest`]) for themselves and, after review, for
//! other artists.
//!
//! - A new app is `PRIVATE`: only its owner can connect it (the consent
//!   screen refuses everyone else). Asking for review makes it `REVIEW`
//!   (still owner-only); an operator approves it (`PUBLIC`, anyone can
//!   connect) or sends it back with a note (`audeniq-admin developer-app`).
//! - Server apps get a client secret (shown once, stored as SHA-256, sent
//!   with HTTP Basic or in the form); public apps (mobile, desktop) use PKCE
//!   only. Both use authorization code + PKCE and rotating refresh tokens.
//! - Deleting an app disables it and ends every connection to it.
//!
//! `GET/POST /api/auth/developer-apps`, `PATCH/DELETE …/{id}`,
//! `POST …/{id}/secret`, `POST …/{id}/review` (Studio session; writes need
//! CSRF). Off unless the API is on.
use super::oauth::{self, valid_redirect_uri};
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
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// Apps one user can have at once (deleted ones do not count).
pub const MAX_APPS_PER_USER: i64 = 10;

fn enabled(s: &AppState) -> Result<()> {
    if s.mcp.enabled && s.mcp.api_enabled {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewApp {
    pub name: String,
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    /// A mobile or desktop app that cannot keep a secret: PKCE only.
    #[serde(default)]
    pub public_client: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppChange {
    #[serde(default)]
    pub redirect_uris: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
}

fn clean_name(v: &str) -> Option<String> {
    let n: String = v.chars().filter(|c| !c.is_control()).collect();
    let n = n.trim().to_string();
    (1..=60).contains(&n.chars().count()).then_some(n)
}

fn clean_redirects(v: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for r in v {
        let r = r.trim();
        if !valid_redirect_uri(r) {
            return Err(Error::InvalidCode("APP_REDIRECT_URI_INVALID"));
        }
        if !out.iter().any(|o| o == r) {
            out.push(r.to_string());
        }
    }
    if out.is_empty() || out.len() > 10 {
        return Err(Error::InvalidCode("APP_REDIRECT_URI_INVALID"));
    }
    Ok(out)
}

fn clean_description(v: Option<&str>) -> Result<Option<String>> {
    let Some(v) = v.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    if v.chars().count() > 500 {
        return Err(Error::InvalidCode("APP_DESCRIPTION_TOO_LONG"));
    }
    Ok(Some(
        v.chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .collect(),
    ))
}

fn clean_website(v: Option<&str>) -> Result<Option<String>> {
    let Some(v) = v.map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let ok = v.len() <= 512
        && url::Url::parse(v).is_ok_and(|u| u.scheme() == "https" && u.host().is_some());
    if ok {
        Ok(Some(v.to_string()))
    } else {
        Err(Error::InvalidCode("APP_WEBSITE_INVALID"))
    }
}

const APP_JSON: &str = "jsonb_build_object(
    'id', c.id, 'client_id', c.client_id, 'name', c.client_name, 'description', c.description,
    'website', c.client_uri, 'redirect_uris', c.redirect_uris,
    'public_client', c.token_endpoint_auth_method = 'none',
    'visibility', c.visibility, 'review_note', c.review_note,
    'review_requested_at', c.review_requested_at, 'reviewed_at', c.reviewed_at, 'created_at', c.created_at,
    'connections', (SELECT count(DISTINCT g.user_id) FROM identity.mcp_grants g WHERE g.client_pk=c.id AND g.revoked_at IS NULL))";

async fn app_json(c: &mut PgConnection, id: Uuid, owner: Uuid) -> Result<Value> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {APP_JSON} FROM identity.mcp_clients c
          WHERE c.id=$1 AND c.owner_user_id=$2 AND c.kind='DEV' AND c.disabled_at IS NULL"
    )))
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

/// `GET /api/auth/developer-apps`: the user's apps.
pub async fn list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    enabled(&s)?;
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {APP_JSON} FROM identity.mcp_clients c
          WHERE c.owner_user_id=$1 AND c.kind='DEV' AND c.disabled_at IS NULL
          ORDER BY c.created_at DESC LIMIT {MAX_APPS_PER_USER}"
    )))
    .bind(a.user)
    .fetch_all(&s.pool)
    .await?;
    Ok(Json(json!({
        "items": items,
        "limit": MAX_APPS_PER_USER,
        "api_base": format!("{}{}", super::issuer(&s), super::rest::API_PATH),
        "resource": super::resource_for(&s, super::Audience::Api),
        "authorization_endpoint": format!("{}/api/oauth/authorize", super::issuer(&s)),
        "token_endpoint": format!("{}/api/oauth/token", super::issuer(&s)),
        "openapi": format!("{}{}/openapi.json", super::issuer(&s), super::rest::API_PATH),
    })))
}

/// `POST /api/auth/developer-apps`: a new private app. The secret is in
/// this answer only.
pub async fn create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<NewApp>,
) -> Result<Json<Value>> {
    enabled(&s)?;
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("developer-app-create:{}", a.user), 20).await?;
    let name = clean_name(&i.name).ok_or(Error::InvalidCode("APP_NAME_INVALID"))?;
    let redirects = clean_redirects(&i.redirect_uris)?;
    let description = clean_description(i.description.as_deref())?;
    let website = clean_website(i.website.as_deref())?;
    let mut tx = s.pool.begin().await?;
    // One at a time per user, so the limit holds under parallel requests.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('developer-apps:' || $1::text, 0))")
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.mcp_clients WHERE owner_user_id=$1 AND kind='DEV' AND disabled_at IS NULL",
    )
    .bind(a.user)
    .fetch_one(&mut *tx)
    .await?;
    if count >= MAX_APPS_PER_USER {
        return Err(Error::InvalidCode("APP_LIMIT_REACHED"));
    }
    let id = Uuid::new_v4();
    let client_id = format!("audeniq-app-{}", &auth::random_token()[..32]);
    let secret = (!i.public_client).then(auth::random_token);
    let method = if i.public_client {
        "none"
    } else {
        "client_secret_basic"
    };
    sqlx::query(
        "INSERT INTO identity.mcp_clients(id,client_id,kind,client_name,client_uri,redirect_uris,token_endpoint_auth_method,secret_hash,owner_user_id,visibility,description)
         VALUES($1,$2,'DEV',$3,$4,$5,$6,$7,$8,'PRIVATE',$9)",
    )
    .bind(id)
    .bind(&client_id)
    .bind(&name)
    .bind(&website)
    .bind(&redirects)
    .bind(method)
    .bind(secret.as_deref().map(auth::hash_token))
    .bind(a.user)
    .bind(&description)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(id),
        "api.app_created",
        method,
        a.request,
    )
    .await?;
    let mut app = app_json(&mut tx, id, a.user).await?;
    tx.commit().await?;
    if let Some(secret) = secret {
        app["client_secret"] = json!(secret);
    }
    Ok(Json(app))
}

/// `PATCH /api/auth/developer-apps/{id}`: redirect URIs, description,
/// website. The name stays (people approved the app under it).
pub async fn update(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
    Json(i): Json<AppChange>,
) -> Result<Json<Value>> {
    enabled(&s)?;
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    owned(&mut tx, id, a.user).await?;
    if let Some(r) = &i.redirect_uris {
        let redirects = clean_redirects(r)?;
        sqlx::query("UPDATE identity.mcp_clients SET redirect_uris=$2 WHERE id=$1")
            .bind(id)
            .bind(&redirects)
            .execute(&mut *tx)
            .await?;
    }
    if i.description.is_some() {
        sqlx::query("UPDATE identity.mcp_clients SET description=$2 WHERE id=$1")
            .bind(id)
            .bind(clean_description(i.description.as_deref())?)
            .execute(&mut *tx)
            .await?;
    }
    if i.website.is_some() {
        sqlx::query("UPDATE identity.mcp_clients SET client_uri=$2 WHERE id=$1")
            .bind(id)
            .bind(clean_website(i.website.as_deref())?)
            .execute(&mut *tx)
            .await?;
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(id),
        "api.app_updated",
        "OWNER",
        a.request,
    )
    .await?;
    let app = app_json(&mut tx, id, a.user).await?;
    tx.commit().await?;
    Ok(Json(app))
}

/// The user's live app, locked for this transaction.
async fn owned(c: &mut PgConnection, id: Uuid, owner: Uuid) -> Result<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT visibility, token_endpoint_auth_method FROM identity.mcp_clients
          WHERE id=$1 AND owner_user_id=$2 AND kind='DEV' AND disabled_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

/// `POST /api/auth/developer-apps/{id}/secret`: a new secret; the old one
/// stops working at once.
pub async fn rotate_secret(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    enabled(&s)?;
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("developer-app-secret:{}", a.user), 20).await?;
    let mut tx = s.pool.begin().await?;
    let row = owned(&mut tx, id, a.user).await?;
    if row.get::<String, _>("token_endpoint_auth_method") == "none" {
        return Err(Error::InvalidCode("APP_HAS_NO_SECRET"));
    }
    let secret = auth::random_token();
    sqlx::query("UPDATE identity.mcp_clients SET secret_hash=$2 WHERE id=$1")
        .bind(id)
        .bind(auth::hash_token(&secret))
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(id),
        "api.app_secret_rotated",
        "OWNER",
        a.request,
    )
    .await?;
    let mut app = app_json(&mut tx, id, a.user).await?;
    tx.commit().await?;
    app["client_secret"] = json!(secret);
    Ok(Json(app))
}

/// `POST /api/auth/developer-apps/{id}/review`: ask to open the app to
/// other artists.
pub async fn request_review(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    enabled(&s)?;
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    let row = owned(&mut tx, id, a.user).await?;
    if row.get::<String, _>("visibility") != "PRIVATE" {
        return Err(Error::InvalidCode("APP_REVIEW_NOT_AVAILABLE"));
    }
    sqlx::query(
        "UPDATE identity.mcp_clients SET visibility='REVIEW', review_requested_at=now(), review_note=NULL WHERE id=$1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(id),
        "api.app_review_requested",
        "OWNER",
        a.request,
    )
    .await?;
    let app = app_json(&mut tx, id, a.user).await?;
    tx.commit().await?;
    Ok(Json(app))
}

/// `DELETE /api/auth/developer-apps/{id}`: the app stops working and every
/// connection to it ends.
pub async fn delete(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    enabled(&s)?;
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    owned(&mut tx, id, a.user).await?;
    sqlx::query("UPDATE identity.mcp_clients SET disabled_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let grants: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM identity.mcp_grants WHERE client_pk=$1 AND revoked_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    for g in grants {
        oauth::revoke_grant(&mut tx, g, "APP_DELETED").await?;
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(id),
        "api.app_deleted",
        "OWNER",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"deleted": true})))
}

/// An operator's answer to a review (`audeniq-admin developer-app`):
/// approve opens the app to everyone, reject sends it back to the owner
/// with a note. Apps already public can be taken back to private.
pub async fn review(
    pool: &sqlx::PgPool,
    operator: &str,
    client_id: &str,
    approve: bool,
    note: Option<&str>,
) -> anyhow::Result<String> {
    let operator = operator.trim();
    anyhow::ensure!(!operator.is_empty(), "--operator NAME is required");
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT id, visibility FROM identity.mcp_clients WHERE client_id=$1 AND kind='DEV' AND disabled_at IS NULL FOR UPDATE",
    )
    .bind(client_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| anyhow::anyhow!("no live developer app with that client_id"))?;
    let id: Uuid = row.get("id");
    let to = if approve { "PUBLIC" } else { "PRIVATE" };
    let note = note
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| n.chars().take(500).collect::<String>());
    sqlx::query("UPDATE identity.mcp_clients SET visibility=$2, reviewed_at=now(), review_note=$3 WHERE id=$1")
        .bind(id)
        .bind(to)
        .bind(&note)
        .execute(&mut *tx)
        .await?;
    // Taking a public app back: other artists' connections end.
    if !approve {
        let others: Vec<Uuid> = sqlx::query_scalar(
            "SELECT g.id FROM identity.mcp_grants g JOIN identity.mcp_clients c ON c.id=g.client_pk
              WHERE g.client_pk=$1 AND g.revoked_at IS NULL AND g.user_id<>c.owner_user_id",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        for g in others {
            oauth::revoke_grant(&mut tx, g, "APP_UNPUBLISHED").await?;
        }
    }
    operations::audit(
        &mut tx,
        None,
        None,
        Some(id),
        "api.app_reviewed",
        &format!(
            "OPERATOR:{operator}:{}",
            if approve { "APPROVED" } else { "RETURNED" }
        ),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(to.to_string())
}

/// Developer apps for operators (`audeniq-admin developer-app list
/// [STATUS]`), the review queue first.
pub async fn admin_list(pool: &sqlx::PgPool, visibility: Option<&str>) -> anyhow::Result<Value> {
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('client_id',c.client_id,'name',c.client_name,'owner_user_id',c.owner_user_id,'visibility',c.visibility,
                'website',c.client_uri,'description',c.description,'redirect_uris',c.redirect_uris,
                'review_requested_at',c.review_requested_at,'reviewed_at',c.reviewed_at,'review_note',c.review_note,'created_at',c.created_at)
           FROM identity.mcp_clients c
          WHERE c.kind='DEV' AND c.disabled_at IS NULL AND ($1::text IS NULL OR c.visibility=$1)
          ORDER BY (c.visibility='REVIEW') DESC, c.review_requested_at, c.created_at LIMIT 500",
    )
    .bind(visibility)
    .fetch_all(pool)
    .await?;
    Ok(json!({"items": items}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_fields_are_checked() {
        assert_eq!(clean_name("  My App\u{7} ").as_deref(), Some("My App"));
        assert!(clean_name("   ").is_none());
        assert!(clean_name(&"x".repeat(61)).is_none());
        assert!(
            clean_redirects(&[
                "https://app.example/cb".into(),
                "https://app.example/cb".into()
            ])
            .is_ok_and(|v| v.len() == 1)
        );
        assert!(clean_redirects(&["http://app.example/cb".into()]).is_err());
        assert!(clean_redirects(&["http://127.0.0.1/cb".into()]).is_ok());
        assert!(clean_redirects(&[]).is_err());
        assert!(clean_website(Some("http://app.example")).is_err());
        assert_eq!(clean_website(Some(" ")).unwrap(), None);
        assert!(clean_description(Some(&"가".repeat(501))).is_err());
    }
}
