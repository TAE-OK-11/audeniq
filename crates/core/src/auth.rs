use crate::{
    api::AppState,
    config::Config,
    error::{Error, Result},
    operations,
};
use argon2::{
    Argon2, PasswordHasher, PasswordVerifier,
    password_hash::{PasswordHash, SaltString},
};
use axum::{Json, http::HeaderMap};
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Row};
use subtle::ConstantTimeEq;
use uuid::Uuid;
#[derive(Clone)]
pub struct Actor {
    pub user: Uuid,
    pub party: Uuid,
    pub session_hash: Vec<u8>,
    pub request: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}
pub fn hash_token(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
pub fn secret_eq(a: &str, b: &str) -> bool {
    hash_token(a).ct_eq(&hash_token(b)).into()
}
pub fn origin(headers: &HeaderMap, config: &Config) -> Result<()> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(config.origin.as_str()) {
        return Err(Error::Forbidden);
    }
    if headers
        .get("sec-fetch-site")
        .is_some_and(|v| v != "same-origin" && v != "none")
    {
        return Err(Error::Forbidden);
    }
    Ok(())
}
pub async fn actor(
    pool: &PgPool,
    headers: &HeaderMap,
    config: &Config,
    write: bool,
) -> Result<Actor> {
    let token = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.split(';').find_map(|v| {
                let (k, val) = v.trim().split_once('=')?;
                (k == config.cookie_name()).then_some(val)
            })
        })
        .ok_or(Error::Unauthorized)?;
    if token.len() != 64 {
        return Err(Error::Unauthorized);
    }
    let hash = hash_token(token);
    let row=sqlx::query("SELECT s.user_id,s.csrf_hash,u.party_id FROM identity.sessions s JOIN identity.users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.revoked_at IS NULL AND s.expires_at>now() AND u.status='ACTIVE'")
 .bind(&hash).fetch_optional(pool).await?.ok_or(Error::Unauthorized)?;
    if write {
        origin(headers, config)?;
        let csrf = headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .ok_or(Error::Forbidden)?;
        let expected: Vec<u8> = row.get("csrf_hash");
        if !bool::from(hash_token(csrf).ct_eq(&expected)) {
            return Err(Error::Forbidden);
        }
    }
    Ok(Actor {
        user: row.get("user_id"),
        party: row.get("party_id"),
        session_hash: hash,
        request: request_id(headers),
    })
}
pub async fn membership(c: &mut PgConnection, a: &Actor, org: Uuid, write: bool) -> Result<String> {
    // Shared row locks prevent a revoke from racing a protected write. Every request rechecks, no role cache.
    let role:Option<String>=sqlx::query_scalar("SELECT m.role FROM identity.memberships m JOIN identity.users u ON u.id=m.user_id WHERE m.org_id=$1 AND m.user_id=$2 AND m.status='ACTIVE' AND u.status='ACTIVE' FOR SHARE OF m,u")
 .bind(org).bind(a.user).fetch_optional(&mut *c).await?;
    let role = role.ok_or(Error::Forbidden)?;
    if write && role == "VIEWER" {
        return Err(Error::Forbidden);
    }
    Ok(role)
}
pub async fn authorize(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    kind: &str,
    write: bool,
) -> Result<()> {
    membership(c, a, org, write).await?;
    let exists:Option<Uuid>=sqlx::query_scalar("SELECT r.id FROM identity.resources r JOIN identity.resource_acl acl ON acl.org_id=r.org_id AND acl.resource_id=r.id WHERE r.org_id=$1 AND r.id=$2 AND r.kind=$3 AND acl.principal_party_id=$4 AND acl.action=$5 AND acl.revoked_at IS NULL AND acl.starts_at<=now() AND (acl.ends_at IS NULL OR acl.ends_at>now()) FOR SHARE OF acl")
 .bind(org).bind(id).bind(kind).bind(a.party).bind(if write{"write"}else{"read"}).fetch_optional(&mut *c).await?;
    exists.ok_or(Error::Forbidden)?;
    Ok(())
}
pub async fn create_resource(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    kind: &str,
) -> Result<()> {
    membership(c, a, org, true).await?;
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,$3)")
        .bind(org)
        .bind(id)
        .bind(kind)
        .execute(&mut *c)
        .await?;
    for action in ["read", "write"] {
        sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action) VALUES($1,$2,$3,$4)").bind(org).bind(id).bind(a.party).bind(action).execute(&mut *c).await?;
    }
    Ok(())
}
pub async fn rate(pool: &PgPool, key: &str, limit: i32) -> Result<()> {
    let n:i32=sqlx::query_scalar("INSERT INTO identity.auth_limits(bucket_hash,attempts) VALUES($1,1) ON CONFLICT(bucket_hash) DO UPDATE SET attempts=CASE WHEN identity.auth_limits.window_start<now()-interval '15 minutes' THEN 1 ELSE identity.auth_limits.attempts+1 END,window_start=CASE WHEN identity.auth_limits.window_start<now()-interval '15 minutes' THEN now() ELSE identity.auth_limits.window_start END RETURNING attempts")
 .bind(hash_token(key)).fetch_one(pool).await?;
    if n > limit {
        Err(Error::RateLimited)
    } else {
        Ok(())
    }
}
fn credentials(c: &Credentials) -> Result<String> {
    let email = c.email.trim().to_lowercase();
    if email.len() > 254
        || !email.contains('@')
        || email.chars().any(char::is_whitespace)
        || c.password.len() < 12
        || c.password.len() > 128
    {
        return Err(Error::Invalid);
    }
    Ok(email)
}
pub async fn password_hash(password: String) -> Result<String> {
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
            .map_err(|_| Error::Internal)
    })
    .await
    .map_err(|_| Error::Internal)?
}
async fn verify(password: String, hash: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}
pub async fn register(s: &AppState, h: &HeaderMap, input: Credentials) -> Result<Json<Value>> {
    origin(h, &s.config)?;
    let email = credentials(&input)?;
    rate(&s.pool, "register:global", 100).await?;
    rate(&s.pool, &format!("register:{email}"), 5).await?;
    let _permit = s
        .password_slots
        .acquire()
        .await
        .map_err(|_| Error::Internal)?;
    let hash = password_hash(input.password).await?;
    let mut tx = s.pool.begin().await?;
    let user = Uuid::new_v4();
    let org = Uuid::new_v4();
    let party = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identity.orgs(id,name,kind) VALUES($1,'Personal workspace','PERSONAL')",
    )
    .bind(org)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON','Account holder')").bind(party).bind(org).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO identity.users(id,email,password_hash,party_id) VALUES($1,$2,$3,$4)")
        .bind(user)
        .bind(email)
        .bind(hash)
        .bind(party)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role) VALUES($1,$2,'OWNER')")
        .bind(org)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(user),
        Some(org),
        Some(user),
        "auth.register",
        "SELF_REGISTER",
        request_id(h),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"user_id":user,"org_id":org,"party_id":party,"submission_enabled":false}),
    ))
}
pub async fn login(
    s: &AppState,
    h: &HeaderMap,
    input: Credentials,
) -> Result<(HeaderMap, Json<Value>)> {
    origin(h, &s.config)?;
    let email = credentials(&input)?;
    rate(&s.pool, "login:global", 300).await?;
    rate(&s.pool, &format!("login:{email}"), 10).await?;
    let _permit = s
        .password_slots
        .acquire()
        .await
        .map_err(|_| Error::Internal)?;
    let row = sqlx::query("SELECT id,password_hash,status FROM identity.users WHERE email=$1")
        .bind(email)
        .fetch_optional(&s.pool)
        .await?;
    let hash = row
        .as_ref()
        .map(|r| r.get::<String, _>("password_hash"))
        .unwrap_or(s.dummy_hash.clone());
    let ok = verify(input.password, hash.clone()).await;
    let mut tx = s.pool.begin().await?;
    if !ok
        || row
            .as_ref()
            .is_none_or(|r| r.get::<String, _>("status") != "ACTIVE")
    {
        operations::audit(
            &mut tx,
            None,
            None,
            None,
            "auth.login_failed",
            "BAD_CREDENTIALS",
            request_id(h),
        )
        .await?;
        tx.commit().await?;
        return Err(Error::Unauthorized);
    }
    let user: Uuid = row.unwrap().get("id");
    // Serialize with password change and recheck the exact credential version before session issuance.
    let current: Option<String> = sqlx::query_scalar("SELECT password_hash FROM identity.users WHERE id=$1 AND status='ACTIVE' FOR SHARE")
        .bind(user).fetch_optional(&mut *tx).await?;
    if current.as_deref() != Some(hash.as_str()) {
        return Err(Error::Unauthorized);
    }
    let token = random_token();
    let csrf = random_token();
    sqlx::query("INSERT INTO identity.sessions(token_hash,user_id,csrf_hash,expires_at) VALUES($1,$2,$3,now()+make_interval(secs=>$4))")
 .bind(hash_token(&token)).bind(user).bind(hash_token(&csrf)).bind(s.config.session_seconds as f64).execute(&mut *tx).await?;
    operations::audit(
        &mut tx,
        Some(user),
        None,
        Some(user),
        "auth.login",
        "PASSWORD",
        request_id(h),
    )
    .await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        "set-cookie",
        cookie(&s.config, &token, s.config.session_seconds)
            .parse()
            .map_err(|_| Error::Internal)?,
    );
    Ok((
        headers,
        Json(json!({"user_id":user,"csrf_token":csrf,"expires_in":s.config.session_seconds})),
    ))
}
pub fn cookie(c: &Config, token: &str, max_age: i64) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{}",
        c.cookie_name(),
        token,
        max_age,
        if c.secure_cookie { "; Secure" } else { "" }
    )
}
pub async fn logout(s: &AppState, h: &HeaderMap) -> Result<(HeaderMap, Json<Value>)> {
    let a = actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE identity.sessions SET revoked_at=now() WHERE token_hash=$1")
        .bind(a.session_hash)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "auth.logout",
        "SELF_LOGOUT",
        a.request,
    )
    .await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    headers.insert("set-cookie", cookie(&s.config, "", 0).parse().unwrap());
    Ok((headers, Json(json!({"revoked":true}))))
}
fn request_id(headers: &HeaderMap) -> Uuid {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(Uuid::new_v4)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChange {
    pub current_password: String,
    pub new_password: String,
}
async fn lock_current_session(c: &mut PgConnection, a: &Actor) -> Result<()> {
    let id: Option<Uuid> = sqlx::query_scalar("SELECT user_id FROM identity.sessions WHERE user_id=$1 AND token_hash=$2 AND revoked_at IS NULL AND expires_at>clock_timestamp() FOR SHARE")
        .bind(a.user).bind(&a.session_hash).fetch_optional(c).await?;
    id.ok_or(Error::Unauthorized)?;
    Ok(())
}
pub async fn sessions(s: &AppState, h: &HeaderMap, page: crate::catalog::Page) -> Result<Json<Value>> {
    let a = actor(&s.pool, h, &s.config, false).await?;
    let limit = page.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(Error::Invalid);
    }
    let mut rows: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'created_at',created_at,'expires_at',expires_at,'current',token_hash=$2) FROM identity.sessions WHERE user_id=$1 AND revoked_at IS NULL AND expires_at>now() AND ($3::uuid IS NULL OR id>$3) ORDER BY id LIMIT $4")
        .bind(a.user).bind(&a.session_hash).bind(page.after).bind(limit+1).fetch_all(&s.pool).await?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = if more { rows.last().map(|r| r["id"].clone()) } else { None };
    Ok(Json(json!({"items":rows,"next_cursor":next,"limit":limit})))
}
pub async fn revoke_session(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<(HeaderMap, Json<Value>)> {
    let a = actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    // All session-management writes lock user first to avoid mutually revoking-session deadlocks.
    sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE").bind(a.user).execute(&mut *tx).await?;
    lock_current_session(&mut tx, &a).await?;
    let current: Option<bool> = sqlx::query_scalar("UPDATE identity.sessions SET revoked_at=COALESCE(revoked_at,now()) WHERE user_id=$1 AND id=$2 RETURNING token_hash=$3")
        .bind(a.user).bind(id).bind(&a.session_hash).fetch_optional(&mut *tx).await?;
    let current = current.ok_or(Error::NotFound)?;
    operations::audit(&mut tx, Some(a.user), None, Some(id), "auth.session_revoked", "USER_REQUEST", a.request).await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    if current {
        headers.insert("set-cookie", cookie(&s.config, "", 0).parse().unwrap());
    }
    Ok((headers, Json(json!({"revoked":true,"reauthentication_required":current}))))
}
pub async fn logout_all(s: &AppState, h: &HeaderMap) -> Result<(HeaderMap, Json<Value>)> {
    let a = actor(&s.pool, h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    sqlx::query("SELECT id FROM identity.users WHERE id=$1 FOR UPDATE").bind(a.user).execute(&mut *tx).await?;
    lock_current_session(&mut tx, &a).await?;
    sqlx::query("UPDATE identity.sessions SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL").bind(a.user).execute(&mut *tx).await?;
    operations::audit(&mut tx, Some(a.user), None, Some(a.user), "auth.logout_all", "USER_REQUEST", a.request).await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    headers.insert("set-cookie", cookie(&s.config, "", 0).parse().unwrap());
    Ok((headers, Json(json!({"revoked":true,"reauthentication_required":true}))))
}
pub async fn change_password(s: &AppState, h: &HeaderMap, i: PasswordChange) -> Result<(HeaderMap, Json<Value>)> {
    let a = actor(&s.pool, h, &s.config, true).await?;
    if !(12..=128).contains(&i.new_password.len()) || i.current_password.len()>128 || i.current_password==i.new_password {
        return Err(Error::Invalid);
    }
    rate(&s.pool, &format!("password:{}", a.user), 5).await?;
    let _permit = s.password_slots.acquire().await.map_err(|_| Error::Internal)?;
    let mut tx = s.pool.begin().await?;
    let old: String = sqlx::query_scalar("SELECT password_hash FROM identity.users WHERE id=$1 AND status='ACTIVE' FOR UPDATE")
        .bind(a.user).fetch_optional(&mut *tx).await?.ok_or(Error::Unauthorized)?;
    lock_current_session(&mut tx, &a).await?;
    if !verify(i.current_password, old).await {
        operations::audit(&mut tx, Some(a.user), None, Some(a.user), "auth.password_change_failed", "BAD_CREDENTIALS", a.request).await?;
        tx.commit().await?;
        return Err(Error::Unauthorized);
    }
    let hash = password_hash(i.new_password).await?;
    sqlx::query("UPDATE identity.users SET password_hash=$2 WHERE id=$1").bind(a.user).bind(hash).execute(&mut *tx).await?;
    sqlx::query("UPDATE identity.sessions SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL").bind(a.user).execute(&mut *tx).await?;
    operations::audit(&mut tx, Some(a.user), None, Some(a.user), "auth.password_changed", "ALL_SESSIONS_REVOKED", a.request).await?;
    tx.commit().await?;
    let mut headers = HeaderMap::new();
    headers.insert("set-cookie", cookie(&s.config, "", 0).parse().unwrap());
    Ok((headers, Json(json!({"changed":true,"reauthentication_required":true}))))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> Config {
        Config {
            database_url: String::new(),
            origin: "https://studio.audeniq.test".into(),
            service_secret: "s".repeat(32),
            secure_cookie: true,
            bind: String::new(),
            session_seconds: 60,
        }
    }
    #[tokio::test]
    async fn argon2id_password_success_failure_and_random_salt() {
        let password = "long-test-password".to_string();
        let hash = password_hash(password.clone()).await.unwrap();
        let other = password_hash(password.clone()).await.unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert_ne!(hash, other);
        assert!(verify(password, hash.clone()).await);
        assert!(!verify("wrong".into(), hash).await);
    }
    #[test]
    fn session_token_entropy_and_digest() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert_eq!(hash_token(&a).len(), 32);
        assert!(!secret_eq(&a, &b));
        assert!(secret_eq(&a, &a));
    }
    #[test]
    fn origin_exact_and_fetch_metadata() {
        let c = config();
        let mut h = HeaderMap::new();
        assert!(origin(&h, &c).is_err());
        h.insert("origin", c.origin.parse().unwrap());
        assert!(origin(&h, &c).is_ok());
        h.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert!(origin(&h, &c).is_err());
        h.remove("sec-fetch-site");
        h.insert(
            "origin",
            "https://studio.audeniq.test.evil.test".parse().unwrap(),
        );
        assert!(origin(&h, &c).is_err());
    }
    #[test]
    fn production_cookie_is_host_only_secure() {
        let cookie = cookie(&config(), "TOKEN", 60);
        assert!(cookie.starts_with("__Host-audeniq_session="));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("; Secure"));
        assert!(!cookie.contains("Domain="));
    }
}
