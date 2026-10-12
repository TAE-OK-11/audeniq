//! Printout provenance: every print / PDF of a terms page, release
//! application, agreement, rights document, signing document or staff review
//! sheet carries two codes on each page.
//!
//! - Document code (`AQ-AGR-7F3KQ2M9XA`): the same document with the same
//!   content always gets the same code.
//! - Verification code (`K7Q2M-9XAB3-...`, 100 random bits): new for every
//!   print. Only its SHA-256 is stored (one-way). The record says who printed
//!   (account, staff member or signing-link recipient), when, from which
//!   session and browser-generated device id, and from where (IP and browser,
//!   sealed with the personal-data keyring).
//!
//! Anyone holding the paper can check the verification code at `/verify`
//! (a masked summary); staff see the full record. Records cannot be changed.
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    payout_keys::Field,
};
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

/// Crockford base32: no I, L, O, U, so codes read and type cleanly.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn base32(bytes: &[u8], chars: usize) -> String {
    let mut out = String::with_capacity(chars);
    let (mut buf, mut bits) = (0u64, 0u32);
    for &b in bytes {
        if out.len() == chars {
            break;
        }
        buf = (buf << 8) | u64::from(b);
        bits += 8;
        while bits >= 5 && out.len() < chars {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 31) as usize] as char);
        }
        // Keep only the bits not yet written (at most 4).
        buf &= (1 << bits) - 1;
    }
    out
}

/// What a person types: case, dashes, spaces and look-alike letters forgiven.
pub fn normalize_code(input: &str) -> Option<String> {
    let s: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect();
    (s.len() == 20 && s.bytes().all(|b| ALPHABET.contains(&b))).then_some(s)
}

fn grouped(code: &str) -> String {
    code.as_bytes()
        .chunks(5)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("-")
}

fn verification_hash(normalized: &str) -> Vec<u8> {
    Sha256::digest(normalized.as_bytes()).to_vec()
}

pub fn document_code(tag: &str, kind: &str, doc_ref: &str, content_hash: Option<&str>) -> String {
    let digest = Sha256::digest(format!("{kind}|{doc_ref}|{}", content_hash.unwrap_or("")));
    format!("AQ-{tag}-{}", base32(&digest, 10))
}

/// `h***@g****.com`
fn mask_email(email: &str) -> String {
    let (local, domain) = email.split_once('@').unwrap_or((email, ""));
    let first = |s: &str| s.chars().next().map(String::from).unwrap_or_default();
    let (host, tld) = domain.rsplit_once('.').unwrap_or((domain, ""));
    format!(
        "{}***@{}****{}",
        first(local),
        first(host),
        if tld.is_empty() {
            String::new()
        } else {
            format!(".{tld}")
        }
    )
}

