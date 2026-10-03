//! 권리 서류 서명 요청: 권리자 본인이 본인확인 후 직접 서명하는 전자 문서.
//!
//! Flow (docs/API.md "Rights signing"):
//! 1. The artist prepares the document and opens a signing request
//!    (`POST /api/orgs/{org}/signing-requests`). The answer carries a one-time
//!    link token; only its SHA-256 is stored.
//! 2. The rights holder opens `/sign/{token}` (sent by the artist, or on the
//!    artist's device handed over in person) without an AUDENIQ account.
//! 3. They pass identity verification with an external provider. The verified
//!    name must match the signer named in the document.
//! 4. They read the document, give the three consents (contents, electronic
//!    signature, personal data) and draw their own signature.
//! 5. The server creates the RIGHTS_PROOF document (form AUD-RIGHTS 2.0,
//!    REVIEW) and a certificate hash over the document, signature, identity,
//!    consents and the hash-chained event log. Staff review stays separate.
//!
//! Identity verification needs a contract with a provider (PortOne, KG
//! Inicis …). Until one is configured the provider is `Unconfigured`: links
//! can be created and read, but nobody can verify or sign, so no document is
//! produced without a verified signer.
use crate::{
    api::AppState,
    auth::{self, Actor},
    domain::{digest, sha256_hex},
    error::{Error, Result},
    operations, text_policy,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub const FORM: &str = "AUD-RIGHTS 2.0";
const KINDS: [&str; 6] = [
    "master",
    "artwork",
    "composition",
    "sample",
    "performer",
    "shared",
];
const ROLES: [&str; 3] = ["권리자 본인", "권리자의 위임을 받은 대리인", "법인 대표자"];
/// A link sent to someone else stays open for a week; a device handed over
/// in person only for the signing session.
const LINK_DAYS: i64 = 7;
const IN_PERSON_MINUTES: i64 = 60;
/// Identity verification must be fresh when the signature is drawn.
const IDENTITY_FRESH_MINUTES: i64 = 30;
/// The signer can reopen the signed copy for this long after signing.
const COPY_DAYS: i64 = 90;

// ---------------------------------------------------------------------------
// Identity provider
// ---------------------------------------------------------------------------

/// What a provider proved about the person who signs. Only the CI's peppered
/// hash is kept, never the CI itself or a phone number.
pub struct VerifiedIdentity {
    pub provider: &'static str,
    pub method: String,
    pub transaction_id: String,
    pub name: String,
    pub birth_date: String,
    pub ci: String,
}

pub enum Provider {
    /// No provider contract yet: verification and signing are refused.
    Unconfigured,
    /// Integration tests and local demos only (`IDENTITY_PROVIDER=test`,
    /// never with `APP_ENV=production`). The transaction id is
    /// `test:<name>:<YYYY-MM-DD>`.
    Test,
}

impl Provider {
    pub fn from_env() -> Self {
        let production = std::env::var("APP_ENV").as_deref() == Ok("production");
        match std::env::var("IDENTITY_PROVIDER").as_deref() {
            Ok("test") if !production => Self::Test,
            _ => Self::Unconfigured,
        }
    }
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Self::Unconfigured => None,
            Self::Test => Some("test"),
        }
    }
    async fn verify(&self, transaction_id: &str) -> Result<VerifiedIdentity> {
        match self {
            Self::Unconfigured => Err(Error::PolicyGate("IDENTITY_PROVIDER_NOT_CONFIGURED")),
            Self::Test => {
                let mut parts = transaction_id.splitn(3, ':');
                let (Some("test"), Some(name), Some(birth)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return Err(Error::InvalidCode("IDENTITY_NOT_VERIFIED"));
                };
                if name.trim().is_empty()
                    || chrono::NaiveDate::parse_from_str(birth, "%Y-%m-%d").is_err()
                {
                    return Err(Error::InvalidCode("IDENTITY_NOT_VERIFIED"));
                }
                Ok(VerifiedIdentity {
                    provider: "test",
                    method: "TEST".into(),
                    transaction_id: transaction_id.into(),
                    name: name.trim().into(),
                    birth_date: birth.into(),
                    ci: format!("test-ci:{name}:{birth}"),
                })
            }
        }
    }
}

