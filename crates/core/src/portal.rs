//! Studio portal API: the artist-facing features that the web studio used to
//! keep in browser storage — profile, payout account, inquiries,
//! notifications, agreement/rights documents, signed release applications,
//! settlement views and reports.
//!
//! Boundaries:
//! - Every call rechecks membership (and resource ACL for release-scoped
//!   records); writes need a non-VIEWER role, money/account writes need OWNER.
//! - Finance is read-only here. A payout request is a portal row that
//!   operations turns into a `finance.payout_orders` row (manual approval).
//! - The full payout account number never leaves this module in clear text:
//!   it is sealed with AES-256-GCM using `PAYOUT_ACCOUNT_KEY` (64 hex chars),
//!   and only bank, holder and the last digits are returned.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations, text_policy,
};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, AeadCore, OsRng},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use std::str::FromStr;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Shared checks
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

fn multiline(s: &str, min: usize, max: usize) -> Result<String> {
    let t = s.trim();
    let n = t.chars().count();
    if n < min || n > max {
        return Err(Error::Invalid);
    }
    text_policy::check_multiline(t)?;
    Ok(t.to_string())
}

/// Small PNG signature from the studio signature pad (data URL, ≤ 60 000 chars).
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

async fn member(c: &mut PgConnection, a: &Actor, org: Uuid, write: bool) -> Result<String> {
    auth::membership(c, a, org, write).await
}

async fn owner(c: &mut PgConnection, a: &Actor, org: Uuid) -> Result<()> {
    if member(c, a, org, true).await? != "OWNER" {
        return Err(Error::Forbidden);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Artist profile (per user)
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    pub display_name: String,
    pub contact_email: String,
    pub bio: String,
    pub country: String,
    pub row_version: i64,
}

pub async fn get_profile(s: &AppState, a: &Actor) -> Result<Value> {
    let v: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('display_name',p.display_name,'contact_email',p.contact_email,'bio',p.bio,'country',p.country,'row_version',p.row_version,'updated_at',p.updated_at)
         FROM portal.artist_profiles p WHERE p.user_id=$1",
    )
    .bind(a.user)
    .fetch_optional(&s.pool)
    .await?;
    if let Some(v) = v {
        return Ok(v);
    }
    let email: String = sqlx::query_scalar("SELECT email FROM identity.users WHERE id=$1")
        .bind(a.user)
        .fetch_one(&s.pool)
        .await?;
    Ok(
        json!({"display_name":"","contact_email":email,"bio":"","country":"KR","row_version":0,"updated_at":null}),
    )
}

pub async fn put_profile(s: &AppState, a: &Actor, i: ProfileInput) -> Result<Value> {
    let name = text(&i.display_name, 0, 120)?;
    let email = text(&i.contact_email, 0, 254)?;
    if !email.is_empty() && (!email.contains('@') || email.contains(' ')) {
        return Err(Error::InvalidCode("EMAIL_INVALID"));
    }
    let bio = multiline(&i.bio, 0, 1500)?;
    let country = i.country.trim().to_ascii_uppercase();
    if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    let rv: Option<i64> = if i.row_version == 0 {
        sqlx::query_scalar(
            "INSERT INTO portal.artist_profiles(user_id,display_name,contact_email,bio,country) VALUES($1,$2,$3,$4,$5)
             ON CONFLICT(user_id) DO NOTHING RETURNING row_version",
        )
        .bind(a.user)
        .bind(&name)
        .bind(&email)
        .bind(&bio)
        .bind(&country)
        .fetch_optional(&mut *tx)
        .await?
    } else {
        sqlx::query_scalar(
            "UPDATE portal.artist_profiles SET display_name=$2,contact_email=$3,bio=$4,country=$5,row_version=row_version+1,updated_at=now()
             WHERE user_id=$1 AND row_version=$6 RETURNING row_version",
        )
        .bind(a.user)
        .bind(&name)
        .bind(&email)
        .bind(&bio)
        .bind(&country)
        .bind(i.row_version)
        .fetch_optional(&mut *tx)
        .await?
    };
    let rv = rv.ok_or(Error::Conflict)?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "portal.profile.updated",
        "USER_EDIT",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"row_version":rv}))
}

// ---------------------------------------------------------------------------
// Payout account (per organisation, OWNER writes)
// ---------------------------------------------------------------------------
const PAYEE_TYPES: [&str; 3] = ["INDIVIDUAL", "SOLE_PROPRIETOR", "CORPORATION"];

