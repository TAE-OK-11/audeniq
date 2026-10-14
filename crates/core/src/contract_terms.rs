//! Contract end and rights expiry (migration 0121).
//!
//! A release's distribution ends on a date, set by the artist (contract end
//! at least [`NOTICE_DAYS`] ahead, or a licensed master's rights expiring at
//! least a day ahead) or by staff (termination, any date). An org-wide
//! contract end (OWNER, at least [`NOTICE_DAYS`] ahead) schedules every
//! submitted release and stops new submissions.
//!
//! [`sweep`] (hourly in the API): [`WARN_DAYS`] before, the org is told (by
//! the notice triggers of 0121); on
//! the date (KST) every platform still carrying the release gets a takedown
//! and final settlement is due within [`FINAL_SETTLEMENT_DAYS`], which staff
//! close once the last royalties are posted. From the date on, no new
//! delivery goes out (`portal.release_term_ended`, checked when deliveries
//! are queued).
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    staff::{self, Duty},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use std::time::Duration;
use uuid::Uuid;

pub const NOTICE_DAYS: i64 = 30;
pub const WARN_DAYS: i32 = 30;
pub const FINAL_SETTLEMENT_DAYS: i32 = 90;

fn kst_today() -> NaiveDate {
    (chrono::Utc::now() + chrono::Duration::hours(9)).date_naive()
}

fn date(v: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(v.trim(), "%Y-%m-%d")
        .map_err(|_| Error::InvalidCode("TERM_DATE_INVALID"))
}

fn note(v: &str) -> Result<String> {
    let v = v.trim();
    if v.chars().count() > 1000 {
        return Err(Error::InvalidCode("TERM_NOTE_INVALID"));
    }
    if !v.is_empty() {
        crate::text_policy::check_multiline(v)?;
    }
    Ok(v.to_string())
}

const TERM_JSON: &str = "jsonb_build_object('release_id',t.release_id,'end_date',t.end_date,'reason',t.reason,'note',t.note,
    'status',t.status,'set_by_staff',t.set_by_staff,'ended_at',t.ended_at,'final_settlement_due',t.final_settlement_due,
    'final_settled_at',t.final_settled_at,'final_note',t.final_note,'created_at',t.created_at)";

async fn release_status(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<String> {
    sqlx::query_scalar(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

// ---------------------------------------------------------------------------
// Artist
// ---------------------------------------------------------------------------
pub async fn release_term(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let term: Option<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {TERM_JSON} FROM portal.release_terms t WHERE t.org_id=$1 AND t.release_id=$2 AND t.status <> 'CANCELLED'"
    )))
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"term": term, "notice_days": NOTICE_DAYS}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TermInput {
    pub end_date: String,
    /// CONTRACT_END or LICENSE_EXPIRY (artist); staff may also TERMINATION.
    pub reason: String,
    #[serde(default)]
    pub note: String,
}

#[allow(clippy::too_many_arguments)]
async fn upsert(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    end: NaiveDate,
    reason: &str,
    note: &str,
    user: Uuid,
    staff: bool,
) -> Result<()> {
    // An ended term stays: distribution cannot be revived by moving the date.
    let n = sqlx::query(
        "INSERT INTO portal.release_terms(release_id, org_id, end_date, reason, note, set_by, set_by_staff)
         VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT (release_id) DO UPDATE SET end_date=$3, reason=$4, note=$5, set_by=$6, set_by_staff=$7,
                status='SCHEDULED', warned_at=NULL, updated_at=now()
          WHERE portal.release_terms.status IN ('SCHEDULED','CANCELLED')",
    )
    .bind(release)
    .bind(org)
    .bind(end)
    .bind(reason)
    .bind(note)
    .bind(user)
    .bind(staff)
    .execute(&mut *c)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("TERM_ALREADY_ENDED"));
    }
    Ok(())
}