/// Names compared without spaces (홍 길동 = 홍길동) and case.
fn same_name(a: &str, b: &str) -> bool {
    let n = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    !n(a).is_empty() && n(a) == n(b)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------
fn text(s: &str, min: usize, max: usize) -> Result<String> {
    let t = s.trim();
    let n = t.chars().count();
    if n < min || n > max {
        return Err(Error::Invalid);
    }
    text_policy::check(t)?;
    Ok(t.to_string())
}

fn signature(s: &str) -> Result<String> {
    let body = s
        .strip_prefix("data:image/png;base64,")
        .ok_or(Error::InvalidCode("SIGNATURE_INVALID"))?;
    if s.len() > 60_000
        || body.len() < 16
        || !body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return Err(Error::InvalidCode("SIGNATURE_INVALID"));
    }
    Ok(s.to_string())
}

/// Peppered hash so stored evidence can be matched later without keeping the
/// raw value (IP address, provider CI).
fn pepper_hash(s: &AppState, kind: &str, value: &str) -> String {
    sha256_hex(format!("{kind}\0{}\0{value}", s.config.service_secret))
}

fn user_agent(h: &HeaderMap) -> String {
    h.get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(300)
        .collect()
}

/// Append one evidence event; its hash covers the previous event's hash.
async fn event(
    c: &mut PgConnection,
    s: &AppState,
    h: Option<&HeaderMap>,
    request: Uuid,
    kind: &str,
    actor: Option<Uuid>,
    detail: Value,
) -> Result<String> {
    let prev: Option<String> = sqlx::query_scalar(
        "SELECT hash FROM portal.signing_events WHERE request_id=$1 ORDER BY id DESC LIMIT 1",
    )
    .bind(request)
    .fetch_optional(&mut *c)
    .await?;
    let at: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *c)
        .await?;
    let ip = h
        .and_then(auth::client_ip_key)
        .map(|ip| pepper_hash(s, "ip", &ip));
    let ua = h.map(user_agent).unwrap_or_default();
    let hash = digest(&json!({
        "request_id": request, "event": kind, "at": at.to_rfc3339(),
        "actor_user_id": actor, "ip_hash": ip, "user_agent": ua,
        "detail": detail, "prev_hash": prev,
    }));
    sqlx::query(
        "INSERT INTO portal.signing_events(request_id,event,at,actor_user_id,ip_hash,user_agent,detail,prev_hash,hash)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(request)
    .bind(kind)
    .bind(at)
    .bind(actor)
    .bind(&ip)
    .bind(&ua)
    .bind(&detail)
    .bind(&prev)
    .bind(&hash)
    .execute(&mut *c)
    .await?;
    Ok(hash)
}

fn new_token() -> (String, Vec<u8>) {
    let t = auth::random_token();
    let h = auth::hash_token(&t);
    (t, h)
}

fn expiry(channel: &str) -> DateTime<Utc> {
    Utc::now()
        + if channel == "IN_PERSON" {
            Duration::minutes(IN_PERSON_MINUTES)
        } else {
            Duration::days(LINK_DAYS)
        }
}