fn account_key() -> Result<Aes256Gcm> {
    let hexkey = std::env::var("PAYOUT_ACCOUNT_KEY")
        .map_err(|_| Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"))?;
    let bytes =
        hex::decode(hexkey.trim()).map_err(|_| Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"))?;
    Aes256Gcm::new_from_slice(&bytes).map_err(|_| Error::PolicyGate("PAYOUT_ACCOUNT_KEY_MISSING"))
}

/// nonce(12) || ciphertext+tag. The org id is bound as associated data so a
/// ciphertext copied to another organisation fails to open.
pub fn seal_account(org: Uuid, number: &str) -> Result<Vec<u8>> {
    let cipher = account_key()?;
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(
            &nonce,
            aes_gcm::aead::Payload {
                msg: number.as_bytes(),
                aad: org.as_bytes(),
            },
        )
        .map_err(|_| Error::Internal)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
}

/// For operations tooling that executes payouts (never exposed over HTTP).
pub fn open_account(org: Uuid, sealed: &[u8]) -> Result<String> {
    if sealed.len() < 13 {
        return Err(Error::Internal);
    }
    let cipher = account_key()?;
    let (nonce, ct) = sealed.split_at(12);
    let pt = cipher
        .decrypt(
            Nonce::from_slice(nonce),
            aes_gcm::aead::Payload {
                msg: ct,
                aad: org.as_bytes(),
            },
        )
        .map_err(|_| Error::Internal)?;
    String::from_utf8(pt).map_err(|_| Error::Internal)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayoutAccountInput {
    pub payee_type: String,
    pub holder_name: String,
    pub bank_name: String,
    pub account_number: String,
}

pub async fn get_payout_account(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let v: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('registered',true,'payee_type',payee_type,'holder_name',holder_name,'bank_name',bank_name,'account_last4',account_last4,'registered_at',registered_at,'row_version',row_version)
         FROM portal.payout_accounts WHERE org_id=$1",
    )
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(v.unwrap_or_else(|| json!({"registered":false})))
}

pub async fn put_payout_account(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    i: PayoutAccountInput,
) -> Result<Value> {
    if !PAYEE_TYPES.contains(&i.payee_type.as_str()) {
        return Err(Error::Invalid);
    }
    let holder = text(&i.holder_name, 1, 120)?;
    let bank = text(&i.bank_name, 1, 80)?;
    let digits: String = i
        .account_number
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    if digits.len() < 8
        || digits.len() > 20
        || i.account_number
            .chars()
            .any(|c| !(c.is_ascii_digit() || c == '-' || c == ' '))
    {
        return Err(Error::InvalidCode("ACCOUNT_NUMBER_INVALID"));
    }
    let last4 = digits[digits.len() - 4..].to_string();
    let sealed = seal_account(org, &digits)?;
    let mut tx = s.pool.begin().await?;
    owner(&mut tx, a, org).await?;
    sqlx::query(
        "INSERT INTO portal.payout_accounts(org_id,payee_type,holder_name,bank_name,account_last4,account_cipher,registered_by)
         VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT(org_id) DO UPDATE SET payee_type=EXCLUDED.payee_type,holder_name=EXCLUDED.holder_name,bank_name=EXCLUDED.bank_name,
           account_last4=EXCLUDED.account_last4,account_cipher=EXCLUDED.account_cipher,registered_by=EXCLUDED.registered_by,
           registered_at=now(),row_version=portal.payout_accounts.row_version+1",
    )
    .bind(org)
    .bind(&i.payee_type)
    .bind(&holder)
    .bind(&bank)
    .bind(&last4)
    .bind(&sealed)
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(org),
        "portal.payout_account.registered",
        "OWNER_EDIT",
        a.request,
    )
    .await?;
    sqlx::query(
        "SELECT portal.notify($1,'ACCOUNT','수익을 받을 계좌가 등록됐어요.',$2,'/settlement')",
    )
    .bind(org)
    .bind(format!("{bank} · •••• {last4} · {holder}"))
    .execute(&mut *tx)
    .await
    .ok();
    tx.commit().await?;
    Ok(json!({"registered":true,"account_last4":last4}))
}

// ---------------------------------------------------------------------------
// Inquiries
// ---------------------------------------------------------------------------
const CATEGORIES: [&str; 5] = ["RELEASE", "SETTLEMENT", "CONTRACT", "ACCOUNT", "OTHER"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InquiryInput {
    pub category: String,
    pub release_id: Option<Uuid>,
    pub subject: String,
    pub body: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageInput {
    pub body: String,
}

pub async fn list_inquiries(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',i.id,'category',i.category,'release_id',i.release_id,'release_title',r.title,'subject',i.subject,
           'status',i.status,'created_at',i.created_at,'updated_at',i.updated_at,
           'messages',(SELECT count(*) FROM portal.inquiry_messages m WHERE m.inquiry_id=i.id))
         FROM portal.inquiries i LEFT JOIN catalog.releases r ON r.id=i.release_id
         WHERE i.org_id=$1 ORDER BY i.created_at DESC LIMIT 200",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items":items}))
}

pub async fn get_inquiry(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let v: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',i.id,'category',i.category,'release_id',i.release_id,'release_title',r.title,'subject',i.subject,
           'status',i.status,'created_at',i.created_at,'updated_at',i.updated_at,
           'messages',COALESCE((SELECT jsonb_agg(jsonb_build_object('id',m.id,'author_kind',m.author_kind,'body',m.body,'created_at',m.created_at) ORDER BY m.created_at)
             FROM portal.inquiry_messages m WHERE m.inquiry_id=i.id),'[]'::jsonb))
         FROM portal.inquiries i LEFT JOIN catalog.releases r ON r.id=i.release_id WHERE i.org_id=$1 AND i.id=$2",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    tx.rollback().await?;
    Ok(v)
}