fn mask_name(name: &str) -> String {
    let mut c = name.trim().chars();
    match c.next() {
        Some(f) => format!("{f}{}", "*".repeat(c.count().clamp(1, 4))),
        None => "*".into(),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StampInput {
    /// TERMS | SETTLEMENT_TERMS | PRIVACY | APPLICATION | DOCUMENT | SIGNING
    pub kind: String,
    /// Terms version, release id, document id, or the signing link token.
    #[serde(rename = "ref")]
    pub doc_ref: String,
    /// Shown for terms pages only; other titles come from the record.
    pub title: Option<String>,
    /// SHA-256 (hex) of the printed document text.
    pub content_hash: Option<String>,
    /// Random id the browser keeps for this device.
    pub device_id: Option<Uuid>,
}

/// Who printed and what, resolved and access-checked on the server.
struct Subject {
    kind: &'static str,
    tag: &'static str,
    doc_ref: String,
    title: String,
    org: Option<Uuid>,
    user: Option<Uuid>,
    signing: Option<Uuid>,
    staff: bool,
    session_hash: Option<Vec<u8>>,
    printed_by: String,
}

fn clean_hash(h: Option<&str>) -> Result<Option<String>> {
    match h {
        None => Ok(None),
        Some(h) if h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Ok(Some(h.to_ascii_lowercase()))
        }
        Some(_) => Err(Error::Invalid),
    }
}

async fn printed_by_user(s: &AppState, user: Uuid, staff: bool) -> Result<String> {
    let email = crate::pii::user_email(&s.payout_keys, &s.pool, user).await?;
    Ok(if staff {
        format!("AUDENIQ 직원 {}", mask_email(&email))
    } else {
        mask_email(&email)
    })
}

async fn subject(s: &AppState, h: &HeaderMap, i: &StampInput) -> Result<Subject> {
    let public = |max: usize| -> Result<(String, String)> {
        let v = i.doc_ref.trim();
        let title = i.title.as_deref().map(str::trim).unwrap_or("").to_owned();
        if v.is_empty()
            || v.len() > max
            || !v
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" ._-".contains(&b))
            || title.is_empty()
            || title.chars().count() > 200
        {
            return Err(Error::Invalid);
        }
        crate::text_policy::check(&title)?;
        Ok((v.to_owned(), title))
    };
    match i.kind.as_str() {
        k @ ("TERMS" | "SETTLEMENT_TERMS" | "PRIVACY") => {
            let (kind, tag) = match k {
                "TERMS" => ("TERMS", "TRM"),
                "SETTLEMENT_TERMS" => ("SETTLEMENT_TERMS", "STL"),
                _ => ("PRIVACY", "PRV"),
            };
            let (doc_ref, title) = public(80)?;
            // Public pages: signed in or not, recorded either way.
            let actor = auth::actor(&s.pool, h, &s.config, true).await.ok();
            let printed_by = match &actor {
                Some(a) => printed_by_user(s, a.user, false).await?,
                None => "로그인하지 않은 방문자".into(),
            };
            Ok(Subject {
                kind,
                tag,
                doc_ref,
                title,
                org: None,
                user: actor.as_ref().map(|a| a.user),
                signing: None,
                staff: false,
                session_hash: actor.map(|a| a.session_hash),
                printed_by,
            })
        }
        "APPLICATION" | "DOCUMENT" => {
            let a = auth::actor(&s.pool, h, &s.config, true).await?;
            let id = Uuid::parse_str(i.doc_ref.trim()).map_err(|_| Error::Invalid)?;
            let row = if i.kind == "APPLICATION" {
                sqlx::query(
                    "SELECT org_id, title || ' 발매 신청서' AS title, 'APP' AS tag FROM catalog.releases WHERE id=$1",
                )
            } else {
                sqlx::query(
                    "SELECT org_id, title, CASE kind WHEN 'AGREEMENT' THEN 'AGR' ELSE 'RTS' END AS tag
                       FROM portal.documents WHERE id=$1",
                )
            }
            .bind(id)
            .fetch_optional(&s.pool)
            .await?
            .ok_or(Error::NotFound)?;
            let org: Uuid = row.get("org_id");
            let mut c = s.pool.acquire().await?;
            auth::membership(&mut c, &a, org, false)
                .await
                .map_err(|_| Error::NotFound)?;
            let tag = match row.get::<&str, _>("tag") {
                "APP" => "APP",
                "AGR" => "AGR",
                _ => "RTS",
            };
            Ok(Subject {
                kind: if i.kind == "APPLICATION" {
                    "APPLICATION"
                } else {
                    "DOCUMENT"
                },
                tag,
                doc_ref: id.to_string(),
                title: row.get("title"),
                org: Some(org),
                user: Some(a.user),
                signing: None,
                staff: false,
                session_hash: Some(a.session_hash),
                printed_by: printed_by_user(s, a.user, false).await?,
            })
        }
        "SIGNING" => {
            // The signing link holder (often without an account).
            auth::origin(h, &s.config)?;
            let token = i.doc_ref.trim();
            if token.len() != 64 {
                return Err(Error::NotFound);
            }
            let row = sqlx::query(
                "SELECT id, org_id, title, signer_name FROM portal.signing_requests WHERE token_hash=$1",
            )
            .bind(auth::hash_token(token))
            .fetch_optional(&s.pool)
            .await?
            .ok_or(Error::NotFound)?;
            let id: Uuid = row.get("id");
            Ok(Subject {
                kind: "SIGNING",
                tag: "SGN",
                // The token never leaves the request: the record names the signing request.
                doc_ref: id.to_string(),
                title: row.get("title"),
                org: Some(row.get("org_id")),
                user: None,
                signing: Some(id),
                staff: false,
                session_hash: None,
                printed_by: format!(
                    "서명 요청 수신자 {}",
                    mask_name(row.get::<Option<&str>, _>("signer_name").unwrap_or(""))
                ),
            })
        }
        _ => Err(Error::Invalid),
    }
}