/// What the artist sees about a request: never the token, the birth date or
/// the CI hash.
fn summary(r: &sqlx::postgres::PgRow) -> Value {
    let identity: Option<Value> = r.get("identity");
    json!({
        "id": r.get::<Uuid, _>("id"),
        "release_id": r.get::<Uuid, _>("release_id"),
        "document_no": r.get::<Uuid, _>("document_no"),
        "form": r.get::<String, _>("form"),
        "document_kind": r.get::<String, _>("document_kind"),
        "title": r.get::<String, _>("title"),
        "rights_holder": r.get::<String, _>("rights_holder"),
        "signer_name": r.get::<String, _>("signer_name"),
        "signer_role": r.get::<String, _>("signer_role"),
        "channel": r.get::<String, _>("channel"),
        "status": effective_status(r),
        "expires_at": r.get::<DateTime<Utc>, _>("expires_at"),
        "signed_at": r.get::<Option<DateTime<Utc>>, _>("signed_at"),
        "document_id": r.get::<Option<Uuid>, _>("document_id"),
        "certificate_hash": r.get::<Option<String>, _>("certificate_hash"),
        "decline_reason": r.get::<String, _>("decline_reason"),
        "identity": identity.map(|i| json!({
            "provider": i["provider"], "method": i["method"], "verified_at": i["verified_at"],
        })),
        "created_at": r.get::<DateTime<Utc>, _>("created_at"),
    })
}

/// PENDING/VERIFIED past the deadline read as EXPIRED (nothing to sweep).
fn effective_status(r: &sqlx::postgres::PgRow) -> String {
    let status: String = r.get("status");
    let expires: DateTime<Utc> = r.get("expires_at");
    if matches!(status.as_str(), "PENDING" | "VERIFIED") && expires <= Utc::now() {
        "EXPIRED".into()
    } else {
        status
    }
}

macro_rules! cols {
    () => {
        "id,org_id,release_id,document_no,form,document_kind,title,body,body_hash,rights_holder,signer_name,signer_role,channel,status,expires_at,identity,signature,consents,signed_at,decline_reason,document_id,certificate_hash,created_by,created_at"
    };
}

// ---------------------------------------------------------------------------
// Artist side (signed-in)
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub release_id: Uuid,
    pub document_no: Uuid,
    pub document_kind: String,
    pub title: String,
    pub body: String,
    pub rights_holder: String,
    pub signer_name: String,
    pub signer_role: String,
    pub channel: String,
}

fn body_hash(i: &CreateInput, title: &str, body: &str, holder: &str, signer: &str) -> String {
    digest(&json!({
        "form": FORM, "document_no": i.document_no, "document_kind": i.document_kind,
        "title": title, "body": body, "rights_holder": holder,
        "signer_name": signer, "signer_role": i.signer_role,
    }))
}

/// Opens a signing request. Retrying with the same document number and the
/// same text issues a fresh link for the still-open request; changed text
/// under the same number is a conflict.
pub async fn create(
    s: &AppState,
    a: &Actor,
    h: &HeaderMap,
    org: Uuid,
    i: CreateInput,
) -> Result<Value> {
    if !KINDS.contains(&i.document_kind.as_str())
        || !ROLES.contains(&i.signer_role.as_str())
        || !matches!(i.channel.as_str(), "LINK" | "IN_PERSON")
    {
        return Err(Error::Invalid);
    }
    let title = text(&i.title, 1, 200)?;
    let body = i.body.trim().to_string();
    if body.is_empty() || body.chars().count() > 20000 {
        return Err(Error::Invalid);
    }
    text_policy::check_multiline(&body)?;
    let holder = text(&i.rights_holder, 1, 120)?;
    let signer = text(&i.signer_name, 1, 120)?;
    let hash = body_hash(&i, &title, &body, &holder, &signer);
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, i.release_id, "release", true).await?;
    let (token, token_hash) = new_token();
    let expires = expiry(&i.channel);
    let id = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO portal.signing_requests(id,org_id,release_id,document_no,form,document_kind,title,body,body_hash,
           rights_holder,signer_name,signer_role,channel,token_hash,expires_at,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
         ON CONFLICT (org_id,document_no) DO NOTHING RETURNING id",
    )
    .bind(id)
    .bind(org)
    .bind(i.release_id)
    .bind(i.document_no)
    .bind(FORM)
    .bind(&i.document_kind)
    .bind(&title)
    .bind(&body)
    .bind(&hash)
    .bind(&holder)
    .bind(&signer)
    .bind(&i.signer_role)
    .bind(&i.channel)
    .bind(&token_hash)
    .bind(expires)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?;
    let id = match inserted {
        Some(id) => {
            event(
                &mut tx,
                s,
                Some(h),
                id,
                "CREATED",
                Some(a.user),
                json!({"channel": i.channel, "body_hash": hash}),
            )
            .await?;
            operations::audit(
                &mut tx,
                Some(a.user),
                Some(org),
                Some(id),
                "portal.signing_request.created",
                "RIGHTS_SIGNING_REQUESTED",
                a.request,
            )
            .await?;
            id
        }
        None => {
            let row = sqlx::query(
                "SELECT id,release_id,body_hash,status FROM portal.signing_requests
                 WHERE org_id=$1 AND document_no=$2 FOR UPDATE",
            )
            .bind(org)
            .bind(i.document_no)
            .fetch_one(&mut *tx)
            .await?;
            let existing: Uuid = row.get("id");
            if row.get::<Uuid, _>("release_id") != i.release_id
                || row.get::<String, _>("body_hash") != hash
                || row.get::<String, _>("status") != "PENDING"
            {
                return Err(Error::Conflict);
            }
            sqlx::query(
                "UPDATE portal.signing_requests SET token_hash=$2,expires_at=$3,channel=$4,updated_at=now() WHERE id=$1",
            )
            .bind(existing)
            .bind(&token_hash)
            .bind(expires)
            .bind(&i.channel)
            .execute(&mut *tx)
            .await?;
            event(
                &mut tx,
                s,
                Some(h),
                existing,
                "LINK_REISSUED",
                Some(a.user),
                json!({"channel": i.channel}),
            )
            .await?;
            existing
        }
    };
    tx.commit().await?;
    Ok(json!({"id": id, "token": token, "expires_at": expires, "status": "PENDING"}))
}