pub async fn create_inquiry(s: &AppState, a: &Actor, org: Uuid, i: InquiryInput) -> Result<Value> {
    if !CATEGORIES.contains(&i.category.as_str()) {
        return Err(Error::Invalid);
    }
    let subject = text(&i.subject, 1, 200)?;
    let body = multiline(&i.body, 1, 4000)?;
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, true).await?;
    if let Some(release) = i.release_id {
        auth::authorize(&mut tx, a, org, release, "release", false).await?;
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO portal.inquiries(id,org_id,created_by,category,release_id,subject) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(org)
        .bind(a.user)
        .bind(&i.category)
        .bind(i.release_id)
        .bind(&subject)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO portal.inquiry_messages(inquiry_id,org_id,author_kind,author_user,body) VALUES($1,$2,'ARTIST',$3,$4)")
        .bind(id)
        .bind(org)
        .bind(a.user)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "portal.inquiry.created",
        "USER_EDIT",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id}))
}

pub async fn add_message(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: MessageInput,
) -> Result<Value> {
    let body = multiline(&i.body, 1, 4000)?;
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, true).await?;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM portal.inquiries WHERE org_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if status == "CLOSED" {
        return Err(Error::PolicyGate("INQUIRY_CLOSED"));
    }
    sqlx::query("INSERT INTO portal.inquiry_messages(inquiry_id,org_id,author_kind,author_user,body) VALUES($1,$2,'ARTIST',$3,$4)")
        .bind(id)
        .bind(org)
        .bind(a.user)
        .bind(&body)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(json!({"id":id}))
}

pub async fn close_inquiry(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, true).await?;
    let n = sqlx::query(
        "UPDATE portal.inquiries SET status='CLOSED',updated_at=now() WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    tx.commit().await?;
    Ok(json!({"id":id,"status":"CLOSED"}))
}

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    #[serde(default)]
    pub ids: Vec<Uuid>,
    #[serde(default)]
    pub all: bool,
}

pub async fn list_notifications(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',n.id,'kind',n.kind,'title',n.title,'detail',n.detail,'link',n.link,'created_at',n.created_at,
           'read',EXISTS(SELECT 1 FROM portal.notification_reads r WHERE r.notification_id=n.id AND r.user_id=$2))
         FROM portal.notifications n WHERE n.org_id=$1 AND (n.user_id IS NULL OR n.user_id=$2)
         ORDER BY n.created_at DESC LIMIT 100",
    )
    .bind(org)
    .bind(a.user)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    let unread = items.iter().filter(|v| v["read"] == json!(false)).count();
    Ok(json!({"items":items,"unread":unread}))
}

pub async fn read_notifications(s: &AppState, a: &Actor, org: Uuid, i: ReadInput) -> Result<Value> {
    if !i.all && (i.ids.is_empty() || i.ids.len() > 100) {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let n = sqlx::query(
        "INSERT INTO portal.notification_reads(notification_id,user_id)
         SELECT n.id,$2 FROM portal.notifications n
         WHERE n.org_id=$1 AND (n.user_id IS NULL OR n.user_id=$2) AND ($3 OR n.id = ANY($4))
         ON CONFLICT DO NOTHING",
    )
    .bind(org)
    .bind(a.user)
    .bind(i.all)
    .bind(&i.ids)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(json!({"marked":n}))
}

// ---------------------------------------------------------------------------
// Documents (agreements to sign, rights proofs to submit)
// ---------------------------------------------------------------------------
const DOC_JSON: &str = "jsonb_build_object('id',d.id,'kind',d.kind,'release_id',d.release_id,'release_title',r.title,'title',d.title,'version',d.version,
  'body',d.body,'status',d.status,'review_note',d.review_note,'asset_id',d.asset_id,'file_name',d.file_name,'checked_at',d.checked_at,
  'signer_name',d.signer_name,'signature',d.signature,'signed_at',d.signed_at,'row_version',d.row_version,'created_at',d.created_at,'updated_at',d.updated_at)";

pub async fn list_documents(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(&format!(
        "SELECT {DOC_JSON} FROM portal.documents d LEFT JOIN catalog.releases r ON r.id=d.release_id
         WHERE d.org_id=$1 ORDER BY d.created_at DESC LIMIT 200"
    ))
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items":items}))
}

async fn locked_doc(
    c: &mut PgConnection,
    org: Uuid,
    id: Uuid,
) -> Result<(String, String, i64, Option<Uuid>)> {
    let row = sqlx::query("SELECT kind,status,row_version,checked_at IS NOT NULL AS checked,release_id FROM portal.documents WHERE org_id=$1 AND id=$2 FOR UPDATE")
        .bind(org)
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    let checked: bool = row.get("checked");
    Ok((
        row.get("kind"),
        if checked {
            format!("{}+CHECKED", row.get::<String, _>("status"))
        } else {
            row.get("status")
        },
        row.get("row_version"),
        row.get("release_id"),
    ))
}

