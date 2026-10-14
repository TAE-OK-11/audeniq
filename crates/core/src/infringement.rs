//! Rights-infringement claims (migration 0114, LEGAL_AUDIT D12·E12).
//!
//! 1. Anyone files a claim without an account (`POST /api/infringement-claims`):
//!    who they are (sealed at once), which right, their work, the infringing
//!    release, evidence links and a good-faith statement. They get a receipt
//!    number; staff contact them by the details given.
//! 2. Staff with the delivery duty review it. Reject: nothing reaches the
//!    artist. Suspend: the claim is tied to a release and a takedown goes to
//!    every platform its latest package reached (the same `delivery.takedown`
//!    jobs as an artist takedown, [`crate::takedown`]); the artist is told and
//!    may answer (counter-notice) within [`COUNTER_DAYS`] days.
//! 3. Staff close it: restored (the claim is not upheld; staff re-send the
//!    release from the delivery screens) or upheld (it stays down).
//!
//! The artist sees the claim's right, work, target and staff notes, never
//! the claimant's contact details.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    payout_keys::Field,
    staff::{self, Duty},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

/// Days the artist has to answer a suspension.
pub const COUNTER_DAYS: i32 = 30;

const RIGHTS: &[&str] = &[
    "COPYRIGHT",
    "NEIGHBORING",
    "TRADEMARK",
    "PUBLICITY",
    "OTHER",
];

/// Receipt number shown to the claimant: `IC-` and the first 8 hex digits.
pub fn receipt(id: Uuid) -> String {
    format!("IC-{}", id.simple().to_string()[..8].to_uppercase())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimInput {
    pub claimant_name: String,
    pub claimant_email: String,
    #[serde(default)]
    pub claimant_phone: String,
    pub claimant_role: String,
    pub right_type: String,
    pub work: String,
    pub target: String,
    #[serde(default)]
    pub evidence_urls: Vec<String>,
    pub statement_accepted: bool,
}

fn invalid() -> Error {
    Error::InvalidCode("CLAIM_INVALID")
}

fn text(v: &str, max: usize) -> Result<String> {
    let v = v.trim();
    if v.is_empty() || v.chars().count() > max {
        return Err(invalid());
    }
    crate::text_policy::check_multiline(v)?;
    Ok(v.to_string())
}

fn email(v: &str) -> Result<String> {
    let v = v.trim().to_lowercase();
    let ok = v.len() <= 254
        && !v.chars().any(|c| c.is_whitespace() || c.is_control())
        && v.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty() && d.contains('.') && !d.starts_with('.') && !d.ends_with('.')
        });
    if ok { Ok(v) } else { Err(invalid()) }
}

fn phone(v: &str) -> Result<Option<String>> {
    let v = v.trim();
    if v.is_empty() {
        return Ok(None);
    }
    if v.len() > 20
        || !v
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')'))
    {
        return Err(invalid());
    }
    Ok(Some(v.to_string()))
}

fn evidence(urls: &[String]) -> Result<Vec<String>> {
    if urls.len() > 5 {
        return Err(invalid());
    }
    urls.iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .map(|u| {
            let parsed = url::Url::parse(u).map_err(|_| invalid())?;
            if u.len() > 500
                || !matches!(parsed.scheme(), "https" | "http")
                || parsed.host_str().is_none()
            {
                return Err(invalid());
            }
            Ok(parsed.to_string())
        })
        .collect()
}