/// The artist schedules the end of this release's distribution.
pub async fn set_release_term(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: TermInput,
) -> Result<Value> {
    let end = date(&i.end_date)?;
    let lead = (end - kst_today()).num_days();
    match i.reason.as_str() {
        "CONTRACT_END" if lead < NOTICE_DAYS => {
            return Err(Error::PolicyGate("TERM_NOTICE_TOO_SHORT"));
        }
        "LICENSE_EXPIRY" if lead < 1 => return Err(Error::PolicyGate("TERM_DATE_PAST")),
        "CONTRACT_END" | "LICENSE_EXPIRY" => {}
        _ => return Err(Error::InvalidCode("TERM_REASON_INVALID")),
    }
    let note = note(&i.note)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    if matches!(
        release_status(&mut tx, org, release).await?.as_str(),
        "DRAFT" | "WITHDRAWN" | "SUPERSEDED"
    ) {
        return Err(Error::PolicyGate("TERM_RELEASE_NOT_SUBMITTED"));
    }
    // A staff termination or the org's contract end is not the artist's to
    // move (an org-wide end also applies on its own, see `sweep`).
    let staff_set: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.release_terms WHERE release_id=$1 AND status='SCHEDULED'
                        AND (set_by_staff OR reason='ORG_CONTRACT_END'))",
    )
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    if staff_set {
        return Err(Error::PolicyGate("TERM_SET_BY_STAFF"));
    }
    upsert(&mut tx, org, release, end, &i.reason, &note, a.user, false).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.term_scheduled",
        &i.reason,
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"release_id": release, "end_date": end, "status": "SCHEDULED"}))
}

/// Cancel a scheduled end the artist set (not a staff termination).
pub async fn cancel_release_term(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let n = sqlx::query(
        "UPDATE portal.release_terms SET status='CANCELLED', updated_at=now()
          WHERE org_id=$1 AND release_id=$2 AND status='SCHEDULED' AND NOT set_by_staff AND reason <> 'ORG_CONTRACT_END'",
    )
    .bind(org)
    .bind(release)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("TERM_NOT_CANCELLABLE"));
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.term_cancelled",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"release_id": release, "status": "CANCELLED"}))
}

pub async fn org_term(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let term: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('end_date',end_date,'note',note,'status',status,'set_by_staff',set_by_staff,'created_at',created_at)
           FROM portal.org_contract_terms WHERE org_id=$1 AND status <> 'CANCELLED'",
    )
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    let releases: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {TERM_JSON} || jsonb_build_object('title', r.title) FROM portal.release_terms t
           JOIN catalog.releases r ON r.id=t.release_id
          WHERE t.org_id=$1 AND t.status <> 'CANCELLED' ORDER BY t.end_date, r.title LIMIT 500"
    )))
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"term": term, "releases": releases, "notice_days": NOTICE_DAYS}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrgTermInput {
    pub end_date: String,
    #[serde(default)]
    pub note: String,
}

async fn schedule_org(
    c: &mut PgConnection,
    org: Uuid,
    end: NaiveDate,
    note: &str,
    user: Uuid,
    staff: bool,
) -> Result<i64> {
    sqlx::query(
        "INSERT INTO portal.org_contract_terms(org_id, end_date, note, set_by, set_by_staff) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT (org_id) DO UPDATE SET end_date=$2, note=$3, set_by=$4, set_by_staff=$5, status='SCHEDULED', updated_at=now()
          WHERE portal.org_contract_terms.status <> 'ENDED'",
    )
    .bind(org)
    .bind(end)
    .bind(note)
    .bind(user)
    .bind(staff)
    .execute(&mut *c)
    .await?;
    // Every submitted release ends with it. A release that already has its
    // own (or a staff) end keeps that row; the org end still applies to it on
    // the org's date (`sweep`, `portal.release_term_ended`), and cancelling
    // the org end leaves it untouched.
    let n = sqlx::query(
        "INSERT INTO portal.release_terms(release_id, org_id, end_date, reason, note, set_by, set_by_staff)
         SELECT r.id, r.org_id, $2, 'ORG_CONTRACT_END', $3, $4, $5 FROM catalog.releases r
          WHERE r.org_id=$1 AND r.archived_at IS NULL AND r.status NOT IN ('DRAFT','WITHDRAWN','SUPERSEDED')
         ON CONFLICT (release_id) DO UPDATE SET end_date=$2, reason='ORG_CONTRACT_END', note=$3, set_by=$4, set_by_staff=$5,
                status='SCHEDULED', warned_at=NULL, updated_at=now()
          WHERE portal.release_terms.status='CANCELLED'
             OR (portal.release_terms.status='SCHEDULED' AND portal.release_terms.reason='ORG_CONTRACT_END')",
    )
    .bind(org)
    .bind(end)
    .bind(note)
    .bind(user)
    .bind(staff)
    .execute(&mut *c)
    .await?
    .rows_affected();
    Ok(n as i64)
}