pub async fn check_document(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, true).await?;
    locked_doc(&mut tx, org, id).await?;
    let rv: i64 = sqlx::query_scalar(
        "UPDATE portal.documents SET checked_at=COALESCE(checked_at,now()),row_version=row_version+1,updated_at=now() WHERE id=$1 RETURNING row_version",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"row_version":rv}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignInput {
    pub signer_name: String,
    pub signature: String,
    pub row_version: i64,
}

pub async fn sign_document(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: SignInput,
) -> Result<Value> {
    let name = text(&i.signer_name, 1, 120)?;
    let sig = signature(&i.signature)?;
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, true).await?;
    let (kind, status, rv, _) = locked_doc(&mut tx, org, id).await?;
    if rv != i.row_version {
        return Err(Error::Conflict);
    }
    if kind != "AGREEMENT" {
        return Err(Error::Invalid);
    }
    if status != "APPROVED+CHECKED" {
        // Signing is only possible after staff review and the artist's read confirmation.
        return Err(Error::PolicyGate(if status.starts_with("APPROVED") {
            "DOCUMENT_NOT_CHECKED"
        } else {
            "DOCUMENT_NOT_APPROVED"
        }));
    }
    let rv: i64 = sqlx::query_scalar(
        "UPDATE portal.documents SET status='SIGNED',signer_name=$2,signature=$3,signed_by=$4,signed_at=now(),row_version=row_version+1,updated_at=now()
         WHERE id=$1 RETURNING row_version",
    )
    .bind(id)
    .bind(&name)
    .bind(&sig)
    .bind(a.user)
    .fetch_one(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "portal.document.signed",
        "ARTIST_SIGNATURE",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"status":"SIGNED","row_version":rv}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentInput {
    pub release_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub asset_id: Option<Uuid>,
    #[serde(default)]
    pub file_name: String,
}

/// Artist-initiated rights proof for one of their releases (licence, consent
/// letter). With a file it goes straight to review; without one it waits.
pub async fn create_document(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    i: DocumentInput,
) -> Result<Value> {
    let title = text(&i.title, 1, 200)?;
    let body = multiline(&i.body, 0, 5000)?;
    let file = text(&i.file_name, 0, 200)?;
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, true).await?;
    auth::authorize(&mut tx, a, org, i.release_id, "release", true).await?;
    if let Some(asset) = i.asset_id {
        auth::authorize(&mut tx, a, org, asset, "asset", false).await?;
        let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.assets WHERE org_id=$1 AND id=$2 AND state='REGISTERED' AND kind IN ('DOCUMENT','IMAGE'))")
            .bind(org)
            .bind(asset)
            .fetch_one(&mut *tx)
            .await?;
        if !ok || file.is_empty() {
            return Err(Error::InvalidCode("DOCUMENT_ASSET_INVALID"));
        }
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.documents(id,org_id,release_id,kind,title,body,status,asset_id,file_name) VALUES($1,$2,$3,'RIGHTS_PROOF',$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(org)
    .bind(i.release_id)
    .bind(&title)
    .bind(&body)
    .bind(if i.asset_id.is_some() { "REVIEW" } else { "AWAITING_DOCUMENTS" })
    .bind(i.asset_id)
    .bind(&file)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "portal.document.created",
        "USER_EDIT",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofInput {
    pub asset_id: Uuid,
    pub file_name: String,
    pub row_version: i64,
}

pub async fn attach_proof(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: ProofInput,
) -> Result<Value> {
    let file = text(&i.file_name, 1, 200)?;
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, true).await?;
    let (kind, status, rv, _) = locked_doc(&mut tx, org, id).await?;
    if rv != i.row_version {
        return Err(Error::Conflict);
    }
    if kind != "RIGHTS_PROOF" || status.starts_with("APPROVED") {
        return Err(Error::PolicyGate("DOCUMENT_NOT_EDITABLE"));
    }
    auth::authorize(&mut tx, a, org, i.asset_id, "asset", false).await?;
    let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM catalog.assets WHERE org_id=$1 AND id=$2 AND state='REGISTERED' AND kind IN ('DOCUMENT','IMAGE'))")
        .bind(org)
        .bind(i.asset_id)
        .fetch_one(&mut *tx)
        .await?;
    if !ok {
        return Err(Error::InvalidCode("DOCUMENT_ASSET_INVALID"));
    }
    let rv: i64 = sqlx::query_scalar(
        "UPDATE portal.documents SET asset_id=$2,file_name=$3,status='REVIEW',review_note='',row_version=row_version+1,updated_at=now() WHERE id=$1 RETURNING row_version",
    )
    .bind(id)
    .bind(i.asset_id)
    .bind(&file)
    .fetch_one(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "portal.document.proof_submitted",
        "USER_EDIT",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"status":"REVIEW","row_version":rv}))
}

// ---------------------------------------------------------------------------
// Signed release application
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationInput {
    pub application_no: String,
    pub form: String,
    pub content_hash: String,
    pub signer_name: String,
    pub signer_role: String,
    pub agreements: Vec<String>,
    pub signature: String,
    pub submitted_at: String,
}