/// File a claim (no account needed).
pub async fn file(s: &AppState, h: &HeaderMap, i: ClaimInput) -> Result<Value> {
    if !i.statement_accepted {
        return Err(Error::InvalidCode("CLAIM_STATEMENT_REQUIRED"));
    }
    let name = text(&i.claimant_name, 100)?;
    let mail = email(&i.claimant_email)?;
    let tel = phone(&i.claimant_phone)?;
    if !matches!(i.claimant_role.as_str(), "RIGHTS_HOLDER" | "AGENT")
        || !RIGHTS.contains(&i.right_type.as_str())
    {
        return Err(invalid());
    }
    let work = text(&i.work, 2000)?;
    let target = text(&i.target, 2000)?;
    let urls = evidence(&i.evidence_urls)?;
    // Per source address and overall, so the form cannot flood the queue.
    let ip = h
        .get(auth::CLIENT_IP_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    auth::rate(&s.pool, &format!("infringement-claim:{ip}"), 5).await?;
    auth::rate(&s.pool, &format!("infringement-claim-mail:{mail}"), 5).await?;
    auth::rate(&s.pool, "infringement-claim:all", 300).await?;
    let id = Uuid::new_v4();
    let keys = &s.payout_keys;
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "INSERT INTO portal.infringement_claims(id, claimant_name_cipher, claimant_email_cipher, claimant_phone_cipher,
            claimant_role, right_type, work, target, evidence_urls) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(keys.seal_field(Field::ClaimantName, id, &name)?)
    .bind(keys.seal_field(Field::ClaimantEmail, id, &mail)?)
    .bind(tel.map(|t| keys.seal_field(Field::ClaimantPhone, id, &t)).transpose()?)
    .bind(&i.claimant_role)
    .bind(&i.right_type)
    .bind(&work)
    .bind(&target)
    .bind(json!(urls))
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        None,
        None,
        Some(id),
        "infringement.claim_filed",
        &i.right_type,
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "receipt": receipt(id), "status": "RECEIVED"}))
}

// ---------------------------------------------------------------------------
// Staff
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
pub struct ListQuery {
    /// `open` (default): RECEIVED, SUSPENDED and COUNTER_NOTICED. `all`.
    pub status: Option<String>,
}

fn claim_json(r: &sqlx::postgres::PgRow) -> Value {
    let id: Uuid = r.get("id");
    json!({
        "id": id, "receipt": receipt(id), "status": r.get::<String,_>("status"),
        "right_type": r.get::<String,_>("right_type"), "work": r.get::<String,_>("work"),
        "target": r.get::<String,_>("target"), "evidence_urls": r.get::<Value,_>("evidence_urls"),
        "org_id": r.get::<Option<Uuid>,_>("org_id"), "release_id": r.get::<Option<Uuid>,_>("release_id"),
        "release_title": r.get::<Option<String>,_>("release_title"),
        "staff_note": r.get::<String,_>("staff_note"),
        "decided_at": r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("decided_at"),
        "counter_deadline": r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("counter_deadline"),
        "counter_notice": r.get::<Option<String>,_>("counter_notice"),
        "counter_at": r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("counter_at"),
        "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
    })
}

const CLAIM_COLUMNS: &str = "c.id, c.status, c.right_type, c.work, c.target, c.evidence_urls, c.org_id, c.release_id,
    r.title AS release_title, c.staff_note, c.decided_at, c.counter_deadline, c.counter_notice, c.counter_at, c.created_at";