#[derive(Deserialize)]
pub struct ListQuery {
    pub release_id: Option<Uuid>,
}

pub async fn list(s: &AppState, a: &Actor, org: Uuid, q: ListQuery) -> Result<Value> {
    let mut c = s.pool.acquire().await?;
    auth::membership(&mut c, a, org, false).await?;
    let rows = sqlx::query(concat!(
        "SELECT ",
        cols!(),
        " FROM portal.signing_requests WHERE org_id=$1 AND ($2::uuid IS NULL OR release_id=$2)
         ORDER BY created_at DESC LIMIT 200"
    ))
    .bind(org)
    .bind(q.release_id)
    .fetch_all(&mut *c)
    .await?;
    let mut items = Vec::new();
    for r in &rows {
        let release: Uuid = r.get("release_id");
        if auth::authorize(&mut c, a, org, release, "release", false)
            .await
            .is_ok()
        {
            items.push(summary(r));
        }
    }
    Ok(json!({"items": items}))
}

async fn owned(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow> {
    let row = sqlx::query(concat!(
        "SELECT ",
        cols!(),
        " FROM portal.signing_requests WHERE org_id=$1 AND id=$2 FOR UPDATE"
    ))
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    auth::authorize(c, a, org, row.get("release_id"), "release", true).await?;
    Ok(row)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReissueInput {
    pub channel: String,
}

/// A new link (the old one stops working) and a new deadline. Only while
/// nobody has verified yet, so a verified session cannot be handed to someone else.
pub async fn reissue(
    s: &AppState,
    a: &Actor,
    h: &HeaderMap,
    org: Uuid,
    id: Uuid,
    i: ReissueInput,
) -> Result<Value> {
    if !matches!(i.channel.as_str(), "LINK" | "IN_PERSON") {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    let row = owned(&mut tx, a, org, id).await?;
    if row.get::<String, _>("status") != "PENDING" {
        return Err(Error::InvalidCode("SIGNING_REQUEST_CLOSED"));
    }
    let (token, token_hash) = new_token();
    let expires = expiry(&i.channel);
    sqlx::query(
        "UPDATE portal.signing_requests SET token_hash=$2,expires_at=$3,channel=$4,updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(&token_hash)
    .bind(expires)
    .bind(&i.channel)
    .execute(&mut *tx)
    .await?;
    event(
        &mut tx,
        s,
        Some(h),
        id,
        "LINK_REISSUED",
        Some(a.user),
        json!({"channel": i.channel}),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "token": token, "expires_at": expires, "status": "PENDING"}))
}

pub async fn cancel(s: &AppState, a: &Actor, h: &HeaderMap, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let row = owned(&mut tx, a, org, id).await?;
    if !matches!(
        row.get::<String, _>("status").as_str(),
        "PENDING" | "VERIFIED"
    ) {
        return Err(Error::InvalidCode("SIGNING_REQUEST_CLOSED"));
    }
    sqlx::query(
        "UPDATE portal.signing_requests SET status='CANCELLED',updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    event(
        &mut tx,
        s,
        Some(h),
        id,
        "CANCELLED",
        Some(a.user),
        json!({}),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "CANCELLED"}))
}