fn application_no_ok(no: &str) -> bool {
    let b = no.as_bytes();
    b.len() == 19
        && no.starts_with("AUD-")
        && b[4..12].iter().all(u8::is_ascii_digit)
        && b[12] == b'-'
        && b[13..]
            .iter()
            .all(|c| matches!(c, b'A'..=b'Z' | b'2'..=b'9'))
}

const APP_JSON: &str = "jsonb_build_object('application_no',application_no,'form',form,'content_hash',content_hash,'signer_name',signer_name,
  'signer_role',signer_role,'agreements',agreements,'signature',signature,'submitted_at',client_submitted_at,'received_at',received_at)";

pub async fn get_application(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let v: Option<Value> = sqlx::query_scalar(&format!(
        "SELECT {APP_JSON} FROM portal.release_applications WHERE org_id=$1 AND release_id=$2 ORDER BY received_at DESC LIMIT 1"
    ))
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    tx.rollback().await?;
    v.ok_or(Error::NotFound)
}

pub async fn record_application(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: ApplicationInput,
) -> Result<Value> {
    if !application_no_ok(&i.application_no) {
        return Err(Error::InvalidCode("APPLICATION_NO_INVALID"));
    }
    if i.content_hash.len() != 64
        || !i
            .content_hash
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::InvalidCode("APPLICATION_HASH_INVALID"));
    }
    if i.agreements.is_empty()
        || i.agreements.len() > 10
        || i.agreements
            .iter()
            .any(|x| x.is_empty() || x.len() > 20 || !x.bytes().all(|b| b.is_ascii_lowercase()))
    {
        return Err(Error::Invalid);
    }
    let form = text(&i.form, 1, 40)?;
    let signer = text(&i.signer_name, 1, 120)?;
    let role = text(&i.signer_role, 1, 60)?;
    let submitted = text(&i.submitted_at, 1, 40)?;
    let sig = signature(&i.signature)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let title: String =
        sqlx::query_scalar("SELECT title FROM catalog.releases WHERE org_id=$1 AND id=$2")
            .bind(org)
            .bind(release)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
    // Same number again = the client retried: return the stored record.
    let existing: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT release_id,content_hash FROM portal.release_applications WHERE application_no=$1",
    )
    .bind(&i.application_no)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((rel, hash)) = existing {
        if rel != release || hash != i.content_hash {
            return Err(Error::Conflict);
        }
        tx.rollback().await?;
        return Ok(json!({"application_no":i.application_no,"recorded":true}));
    } else {
        sqlx::query(
            "INSERT INTO portal.release_applications(id,org_id,release_id,application_no,form,content_hash,signer_name,signer_role,agreements,signature,client_submitted_at,submitted_by)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(Uuid::new_v4())
        .bind(org)
        .bind(release)
        .bind(&i.application_no)
        .bind(&form)
        .bind(&i.content_hash)
        .bind(&signer)
        .bind(&role)
        .bind(&i.agreements)
        .bind(&sig)
        .bind(&submitted)
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
        operations::audit(
            &mut tx,
            Some(a.user),
            Some(org),
            Some(release),
            "portal.application.signed",
            "ARTIST_SIGNATURE",
            a.request,
        )
        .await?;
    }
    // The distribution agreement for this release enters staff review.
    let body = format!(
        "AUDENIQ 디지털 음원 배급 신청·계약서\n\n발매: {title}\n신청서 번호: {no}\n신청인: {signer} ({role})\n문서 확인 코드: {hash}\n\n신청인은 위 발매 정보를 기준으로 AUDENIQ에 디지털 음원 배급을 신청하고, 배급 범위·정산·수정·테이크다운 및 권리 보증 조항에 동의합니다. AUDENIQ 검토가 끝나면 이 계약서에 서명해 계약이 체결됩니다.",
        no = i.application_no,
        hash = i.content_hash
    );
    sqlx::query(
        "INSERT INTO portal.documents(id,org_id,release_id,kind,title,body,status) VALUES($1,$2,$3,'AGREEMENT',$4,$5,'REVIEW')
         ON CONFLICT(org_id,release_id) WHERE kind='AGREEMENT' DO UPDATE
           SET body=EXCLUDED.body,title=EXCLUDED.title,
               status=CASE WHEN portal.documents.status='SIGNED' THEN 'SIGNED' ELSE 'REVIEW' END,
               row_version=portal.documents.row_version+1,updated_at=now()",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(release)
    .bind(format!("{} · AUDENIQ 디지털 음원 배급 신청·계약서", title.chars().take(120).collect::<String>()))
    .bind(&body)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(json!({"application_no":i.application_no,"recorded":true}))
}

// ---------------------------------------------------------------------------
// Settlement (read-only finance) and payout requests
// ---------------------------------------------------------------------------
/// Credits minus debits on the organisation's royalty payable, less money
/// already requested or in flight. Settled payouts are posted to the ledger
/// by finance operations, so only unsettled orders are subtracted.
async fn balances(c: &mut PgConnection, org: Uuid) -> Result<(Decimal, Decimal)> {
    let payable: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CASE WHEN e.side='CREDIT' THEN e.amount ELSE -e.amount END),0)
         FROM finance.ledger_entries e WHERE e.org_id=$1 AND e.account='ROYALTY_PAYABLE' AND e.currency='KRW'",
    )
    .bind(org)
    .fetch_one(&mut *c)
    .await?;
    let pending: Decimal = sqlx::query_scalar(
        "SELECT COALESCE((SELECT SUM(amount) FROM portal.payout_requests WHERE org_id=$1 AND status='REQUESTED' AND currency='KRW'),0)
              + COALESCE((SELECT SUM(amount) FROM finance.payout_orders WHERE org_id=$1 AND currency='KRW'
                          AND status IN ('PENDING_APPROVAL','APPROVED','SUBMITTED','SUBMITTED_UNKNOWN')),0)",
    )
    .bind(org)
    .fetch_one(&mut *c)
    .await?;
    Ok((payable, pending))
}