/// Staff queue with the claimant's contact details (staff contact them).
pub async fn staff_list(s: &AppState, h: &HeaderMap, q: ListQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let all = q.status.as_deref() == Some("all");
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {CLAIM_COLUMNS}, o.name AS org_name, c.claimant_role,
                c.claimant_name_cipher, c.claimant_email_cipher, c.claimant_phone_cipher
           FROM portal.infringement_claims c
           LEFT JOIN catalog.releases r ON r.id=c.release_id
           LEFT JOIN identity.orgs o ON o.id=c.org_id
          WHERE $1 OR c.status IN ('RECEIVED','SUSPENDED','COUNTER_NOTICED')
          ORDER BY (c.status IN ('RECEIVED','COUNTER_NOTICED')) DESC, c.created_at DESC LIMIT 200"
    )))
    .bind(all)
    .fetch_all(&mut *tx)
    .await?;
    let keys = &s.payout_keys;
    let mut items = Vec::with_capacity(rows.len());
    for r in &rows {
        let id: Uuid = r.get("id");
        let open = |f: Field, col: &str| -> Result<Option<String>> {
            r.get::<Option<Vec<u8>>, _>(col)
                .map(|c| keys.open_field(f, id, &c).map(|v| v.to_string()))
                .transpose()
        };
        let mut v = claim_json(r);
        v["org_name"] = json!(r.get::<Option<String>, _>("org_name"));
        v["claimant"] = json!({
            "role": r.get::<String,_>("claimant_role"),
            "name": open(Field::ClaimantName, "claimant_name_cipher")?,
            "email": open(Field::ClaimantEmail, "claimant_email_cipher")?,
            "phone": open(Field::ClaimantPhone, "claimant_phone_cipher")?,
        });
        if let (Some(org), Some(release)) = (
            r.get::<Option<Uuid>, _>("org_id"),
            r.get::<Option<Uuid>, _>("release_id"),
        ) {
            v["platforms"] = crate::takedown::platform_states(&mut tx, org, release).await?;
        }
        items.push(v);
    }
    tx.commit().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInput {
    #[serde(default)]
    pub note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuspendInput {
    pub release_id: Uuid,
    #[serde(default)]
    pub note: String,
}

async fn decide(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    from: &[&str],
    to: &str,
    note: &str,
    note_required: bool,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    staff::note_ok(note, 1000)?;
    if note_required && note.trim().is_empty() {
        return Err(Error::InvalidCode("CLAIM_NOTE_REQUIRED"));
    }
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let from: Vec<String> = from.iter().map(|v| v.to_string()).collect();
    let row = sqlx::query(
        "UPDATE portal.infringement_claims SET status=$2, staff_note=$3, decided_by=$4, decided_at=now(), updated_at=now()
          WHERE id=$1 AND status = ANY($5) RETURNING org_id, release_id",
    )
    .bind(id)
    .bind(to)
    .bind(note.trim())
    .bind(st.actor.user)
    .bind(&from)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        row.get("org_id"),
        Some(id),
        &format!("staff.infringement_{}", to.to_lowercase()),
        to,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": to}))
}

/// Not actionable: closed without touching any release.
pub async fn reject(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionInput) -> Result<Value> {
    decide(s, h, id, &["RECEIVED"], "REJECTED", &i.note, true).await
}

/// Tie the claim to a release and take it down from every platform.
pub async fn suspend(s: &AppState, h: &HeaderMap, id: Uuid, i: SuspendInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    staff::note_ok(&i.note, 1000)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM catalog.releases WHERE id=$1")
        .bind(i.release_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    let updated = sqlx::query(
        "UPDATE portal.infringement_claims
            SET status='SUSPENDED', org_id=$2, release_id=$3, staff_note=$4, decided_by=$5, decided_at=now(),
                counter_deadline=now() + make_interval(days => $6), updated_at=now()
          WHERE id=$1 AND status='RECEIVED'",
    )
    .bind(id)
    .bind(org)
    .bind(i.release_id)
    .bind(i.note.trim())
    .bind(st.actor.user)
    .bind(COUNTER_DAYS)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(Error::Conflict);
    }
    let queued =
        crate::takedown::queue_takedowns(&mut tx, org, i.release_id, &format!(":claim:{id}"))
            .await?;
    let (jobs, platforms) = queued
        .map(|(_, j, p)| (j, p))
        .unwrap_or_else(|| (vec![], json!([])));
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(id),
        "staff.infringement_suspended",
        &format!("{} platform(s)", jobs.len()),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"id": id, "status": "SUSPENDED", "release_id": i.release_id, "jobs": jobs, "platforms": platforms}),
    )
}

/// Claim not upheld: the artist is told; staff re-send from the delivery screens.
pub async fn restore(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionInput) -> Result<Value> {
    decide(
        s,
        h,
        id,
        &["SUSPENDED", "COUNTER_NOTICED"],
        "RESTORED",
        &i.note,
        false,
    )
    .await
}

/// Claim upheld: the release stays down.
pub async fn uphold(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionInput) -> Result<Value> {
    decide(
        s,
        h,
        id,
        &["SUSPENDED", "COUNTER_NOTICED"],
        "UPHELD",
        &i.note,
        true,
    )
    .await
}