/// The OWNER ends the whole distribution contract.
pub async fn set_org_term(s: &AppState, a: &Actor, org: Uuid, i: OrgTermInput) -> Result<Value> {
    let end = date(&i.end_date)?;
    if (end - kst_today()).num_days() < NOTICE_DAYS {
        return Err(Error::PolicyGate("TERM_NOTICE_TOO_SHORT"));
    }
    let note = note(&i.note)?;
    let mut tx = s.pool.begin().await?;
    if auth::membership(&mut tx, a, org, true).await? != "OWNER" {
        return Err(Error::Forbidden);
    }
    let n = schedule_org(&mut tx, org, end, &note, a.user, false).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        None,
        "org.contract_end_scheduled",
        "OWNER",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"end_date": end, "releases": n}))
}

/// Cancel the org's contract end before the date (the release ends it
/// scheduled are cancelled with it).
pub async fn cancel_org_term(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    if auth::membership(&mut tx, a, org, true).await? != "OWNER" {
        return Err(Error::Forbidden);
    }
    let n = sqlx::query(
        "UPDATE portal.org_contract_terms SET status='CANCELLED', updated_at=now()
          WHERE org_id=$1 AND status='SCHEDULED' AND NOT set_by_staff",
    )
    .bind(org)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("TERM_NOT_CANCELLABLE"));
    }
    sqlx::query(
        "UPDATE portal.release_terms SET status='CANCELLED', updated_at=now()
          WHERE org_id=$1 AND status='SCHEDULED' AND reason='ORG_CONTRACT_END'",
    )
    .bind(org)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        None,
        "org.contract_end_cancelled",
        "OWNER",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"status": "CANCELLED"}))
}

// ---------------------------------------------------------------------------
// Sweep
// ---------------------------------------------------------------------------

/// Warn, end what is due (takedowns), close org contracts. Returns how many
/// releases ended.
pub async fn sweep(pool: &PgPool) -> Result<usize> {
    let today = kst_today();
    let mut tx = pool.begin().await?;
    // Tell the org once, WARN_DAYS before.
    sqlx::query(
        "UPDATE portal.release_terms t SET warned_at=now()
          WHERE t.status='SCHEDULED' AND t.warned_at IS NULL AND t.end_date <= $1 + $2 AND t.end_date > $1",
    )
    .bind(today)
    .bind(WARN_DAYS)
    .execute(&mut *tx)
    .await?;
    // Org-wide ends are told once for the whole contract.
    sqlx::query(
        "UPDATE portal.org_contract_terms SET warned_at=now()
          WHERE status='SCHEDULED' AND warned_at IS NULL AND end_date <= $1 + $2 AND end_date > $1",
    )
    .bind(today)
    .bind(WARN_DAYS)
    .execute(&mut *tx)
    .await?;
    let due = sqlx::query(
        "SELECT t.org_id, t.release_id FROM portal.release_terms t
          WHERE t.status='SCHEDULED'
            AND (t.end_date <= $1 OR EXISTS (SELECT 1 FROM portal.org_contract_terms o
                   WHERE o.org_id=t.org_id AND o.status='SCHEDULED' AND o.end_date <= $1))
          ORDER BY t.end_date LIMIT 200 FOR UPDATE SKIP LOCKED",
    )
    .bind(today)
    .fetch_all(&mut *tx)
    .await?;
    for d in &due {
        let org: Uuid = d.get("org_id");
        let release: Uuid = d.get("release_id");
        crate::takedown::queue_takedowns(&mut tx, org, release, &format!(":term:{today}")).await?;
        sqlx::query(
            "UPDATE portal.release_terms SET status='ENDED', ended_at=now(), final_settlement_due=$2 + $3, updated_at=now()
              WHERE release_id=$1",
        )
        .bind(release)
        .bind(today)
        .bind(FINAL_SETTLEMENT_DAYS)
        .execute(&mut *tx)
        .await?;
        operations::audit(
            &mut tx,
            None,
            Some(org),
            Some(release),
            "release.term_ended",
            "SWEEP",
            Uuid::new_v4(),
        )
        .await?;
    }
    sqlx::query("UPDATE portal.org_contract_terms SET status='ENDED', updated_at=now() WHERE status='SCHEDULED' AND end_date <= $1")
        .bind(today)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(due.len())
}