pub async fn finance_summary(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let (payable, pending) = balances(&mut tx, org).await?;
    let account: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM portal.payout_accounts WHERE org_id=$1)")
            .bind(org)
            .fetch_one(&mut *tx)
            .await?;
    tx.rollback().await?;
    let available = (payable - pending).max(Decimal::ZERO);
    Ok(
        json!({"currency":"KRW","payable":payable.to_string(),"pending":pending.to_string(),"available":available.to_string(),"account_registered":account,"minimum_payout":MIN_PAYOUT.to_string()}),
    )
}

pub async fn statements(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',t.id,'code',t.transaction_code,'description',t.description,'source',t.source_ref,
            'status',t.status,'created_at',t.created_at,'currency',t.currency,
            'amount',(SUM(CASE WHEN e.side='CREDIT' THEN e.amount ELSE -e.amount END))::text)
         FROM finance.ledger_transactions t JOIN finance.ledger_entries e ON e.transaction_id=t.id AND e.org_id=t.org_id
         WHERE t.org_id=$1 AND e.account='ROYALTY_PAYABLE'
         GROUP BY t.id ORDER BY t.created_at DESC LIMIT 200",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items":items}))
}

pub async fn list_payouts(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',p.id,'amount',p.amount::text,'currency',p.currency,'status',p.status,'note',p.note,'created_at',p.created_at,
            'order_status',o.status,'order_updated_at',COALESCE(o.approved_at,o.created_at))
         FROM portal.payout_requests p LEFT JOIN finance.payout_orders o ON o.id=p.payout_order_id
         WHERE p.org_id=$1 ORDER BY p.created_at DESC LIMIT 200",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items":items}))
}

pub const MIN_PAYOUT: i64 = 10_000;