// ---------------------------------------------------------------------------
// Artist
// ---------------------------------------------------------------------------
/// Claims against one release (no claimant details).
pub async fn for_release(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {CLAIM_COLUMNS}, (c.status='SUSPENDED' AND c.counter_deadline > now()) AS can_answer
           FROM portal.infringement_claims c LEFT JOIN catalog.releases r ON r.id=c.release_id
          WHERE c.org_id=$1 AND c.release_id=$2 ORDER BY c.created_at DESC"
    )))
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut v = claim_json(r);
            v["can_answer"] = json!(r.get::<bool, _>("can_answer"));
            v
        })
        .collect();
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CounterInput {
    pub statement: String,
}

/// The artist's answer to a suspension, within the deadline.
pub async fn counter(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    id: Uuid,
    i: CounterInput,
) -> Result<Value> {
    let statement = i.statement.trim();
    if statement.is_empty() || statement.chars().count() > 3000 {
        return Err(Error::InvalidCode("CLAIM_COUNTER_INVALID"));
    }
    crate::text_policy::check_multiline(statement)?;
    let mut tx = s.pool.begin().await?;
    let release: Uuid = sqlx::query_scalar(
        "SELECT release_id FROM portal.infringement_claims WHERE id=$1 AND org_id=$2",
    )
    .bind(id)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?
    .flatten()
    .ok_or(Error::NotFound)?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let n = sqlx::query(
        "UPDATE portal.infringement_claims SET status='COUNTER_NOTICED', counter_notice=$3, counter_by=$4, counter_at=now(), updated_at=now()
          WHERE id=$1 AND org_id=$2 AND status='SUSPENDED' AND counter_deadline > now()",
    )
    .bind(id)
    .bind(org)
    .bind(statement)
    .bind(a.user)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("CLAIM_COUNTER_CLOSED"));
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "release.infringement_counter_notice",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "COUNTER_NOTICED"}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_file(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ClaimInput>,
) -> Result<Json<Value>> {
    Ok(Json(file(&s, &h, i).await?))
}
async fn h_staff_list(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>> {
    Ok(Json(staff_list(&s, &h, q).await?))
}
async fn h_reject(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    Ok(Json(reject(&s, &h, id, i).await?))
}
async fn h_suspend(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<SuspendInput>,
) -> Result<Json<Value>> {
    Ok(Json(suspend(&s, &h, id, i).await?))
}
async fn h_restore(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    Ok(Json(restore(&s, &h, id, i).await?))
}
async fn h_uphold(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    Ok(Json(uphold(&s, &h, id, i).await?))
}
async fn h_for_release(
    State(s): State<AppState>,
    Path((org, release)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(for_release(&s, &a, org, release).await?))
}
async fn h_counter(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<CounterInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(counter(&s, &a, org, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/infringement-claims", post(h_file))
        .route("/api/staff/infringement-claims", get(h_staff_list))
        .route("/api/staff/infringement-claims/{id}/reject", post(h_reject))
        .route(
            "/api/staff/infringement-claims/{id}/suspend",
            post(h_suspend),
        )
        .route(
            "/api/staff/infringement-claims/{id}/restore",
            post(h_restore),
        )
        .route("/api/staff/infringement-claims/{id}/uphold", post(h_uphold))
        .route(
            "/api/orgs/{org}/releases/{id}/infringement-claims",
            get(h_for_release),
        )
        .route(
            "/api/orgs/{org}/infringement-claims/{id}/counter",
            post(h_counter),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_checks() {
        assert!(email("Owner@Label.co.kr").is_ok_and(|e| e == "owner@label.co.kr"));
        for bad in ["", "a@b", "a b@c.d", "@c.d", "a@.d"] {
            assert!(email(bad).is_err(), "{bad}");
        }
        assert_eq!(phone("").unwrap(), None);
        assert!(phone("010-1234-5678").is_ok() && phone("010-ABCD").is_err());
        assert!(evidence(&["https://example.com/a".into()]).is_ok());
        assert!(evidence(&["javascript:alert(1)".into()]).is_err());
        assert!(evidence(&vec!["https://e.com".into(); 6]).is_err());
        assert_eq!(receipt(Uuid::nil()), "IC-00000000");
    }
}