async fn record(s: &AppState, h: &HeaderMap, sub: Subject, i: &StampInput) -> Result<Value> {
    let content_hash = clean_hash(i.content_hash.as_deref())?;
    let mut raw = [0u8; 13];
    {
        use rand::{TryRng, rngs::SysRng};
        SysRng
            .try_fill_bytes(&mut raw)
            .map_err(|_| Error::Internal)?;
    }
    let code = base32(&raw, 20);
    let doc_code = document_code(sub.tag, sub.kind, &sub.doc_ref, content_hash.as_deref());
    let id = Uuid::new_v4();
    let source = json!({
        "ip": auth::client_ip_key(h),
        "user_agent": h.get("user-agent").and_then(|v| v.to_str().ok()).map(|v| v.chars().take(300).collect::<String>()),
    })
    .to_string();
    let sealed = if s.payout_keys.ready() {
        Some(s.payout_keys.seal_field(Field::PrintSource, id, &source)?)
    } else {
        None
    };
    let session_id: Option<Uuid> = match &sub.session_hash {
        Some(hash) => {
            sqlx::query_scalar("SELECT id FROM identity.sessions WHERE token_hash=$1")
                .bind(hash)
                .fetch_optional(&s.pool)
                .await?
        }
        None => None,
    };
    let printed_at: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "INSERT INTO operations.print_stamps
           (id,verification_hash,document_code,doc_kind,doc_ref,title,content_hash,org_id,user_id,
            signing_request_id,staff,session_id,device_id,source_cipher)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING printed_at",
    )
    .bind(id)
    .bind(verification_hash(&code))
    .bind(&doc_code)
    .bind(sub.kind)
    .bind(&sub.doc_ref)
    .bind(&sub.title)
    .bind(&content_hash)
    .bind(sub.org)
    .bind(sub.user)
    .bind(sub.signing)
    .bind(sub.staff)
    .bind(session_id)
    .bind(i.device_id)
    .bind(sealed)
    .fetch_one(&s.pool)
    .await?;
    Ok(json!({
        "document_code": doc_code,
        "verification_code": grouped(&code),
        "printed_at": printed_at,
        "printed_by": sub.printed_by,
        "title": sub.title,
        "verify_url": format!("{}/verify", s.config.origin),
    }))
}

fn source_rate_key(h: &HeaderMap, what: &str) -> (String, i32) {
    match auth::client_ip_key(h) {
        Some(ip) => (format!("{what}:src:{ip}"), 1),
        None => (format!("{what}:src:unknown"), auth::UNKNOWN_SOURCE_FACTOR),
    }
}

/// Studio: stamp a printout of a page the caller may see.
pub async fn stamp(s: &AppState, h: &HeaderMap, i: StampInput) -> Result<Value> {
    let (key, factor) = source_rate_key(h, "print-stamp");
    auth::rate(&s.pool, &key, 120 * factor).await?;
    let sub = subject(s, h, &i).await?;
    record(s, h, sub, &i).await
}