/// Hourly in the API process.
pub fn spawn(pool: PgPool) {
    tokio::spawn(async move {
        loop {
            if let Err(error) = sweep(&pool).await {
                tracing::warn!(%error, "contract term sweep failed");
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
}

// ---------------------------------------------------------------------------
// Staff
// ---------------------------------------------------------------------------
pub async fn staff_list(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {TERM_JSON} || jsonb_build_object('title',r.title,'org_name',o.name,'org_id',t.org_id)
           FROM portal.release_terms t JOIN catalog.releases r ON r.id=t.release_id JOIN identity.orgs o ON o.id=t.org_id
          WHERE t.status IN ('SCHEDULED','ENDED')
          ORDER BY (t.status='ENDED') DESC, coalesce(t.final_settlement_due, t.end_date) LIMIT 500"
    )))
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

/// Staff terminate a release (any date, today included).
pub async fn staff_terminate(
    s: &AppState,
    h: &HeaderMap,
    release: Uuid,
    i: TermInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    let end = date(&i.end_date)?;
    if !matches!(
        i.reason.as_str(),
        "TERMINATION" | "LICENSE_EXPIRY" | "CONTRACT_END"
    ) {
        return Err(Error::InvalidCode("TERM_REASON_INVALID"));
    }
    let note = note(&i.note)?;
    if i.reason == "TERMINATION" && note.is_empty() {
        return Err(Error::InvalidCode("TERM_NOTE_REQUIRED"));
    }
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let org: Uuid = sqlx::query_scalar(
        "SELECT org_id FROM catalog.releases WHERE id=$1 AND archived_at IS NULL",
    )
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    upsert(
        &mut tx,
        org,
        release,
        end,
        &i.reason,
        &note,
        st.actor.user,
        true,
    )
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "staff.term_set",
        &i.reason,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"release_id": release, "end_date": end, "status": "SCHEDULED"}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalInput {
    #[serde(default)]
    pub note: String,
}

/// Staff close the final settlement of an ended release.
pub async fn staff_final_settled(
    s: &AppState,
    h: &HeaderMap,
    release: Uuid,
    i: FinalInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    let note = note(&i.note)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let org: Uuid = sqlx::query_scalar(
        "UPDATE portal.release_terms SET status='FINAL_SETTLED', final_settled_at=now(), final_note=$2, updated_at=now()
          WHERE release_id=$1 AND status='ENDED' RETURNING org_id",
    )
    .bind(release)
    .bind(&note)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "staff.term_final_settled",
        "FINAL",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"release_id": release, "status": "FINAL_SETTLED"}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_term(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(release_term(&s, &a, org, id).await?))
}
async fn h_set_term(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<TermInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(set_release_term(&s, &a, org, id, i).await?))
}
async fn h_cancel_term(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(cancel_release_term(&s, &a, org, id).await?))
}
async fn h_org_term(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(org_term(&s, &a, org).await?))
}
async fn h_set_org_term(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<OrgTermInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(set_org_term(&s, &a, org, i).await?))
}
async fn h_cancel_org_term(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(cancel_org_term(&s, &a, org).await?))
}
async fn h_staff_list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(staff_list(&s, &h).await?))
}
async fn h_staff_terminate(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<TermInput>,
) -> Result<Json<Value>> {
    Ok(Json(staff_terminate(&s, &h, id, i).await?))
}
async fn h_staff_final(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<FinalInput>,
) -> Result<Json<Value>> {
    Ok(Json(staff_final_settled(&s, &h, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/releases/{id}/term",
            get(h_term).put(h_set_term),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/term/cancel",
            post(h_cancel_term),
        )
        .route(
            "/api/orgs/{org}/contract-end",
            get(h_org_term).put(h_set_org_term),
        )
        .route(
            "/api/orgs/{org}/contract-end/cancel",
            post(h_cancel_org_term),
        )
        .route("/api/staff/terms", get(h_staff_list))
        .route("/api/staff/releases/{id}/term", post(h_staff_terminate))
        .route(
            "/api/staff/releases/{id}/term/final-settled",
            post(h_staff_final),
        )
}