// ---------------------------------------------------------------------------
// Signer side (link token, no account)
// ---------------------------------------------------------------------------
async fn by_token(c: &mut PgConnection, token: &str, lock: bool) -> Result<sqlx::postgres::PgRow> {
    if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::NotFound);
    }
    let q = if lock {
        concat!(
            "SELECT ",
            cols!(),
            " FROM portal.signing_requests WHERE token_hash=$1 FOR UPDATE"
        )
    } else {
        concat!(
            "SELECT ",
            cols!(),
            " FROM portal.signing_requests WHERE token_hash=$1"
        )
    };
    sqlx::query(q)
        .bind(auth::hash_token(token))
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)
}

/// Open for verification/signing: PENDING or VERIFIED and before the deadline.
fn open(r: &sqlx::postgres::PgRow) -> Result<()> {
    match effective_status(r).as_str() {
        "PENDING" | "VERIFIED" => Ok(()),
        "EXPIRED" => Err(Error::InvalidCode("SIGNING_LINK_EXPIRED")),
        _ => Err(Error::InvalidCode("SIGNING_REQUEST_CLOSED")),
    }
}

pub async fn view(s: &AppState, h: &HeaderMap, token: &str) -> Result<Value> {
    auth::rate(
        &s.pool,
        &format!("sign-view:{}", auth::client_ip_key(h).unwrap_or_default()),
        300,
    )
    .await?;
    let mut tx = s.pool.begin().await?;
    let r = by_token(&mut tx, token, true).await?;
    let id: Uuid = r.get("id");
    let status = effective_status(&r);
    let signed_at: Option<DateTime<Utc>> = r.get("signed_at");
    if status == "SIGNED" && signed_at.is_some_and(|t| t + Duration::days(COPY_DAYS) < Utc::now()) {
        return Err(Error::NotFound);
    }
    let viewed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.signing_events WHERE request_id=$1 AND event='VIEWED')",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !viewed && matches!(status.as_str(), "PENDING" | "VERIFIED") {
        event(&mut tx, s, Some(h), id, "VIEWED", None, json!({})).await?;
    }
    let meta = sqlx::query(
        "SELECT r.title AS release_title, o.name AS org_name,
           COALESCE(NULLIF(btrim(r.draft->>'artist'),''), o.name) AS artist
         FROM catalog.releases r JOIN identity.orgs o ON o.id=r.org_id
         WHERE r.id=$1",
    )
    .bind(r.get::<Uuid, _>("release_id"))
    .fetch_optional(&mut *tx)
    .await
    .ok()
    .flatten();
    let events: Vec<Value> = if status == "SIGNED" {
        sqlx::query("SELECT event,at,hash FROM portal.signing_events WHERE request_id=$1 ORDER BY id")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?
            .iter()
            .map(|e| json!({"event": e.get::<String, _>("event"), "at": e.get::<DateTime<Utc>, _>("at"), "hash": e.get::<String, _>("hash")}))
            .collect()
    } else {
        Vec::new()
    };
    tx.commit().await?;
    let identity: Option<Value> = r.get("identity");
    let provider = Provider::from_env();
    Ok(json!({
        "status": status,
        "form": r.get::<String, _>("form"),
        "document_no": r.get::<Uuid, _>("document_no"),
        "document_kind": r.get::<String, _>("document_kind"),
        "title": r.get::<String, _>("title"),
        "body": r.get::<String, _>("body"),
        "body_hash": r.get::<String, _>("body_hash"),
        "rights_holder": r.get::<String, _>("rights_holder"),
        "signer_name": r.get::<String, _>("signer_name"),
        "signer_role": r.get::<String, _>("signer_role"),
        "channel": r.get::<String, _>("channel"),
        "expires_at": r.get::<DateTime<Utc>, _>("expires_at"),
        "release_title": meta.as_ref().map(|m| m.get::<String, _>("release_title")),
        "requested_by": meta.as_ref().map(|m| m.get::<String, _>("org_name")),
        "artist": meta.as_ref().map(|m| m.get::<String, _>("artist")),
        "identity": identity.map(|i| json!({"method": i["method"], "verified_at": i["verified_at"], "name": i["name"]})),
        "identity_provider": {"ready": provider.name().is_some(), "name": provider.name()},
        "signature": if status == "SIGNED" { Some(r.get::<String, _>("signature")) } else { None },
        "signed_at": signed_at,
        "certificate_hash": r.get::<Option<String>, _>("certificate_hash"),
        "events": events,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityInput {
    pub transaction_id: String,
}

pub async fn verify_identity(
    s: &AppState,
    h: &HeaderMap,
    token: &str,
    i: IdentityInput,
) -> Result<Value> {
    auth::origin(h, &s.config)?;
    let provider = Provider::from_env();
    let Some(provider_name) = provider.name() else {
        return Err(Error::PolicyGate("IDENTITY_PROVIDER_NOT_CONFIGURED"));
    };
    if i.transaction_id.is_empty() || i.transaction_id.len() > 200 {
        return Err(Error::Invalid);
    }
    auth::rate(&s.pool, &format!("sign-identity:{token}"), 10).await?;
    let mut tx = s.pool.begin().await?;
    let r = by_token(&mut tx, token, true).await?;
    open(&r)?;
    let id: Uuid = r.get("id");
    let signer: String = r.get("signer_name");
    let verified = match provider.verify(&i.transaction_id).await {
        Ok(v) => v,
        Err(e) => {
            event(
                &mut tx,
                s,
                Some(h),
                id,
                "IDENTITY_FAILED",
                None,
                json!({"provider": provider_name, "reason": "NOT_VERIFIED"}),
            )
            .await?;
            tx.commit().await?;
            return Err(e);
        }
    };
    // The same provider transaction cannot verify two requests.
    let reused: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.signing_requests WHERE id<>$1 AND identity->>'transaction_id'=$2 AND identity->>'provider'=$3)",
    )
    .bind(id)
    .bind(&verified.transaction_id)
    .bind(verified.provider)
    .fetch_one(&mut *tx)
    .await?;
    if reused {
        return Err(Error::InvalidCode("IDENTITY_NOT_VERIFIED"));
    }
    if !same_name(&verified.name, &signer) {
        event(
            &mut tx,
            s,
            Some(h),
            id,
            "IDENTITY_FAILED",
            None,
            json!({"provider": provider_name, "reason": "NAME_MISMATCH"}),
        )
        .await?;
        tx.commit().await?;
        return Err(Error::InvalidCode("IDENTITY_NAME_MISMATCH"));
    }
    let at: DateTime<Utc> = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *tx)
        .await?;
    let identity = json!({
        "provider": verified.provider, "method": verified.method,
        "transaction_id": verified.transaction_id, "name": verified.name,
        "birth_date": verified.birth_date,
        "ci_hash": pepper_hash(s, "ci", &verified.ci),
        "verified_at": at,
    });
    sqlx::query(
        "UPDATE portal.signing_requests SET status='VERIFIED',identity=$2,updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(&identity)
    .execute(&mut *tx)
    .await?;
    event(
        &mut tx,
        s,
        Some(h),
        id,
        "IDENTITY_VERIFIED",
        None,
        json!({"provider": verified.provider, "method": identity["method"], "transaction_id": identity["transaction_id"], "ci_hash": identity["ci_hash"]}),
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"status": "VERIFIED", "identity": {"method": identity["method"], "verified_at": at, "name": identity["name"]}}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Consents {
    /// 문서 내용을 모두 읽고 확인함
    pub document: bool,
    /// 전자 문서·전자서명으로 체결하는 데 동의함
    pub electronic_signature: bool,
    /// 본인확인 정보 수집·이용에 동의함
    pub privacy: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignInput {
    pub signature: String,
    pub consents: Consents,
}

pub async fn sign(s: &AppState, h: &HeaderMap, token: &str, i: SignInput) -> Result<Value> {
    auth::origin(h, &s.config)?;
    if !(i.consents.document && i.consents.electronic_signature && i.consents.privacy) {
        return Err(Error::InvalidCode("SIGNING_CONSENT_REQUIRED"));
    }
    let sig = signature(&i.signature)?;
    let mut tx = s.pool.begin().await?;
    let r = by_token(&mut tx, token, true).await?;
    open(&r)?;
    if r.get::<String, _>("status") != "VERIFIED" {
        return Err(Error::InvalidCode("IDENTITY_NOT_VERIFIED"));
    }
    let identity: Value = r.get("identity");
    let verified_at: DateTime<Utc> =
        serde_json::from_value(identity["verified_at"].clone()).map_err(|_| Error::Internal)?;
    if verified_at + Duration::minutes(IDENTITY_FRESH_MINUTES) < Utc::now() {
        return Err(Error::InvalidCode("IDENTITY_EXPIRED"));
    }
    let id: Uuid = r.get("id");
    let org: Uuid = r.get("org_id");
    let release: Uuid = r.get("release_id");
    let document_no: Uuid = r.get("document_no");
    let title: String = r.get("title");
    let body: String = r.get("body");
    let kind: String = r.get("document_kind");
    let holder: String = r.get("rights_holder");
    let signer: String = r.get("signer_name");
    let role: String = r.get("signer_role");
    let channel: String = r.get("channel");
    let created_by: Uuid = r.get("created_by");
    let consents = json!({"document": true, "electronic_signature": true, "privacy": true});
    let signed_hash = event(
        &mut tx,
        s,
        Some(h),
        id,
        "SIGNED",
        None,
        json!({"signature_hash": sha256_hex(&sig), "consents": consents}),
    )
    .await?;
    let signed_at: DateTime<Utc> =
        sqlx::query_scalar("SELECT at FROM portal.signing_events WHERE hash=$1")
            .bind(&signed_hash)
            .fetch_one(&mut *tx)
            .await?;
    // Same content hash shape as the 1.0 records: text + signer + signature.
    let content_hash = digest(&json!({
        "title": title, "body": body, "document_no": document_no, "form": FORM,
        "document_kind": kind, "rights_holder": holder, "signer_name": signer,
        "signer_role": role, "signature": sig,
    }));
    let certificate_hash = digest(&json!({
        "signing_request_id": id, "document_no": document_no, "form": FORM,
        "body_hash": r.get::<String, _>("body_hash"), "content_hash": content_hash,
        "signature_hash": sha256_hex(&sig), "identity": identity, "consents": consents,
        "signed_at": signed_at.to_rfc3339(), "events_head": signed_hash,
    }));
    let record = json!({
        "document_no": document_no, "form": FORM, "document_kind": kind,
        "rights_holder": holder, "signer_name": signer, "signer_role": role,
        "content_hash": content_hash, "signing_request_id": id, "channel": channel,
        "identity": {"provider": identity["provider"], "method": identity["method"],
                     "name": identity["name"], "verified_at": identity["verified_at"]},
        "consents": consents, "certificate_hash": certificate_hash,
    });
    let doc = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.documents(id,org_id,release_id,kind,title,body,status,checked_at,signer_name,signature,signed_by,signed_at,electronic_record)
         VALUES($1,$2,$3,'RIGHTS_PROOF',$4,$5,'REVIEW',$6,$7,$8,$9,$6,$10)",
    )
    .bind(doc)
    .bind(org)
    .bind(release)
    .bind(&title)
    .bind(&body)
    .bind(signed_at)
    .bind(&signer)
    .bind(&sig)
    .bind(created_by)
    .bind(&record)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE portal.signing_requests SET status='SIGNED',signature=$2,consents=$3,signed_at=$4,document_id=$5,certificate_hash=$6,updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(&sig)
    .bind(&consents)
    .bind(signed_at)
    .bind(doc)
    .bind(&certificate_hash)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        None,
        Some(org),
        Some(doc),
        "portal.rights_document.signed",
        "RIGHTS_HOLDER_VERIFIED_SIGNATURE",
        auth::request_id(h),
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"status": "SIGNED", "document_id": doc, "signed_at": signed_at, "certificate_hash": certificate_hash}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclineInput {
    #[serde(default)]
    pub reason: String,
}

pub async fn decline(s: &AppState, h: &HeaderMap, token: &str, i: DeclineInput) -> Result<Value> {
    auth::origin(h, &s.config)?;
    let reason = text(&i.reason, 0, 500)?;
    let mut tx = s.pool.begin().await?;
    let r = by_token(&mut tx, token, true).await?;
    open(&r)?;
    let id: Uuid = r.get("id");
    sqlx::query("UPDATE portal.signing_requests SET status='DECLINED',decline_reason=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(&reason)
        .execute(&mut *tx)
        .await?;
    event(
        &mut tx,
        s,
        Some(h),
        id,
        "DECLINED",
        None,
        json!({"reason": reason}),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"status": "DECLINED"}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<CreateInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(create(&s, &a, &h, org, i).await?))
}
async fn h_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    Query(q): Query<ListQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list(&s, &a, org, q).await?))
}
async fn h_reissue(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<ReissueInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(reissue(&s, &a, &h, org, id, i).await?))
}
async fn h_cancel(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(cancel(&s, &a, &h, org, id).await?))
}
async fn h_view(
    State(s): State<AppState>,
    Path(token): Path<String>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(view(&s, &h, &token).await?))
}
async fn h_identity(
    State(s): State<AppState>,
    Path(token): Path<String>,
    h: HeaderMap,
    Json(i): Json<IdentityInput>,
) -> Result<Json<Value>> {
    Ok(Json(verify_identity(&s, &h, &token, i).await?))
}
async fn h_sign(
    State(s): State<AppState>,
    Path(token): Path<String>,
    h: HeaderMap,
    Json(i): Json<SignInput>,
) -> Result<Json<Value>> {
    Ok(Json(sign(&s, &h, &token, i).await?))
}
async fn h_decline(
    State(s): State<AppState>,
    Path(token): Path<String>,
    h: HeaderMap,
    Json(i): Json<DeclineInput>,
) -> Result<Json<Value>> {
    Ok(Json(decline(&s, &h, &token, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/signing-requests",
            get(h_list).post(h_create),
        )
        .route(
            "/api/orgs/{org}/signing-requests/{id}/reissue",
            post(h_reissue),
        )
        .route(
            "/api/orgs/{org}/signing-requests/{id}/cancel",
            post(h_cancel),
        )
        .route("/api/sign/{token}", get(h_view))
        .route("/api/sign/{token}/identity", post(h_identity))
        .route("/api/sign/{token}/sign", post(h_sign))
        .route("/api/sign/{token}/decline", post(h_decline))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_without_spaces_or_case() {
        assert!(same_name("홍 길동", "홍길동"));
        assert!(same_name("Kim Seo", "kimseo"));
        assert!(!same_name("홍길동", "홍길순"));
        assert!(!same_name("", ""));
    }
}