/// "₩30,000" for user-facing notices.
fn won(amount: Decimal) -> String {
    let digits = amount.trunc().abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("₩{out}")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayoutRequestInput {
    pub amount: String,
    pub idempotency_key: String,
}

pub async fn request_payout(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    i: PayoutRequestInput,
) -> Result<Value> {
    let amount = Decimal::from_str(i.amount.trim()).map_err(|_| Error::Invalid)?;
    if amount.fract() != Decimal::ZERO || amount < Decimal::from(MIN_PAYOUT) {
        return Err(Error::PolicyGate("PAYOUT_BELOW_MINIMUM"));
    }
    let key = text(&i.idempotency_key, 8, 120)?;
    let mut tx = s.pool.begin().await?;
    owner(&mut tx, a, org).await?;
    // Serialise requests per organisation so two tabs cannot both spend the balance.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text, 7))")
        .bind(org)
        .execute(&mut *tx)
        .await?;
    if let Some(v) = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_object('id',id,'amount',amount::text,'status',status) FROM portal.payout_requests WHERE org_id=$1 AND idempotency_key=$2",
    )
    .bind(org)
    .bind(&key)
    .fetch_optional(&mut *tx)
    .await?
    {
        return Ok(v);
    }
    let account: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM portal.payout_accounts WHERE org_id=$1)")
            .bind(org)
            .fetch_one(&mut *tx)
            .await?;
    if !account {
        return Err(Error::PolicyGate("PAYOUT_ACCOUNT_REQUIRED"));
    }
    let held: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM finance.finance_holds WHERE org_id=$1 AND scope_type='PARTY' AND scope_value=$2 AND active)",
    )
    .bind(org)
    .bind(a.party.to_string())
    .fetch_one(&mut *tx)
    .await?;
    if held {
        return Err(Error::PolicyGate("PAYEE_ON_HOLD"));
    }
    let (payable, pending) = balances(&mut tx, org).await?;
    if amount > payable - pending {
        return Err(Error::PolicyGate("PAYOUT_EXCEEDS_BALANCE"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO portal.payout_requests(id,org_id,requested_by,payee_party_id,amount,currency,idempotency_key) VALUES($1,$2,$3,$4,$5,'KRW',$6)")
        .bind(id)
        .bind(org)
        .bind(a.user)
        .bind(a.party)
        .bind(amount)
        .bind(&key)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "portal.payout.requested",
        "OWNER_REQUEST",
        a.request,
    )
    .await?;
    sqlx::query("SELECT portal.notify($1,'SETTLEMENT','수익 지급을 요청했어요.',$2,'/settlement')")
        .bind(org)
        .bind(format!(
            "{} · 담당자 확인 후 등록한 계좌로 보내 드려요.",
            won(amount)
        ))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(json!({"id":id,"amount":amount.to_string(),"status":"REQUESTED"}))
}

// ---------------------------------------------------------------------------
// Reports (matched royalty report lines)
// ---------------------------------------------------------------------------
pub async fn reports(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    member(&mut tx, a, org, false).await?;
    let base = "FROM finance.report_lines l JOIN finance.royalty_reports rr ON rr.id=l.report_id
                WHERE l.org_id=$1 AND l.match_status='AUTO' AND rr.period_start >= (date_trunc('month', now()) - interval '11 months')";
    let by_month: Vec<Value> = sqlx::query_scalar(&format!(
        "SELECT jsonb_build_object('month',to_char(rr.period_start,'YYYY-MM'),'streams',COALESCE(SUM(l.quantity),0)::text,'revenue',COALESCE(SUM(l.gross_amount),0)::text)
         {base} GROUP BY to_char(rr.period_start,'YYYY-MM') ORDER BY 1"
    ))
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    let by_dsp: Vec<Value> = sqlx::query_scalar(&format!(
        "SELECT jsonb_build_object('dsp',rr.dsp_id,'streams',COALESCE(SUM(l.quantity),0)::text,'revenue',COALESCE(SUM(l.gross_amount),0)::text)
         {base} GROUP BY rr.dsp_id ORDER BY SUM(l.quantity) DESC NULLS LAST"
    ))
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    let by_release: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('release_id',r.id,'title',r.title,'streams',COALESCE(SUM(l.quantity),0)::text,'revenue',COALESCE(SUM(l.gross_amount),0)::text)
         FROM finance.report_lines l JOIN finance.royalty_reports rr ON rr.id=l.report_id
           JOIN catalog.releases r ON r.id=l.matched_release_id AND r.org_id=l.org_id
         WHERE l.org_id=$1 AND l.match_status='AUTO' AND rr.period_start >= (date_trunc('month', now()) - interval '11 months')
         GROUP BY r.id ORDER BY SUM(l.quantity) DESC NULLS LAST LIMIT 20",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    // Month × platform × release rows for the studio report table and chart.
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('month',to_char(rr.period_start,'YYYY-MM'),'dsp',rr.dsp_id,'release_id',r.id,'release',COALESCE(r.title,''),
            'streams',COALESCE(SUM(l.quantity),0)::text,'revenue',COALESCE(SUM(l.gross_amount),0)::text)
         FROM finance.report_lines l JOIN finance.royalty_reports rr ON rr.id=l.report_id
           LEFT JOIN catalog.releases r ON r.id=l.matched_release_id AND r.org_id=l.org_id
         WHERE l.org_id=$1 AND l.match_status='AUTO' AND rr.period_start >= (date_trunc('month', now()) - interval '11 months')
         GROUP BY to_char(rr.period_start,'YYYY-MM'), rr.dsp_id, r.id, r.title
         ORDER BY 1 DESC, SUM(l.gross_amount) DESC NULLS LAST LIMIT 500",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"by_month":by_month,"by_dsp":by_dsp,"by_release":by_release,"rows":rows}))
}

// ---------------------------------------------------------------------------
// HTTP routes (merged into api::router)
// ---------------------------------------------------------------------------
macro_rules! org_get {
    ($name:ident, $f:ident) => {
        async fn $name(
            State(s): State<AppState>,
            Path(org): Path<Uuid>,
            h: HeaderMap,
        ) -> Result<Json<Value>> {
            let a = auth::actor(&s.pool, &h, &s.config, false).await?;
            Ok(Json($f(&s, &a, org).await?))
        }
    };
}
macro_rules! org_body {
    ($name:ident, $f:ident, $input:ty) => {
        async fn $name(
            State(s): State<AppState>,
            Path(org): Path<Uuid>,
            h: HeaderMap,
            Json(i): Json<$input>,
        ) -> Result<Json<Value>> {
            let a = auth::actor(&s.pool, &h, &s.config, true).await?;
            Ok(Json($f(&s, &a, org, i).await?))
        }
    };
}
macro_rules! id_get {
    ($name:ident, $f:ident) => {
        async fn $name(
            State(s): State<AppState>,
            Path((org, id)): Path<(Uuid, Uuid)>,
            h: HeaderMap,
        ) -> Result<Json<Value>> {
            let a = auth::actor(&s.pool, &h, &s.config, false).await?;
            Ok(Json($f(&s, &a, org, id).await?))
        }
    };
}
macro_rules! id_post {
    ($name:ident, $f:ident) => {
        async fn $name(
            State(s): State<AppState>,
            Path((org, id)): Path<(Uuid, Uuid)>,
            h: HeaderMap,
            Json(_): Json<Empty>,
        ) -> Result<Json<Value>> {
            let a = auth::actor(&s.pool, &h, &s.config, true).await?;
            Ok(Json($f(&s, &a, org, id).await?))
        }
    };
}
macro_rules! id_body {
    ($name:ident, $f:ident, $input:ty) => {
        async fn $name(
            State(s): State<AppState>,
            Path((org, id)): Path<(Uuid, Uuid)>,
            h: HeaderMap,
            Json(i): Json<$input>,
        ) -> Result<Json<Value>> {
            let a = auth::actor(&s.pool, &h, &s.config, true).await?;
            Ok(Json($f(&s, &a, org, id, i).await?))
        }
    };
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

async fn h_get_profile(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(get_profile(&s, &a).await?))
}
async fn h_put_profile(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ProfileInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(put_profile(&s, &a, i).await?))
}
org_get!(h_get_account, get_payout_account);
org_body!(h_put_account, put_payout_account, PayoutAccountInput);
org_get!(h_inquiries, list_inquiries);
org_body!(h_create_inquiry, create_inquiry, InquiryInput);
id_get!(h_inquiry, get_inquiry);
id_body!(h_inquiry_message, add_message, MessageInput);
id_post!(h_inquiry_close, close_inquiry);
org_get!(h_notifications, list_notifications);
org_body!(h_read_notifications, read_notifications, ReadInput);
org_get!(h_documents, list_documents);
org_body!(h_create_document, create_document, DocumentInput);
id_post!(h_document_check, check_document);
id_body!(h_document_sign, sign_document, SignInput);
id_body!(h_document_proof, attach_proof, ProofInput);
id_get!(h_application, get_application);
id_body!(h_record_application, record_application, ApplicationInput);
org_get!(h_finance_summary, finance_summary);
org_get!(h_statements, statements);
org_get!(h_payouts, list_payouts);
org_body!(h_request_payout, request_payout, PayoutRequestInput);
org_get!(h_reports, reports);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/me/profile", get(h_get_profile).put(h_put_profile))
        .route(
            "/api/orgs/{org}/payout-account",
            get(h_get_account).put(h_put_account),
        )
        .route(
            "/api/orgs/{org}/inquiries",
            get(h_inquiries).post(h_create_inquiry),
        )
        .route("/api/orgs/{org}/inquiries/{id}", get(h_inquiry))
        .route(
            "/api/orgs/{org}/inquiries/{id}/messages",
            post(h_inquiry_message),
        )
        .route(
            "/api/orgs/{org}/inquiries/{id}/close",
            post(h_inquiry_close),
        )
        .route("/api/orgs/{org}/notifications", get(h_notifications))
        .route(
            "/api/orgs/{org}/notifications/read",
            post(h_read_notifications),
        )
        .route(
            "/api/orgs/{org}/documents",
            get(h_documents).post(h_create_document),
        )
        .route(
            "/api/orgs/{org}/documents/{id}/check",
            post(h_document_check),
        )
        .route("/api/orgs/{org}/documents/{id}/sign", post(h_document_sign))
        .route(
            "/api/orgs/{org}/documents/{id}/proof",
            post(h_document_proof),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/application",
            get(h_application).post(h_record_application),
        )
        .route("/api/orgs/{org}/finance/summary", get(h_finance_summary))
        .route("/api/orgs/{org}/finance/statements", get(h_statements))
        .route(
            "/api/orgs/{org}/finance/payouts",
            get(h_payouts).post(h_request_payout),
        )
        .route("/api/orgs/{org}/reports", get(h_reports))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_numbers() {
        assert!(application_no_ok("AUD-20260926-ABCDEF"));
        assert!(application_no_ok("AUD-20260926-A2Z9QX"));
        assert!(!application_no_ok("AQ-20260926-ABCDEF"));
        assert!(!application_no_ok("AUD-2026092-ABCDEF"));
        assert!(!application_no_ok("AUD-20260926-ABCDE1"));
    }

    #[test]
    fn won_formatting() {
        assert_eq!(won(Decimal::from(30000)), "₩30,000");
        assert_eq!(won(Decimal::from(1234567)), "₩1,234,567");
        assert_eq!(won(Decimal::from(999)), "₩999");
    }

    #[test]
    fn signatures() {
        assert!(signature("data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==").is_ok());
        assert!(signature("data:image/jpeg;base64,iVBORw0KGgoAAAANSUhEUg==").is_err());
        assert!(signature("data:image/png;base64,<script>alert(1)</script>").is_err());
    }

    #[test]
    fn account_seal_round_trip_is_bound_to_org() {
        // SAFETY: single-threaded test setup before any other access to the variable.
        unsafe { std::env::set_var("PAYOUT_ACCOUNT_KEY", "11".repeat(32)) };
        let org = Uuid::new_v4();
        let sealed = seal_account(org, "11012345678901").unwrap();
        assert_eq!(open_account(org, &sealed).unwrap(), "11012345678901");
        assert!(open_account(Uuid::new_v4(), &sealed).is_err());
        assert!(!sealed.windows(4).any(|w| w == b"5678"));
    }
}