/// Staff console: stamp a printout of a release review sheet.
pub async fn stamp_staff(s: &AppState, h: &HeaderMap, i: StampInput) -> Result<Value> {
    let st = crate::staff::staff(s, h, true).await?;
    if i.kind != "STAFF_SHEET" {
        return Err(Error::Invalid);
    }
    let id = Uuid::parse_str(i.doc_ref.trim()).map_err(|_| Error::Invalid)?;
    let row = sqlx::query("SELECT org_id, title FROM catalog.releases WHERE id=$1")
        .bind(id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or(Error::NotFound)?;
    let sub = Subject {
        kind: "STAFF_SHEET",
        tag: "SUB",
        doc_ref: id.to_string(),
        title: format!("{} 심사 기록", row.get::<String, _>("title")),
        org: Some(row.get("org_id")),
        user: Some(st.actor.user),
        signing: None,
        staff: true,
        session_hash: Some(st.actor.session_hash.clone()),
        printed_by: printed_by_user(s, st.actor.user, true).await?,
    };
    record(s, h, sub, &i).await
}

#[derive(Deserialize)]
pub struct VerifyQuery {
    pub code: String,
}

async fn find(s: &AppState, code: &str) -> Result<Option<sqlx::postgres::PgRow>> {
    let Some(normalized) = normalize_code(code) else {
        return Ok(None);
    };
    Ok(sqlx::query(
        "SELECT p.*, sr.signer_name FROM operations.print_stamps p
           LEFT JOIN portal.signing_requests sr ON sr.id=p.signing_request_id
          WHERE p.verification_hash=$1",
    )
    .bind(verification_hash(&normalized))
    .fetch_optional(&s.pool)
    .await?)
}

async fn who(s: &AppState, r: &sqlx::postgres::PgRow, full: bool) -> Result<String> {
    if let Some(user) = r.get::<Option<Uuid>, _>("user_id") {
        let email = crate::pii::user_email(&s.payout_keys, &s.pool, user).await?;
        let shown = if full { email } else { mask_email(&email) };
        return Ok(if r.get("staff") {
            format!("AUDENIQ 직원 {shown}")
        } else {
            shown
        });
    }
    if r.get::<Option<Uuid>, _>("signing_request_id").is_some() {
        let name = r
            .get::<Option<String>, _>("signer_name")
            .unwrap_or_default();
        return Ok(format!(
            "서명 요청 수신자 {}",
            if full { name } else { mask_name(&name) }
        ));
    }
    Ok("로그인하지 않은 방문자".into())
}

/// Public: is this printout genuine? A masked summary, never the source.
pub async fn verify(s: &AppState, h: &HeaderMap, q: VerifyQuery) -> Result<Value> {
    let (key, factor) = source_rate_key(h, "print-verify");
    auth::rate(&s.pool, &key, 30 * factor).await?;
    let Some(r) = find(s, &q.code).await? else {
        return Ok(json!({"valid": false}));
    };
    Ok(json!({
        "valid": true,
        "document_code": r.get::<String, _>("document_code"),
        "title": r.get::<String, _>("title"),
        "kind": r.get::<String, _>("doc_kind"),
        "printed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("printed_at"),
        "printed_by": who(s, &r, false).await?,
        "content_hash": r.get::<Option<String>, _>("content_hash"),
    }))
}

/// Staff: the full record behind a verification code (who, session, device,
/// IP and browser). The lookup itself is in the staff access log.
pub async fn verify_staff(s: &AppState, h: &HeaderMap, q: VerifyQuery) -> Result<Value> {
    crate::staff::staff(s, h, false).await?;
    let Some(r) = find(s, &q.code).await? else {
        return Ok(json!({"valid": false}));
    };
    let id: Uuid = r.get("id");
    let source: Value = match r.get::<Option<Vec<u8>>, _>("source_cipher") {
        Some(c) => serde_json::from_str(&s.payout_keys.open_field(Field::PrintSource, id, &c)?)
            .unwrap_or(Value::Null),
        None => Value::Null,
    };
    let document_code: String = r.get("document_code");
    let prints: i64 =
        sqlx::query_scalar("SELECT count(*) FROM operations.print_stamps WHERE document_code=$1")
            .bind(&document_code)
            .fetch_one(&s.pool)
            .await?;
    Ok(json!({
        "valid": true,
        "id": id,
        "document_code": document_code,
        "title": r.get::<String, _>("title"),
        "kind": r.get::<String, _>("doc_kind"),
        "doc_ref": r.get::<String, _>("doc_ref"),
        "content_hash": r.get::<Option<String>, _>("content_hash"),
        "printed_at": r.get::<chrono::DateTime<chrono::Utc>, _>("printed_at"),
        "printed_by": who(s, &r, true).await?,
        "user_id": r.get::<Option<Uuid>, _>("user_id"),
        "org_id": r.get::<Option<Uuid>, _>("org_id"),
        "signing_request_id": r.get::<Option<Uuid>, _>("signing_request_id"),
        "session_id": r.get::<Option<Uuid>, _>("session_id"),
        "device_id": r.get::<Option<Uuid>, _>("device_id"),
        "source": source,
        "prints_of_this_document": prints,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_readable_and_forgiving() {
        let code = base32(&[0xff; 13], 20);
        assert_eq!(code.len(), 20);
        let g = grouped(&code);
        assert_eq!(g.len(), 23);
        assert_eq!(
            normalize_code(&g.to_lowercase()).as_deref(),
            Some(code.as_str())
        );
        // Look-alike letters read as digits: O→0, I/L→1.
        assert_eq!(
            normalize_code("o1l2i-34567-890ab-cdefg").as_deref(),
            Some("0112134567890ABCDEFG")
        );
        assert!(normalize_code("short").is_none());
        assert!(
            normalize_code("UUUUU-UUUUU-UUUUU-UUUUU").is_none(),
            "U is not in the alphabet"
        );
    }

    #[test]
    fn the_same_content_keeps_its_document_code() {
        let a = document_code("AGR", "DOCUMENT", "x", Some("ab"));
        assert_eq!(a, document_code("AGR", "DOCUMENT", "x", Some("ab")));
        assert_ne!(a, document_code("AGR", "DOCUMENT", "x", Some("ac")));
        assert!(a.starts_with("AQ-AGR-") && a.len() == 17);
    }

    #[test]
    fn public_summaries_are_masked() {
        assert_eq!(mask_email("jimin.kim@gmail.com"), "j***@g****.com");
        assert_eq!(mask_name("홍길동"), "홍**");
        assert_eq!(mask_name(""), "*");
    }
}
