//! AUDENIQ staff portal API (`/api/staff/*`): release review decisions,
//! second-person approvals, agreement / rights-proof review, inquiry
//! replies, delivery staging approvals and the DSP (D-n) overview.
//!
//! Staff are identified by their normal authenticated session plus an
//! ACTIVE row in `identity.staff_members` (granted only by the operator CLI).
//! Every call rechecks the role; every write is audited with the staff user.
//! Same service-secret, Origin and CSRF rules as the rest of the API.
//!
//! Review decisions never edit check results. They write the same
//! append-only `rights.review_overrides` rows the Stage 2 decision already
//! honours and queue a Stage 2 re-evaluation, so the pipeline (not this
//! module) moves the release: PASS -> Stage 3, CORRECTION -> the artist.
//! Only REJECT moves the status directly (STAGE2_REVIEW -> WITHDRAWN).
use crate::{
    api::AppState,
    auth::{self, Actor},
    dsp_registry::{self, Dsp},
    error::{Error, Result},
    operations,
    review::{self, RIGHTS_MONEY_CLASSES},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaffRole {
    Admin,
    Reviewer,
    Operator,
    Support,
}

/// What a staff role may do. Reads are open to every staff role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duty {
    Review,
    Documents,
    Inquiries,
    Delivery,
}

impl StaffRole {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "ADMIN" => Self::Admin,
            "REVIEWER" => Self::Reviewer,
            "OPERATOR" => Self::Operator,
            "SUPPORT" => Self::Support,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "ADMIN",
            Self::Reviewer => "REVIEWER",
            Self::Operator => "OPERATOR",
            Self::Support => "SUPPORT",
        }
    }
    pub fn may(self, duty: Duty) -> bool {
        use Duty::*;
        match self {
            Self::Admin => true,
            Self::Reviewer => matches!(duty, Review | Documents | Inquiries),
            Self::Operator => matches!(duty, Delivery),
            Self::Support => matches!(duty, Inquiries),
        }
    }
}

pub struct Staff {
    pub actor: Actor,
    pub role: StaffRole,
}

/// Authenticated session + ACTIVE staff role. Non-staff get 403 (the
/// endpoint set is not secret, the data is).
pub async fn staff(s: &AppState, h: &HeaderMap, write: bool) -> Result<Staff> {
    let actor = auth::actor(&s.pool, h, &s.config, write).await?;
    let role: Option<String> = sqlx::query_scalar(
        "SELECT sm.role FROM identity.staff_members sm JOIN identity.users u ON u.id=sm.user_id
         WHERE sm.user_id=$1 AND sm.status='ACTIVE' AND u.status='ACTIVE'",
    )
    .bind(actor.user)
    .fetch_optional(&s.pool)
    .await?;
    let role = role
        .as_deref()
        .and_then(StaffRole::parse)
        .ok_or(Error::Forbidden)?;
    Ok(Staff { actor, role })
}

fn require(st: &Staff, duty: Duty) -> Result<()> {
    if st.role.may(duty) {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

/// Cross-org visibility for RLS tables whose policy honours `app.staff`
/// (delivery staging). Only called after `staff()` succeeded.
async fn staff_scope(c: &mut PgConnection) -> Result<()> {
    sqlx::query("SELECT set_config('app.staff','on',true)")
        .execute(&mut *c)
        .await?;
    Ok(())
}

fn note_ok(s: &str, max: usize) -> Result<()> {
    if s.chars().count() > max {
        return Err(Error::InvalidCode("NOTE_TOO_LONG"));
    }
    crate::text_policy::check_multiline(s)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Page {
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Page {
    fn bounds(&self) -> (i64, i64) {
        (
            self.limit.unwrap_or(50).clamp(1, 100),
            self.offset.unwrap_or(0).clamp(0, 10_000),
        )
    }
}

// ---------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------

pub async fn me(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff(s, h, false).await?;
    let duties: Vec<&str> = [
        (Duty::Review, "REVIEW"),
        (Duty::Documents, "DOCUMENTS"),
        (Duty::Inquiries, "INQUIRIES"),
        (Duty::Delivery, "DELIVERY"),
    ]
    .into_iter()
    .filter(|(d, _)| st.role.may(*d))
    .map(|(_, n)| n)
    .collect();
    Ok(json!({"user_id": st.actor.user, "role": st.role.as_str(), "duties": duties}))
}

pub async fn overview(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let row = sqlx::query(
        "SELECT
           (SELECT count(*) FROM catalog.releases WHERE status='STAGE2_REVIEW' AND archived_at IS NULL) AS review,
           (SELECT count(*) FROM catalog.releases WHERE status LIKE '%\\_CORRECTION' AND archived_at IS NULL) AS correction,
           (SELECT count(*) FROM catalog.releases WHERE status IN ('SUBMITTED','STAGE1_RUNNING','STAGE1_PASSED','STAGE2_RUNNING','STAGE2_PASSED','STAGE3_PREPARING')) AS in_pipeline,
           (SELECT count(*) FROM rights.staff_approvals WHERE status='PENDING' AND expires_at>now()) AS second_approvals,
           (SELECT count(*) FROM portal.documents WHERE status='REVIEW') AS documents,
           (SELECT count(*) FROM portal.inquiries WHERE status='OPEN') AS inquiries,
           (SELECT count(*) FROM distribution.delivery_staging WHERE approval='PENDING' AND readiness<>'CONTENT_BLOCKED') AS deliveries_to_approve,
           (SELECT count(*) FROM distribution.delivery_staging WHERE readiness='CONTENT_BLOCKED') AS deliveries_blocked,
           (SELECT count(*) FROM portal.payout_requests WHERE status='REQUESTED') AS payout_requests",
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    let n = |k: &str| row.get::<i64, _>(k);
    Ok(json!({
        "review": n("review"), "correction": n("correction"), "in_pipeline": n("in_pipeline"),
        "second_approvals": n("second_approvals"), "documents": n("documents"),
        "inquiries": n("inquiries"), "deliveries_to_approve": n("deliveries_to_approve"),
        "deliveries_blocked": n("deliveries_blocked"), "payout_requests": n("payout_requests"),
    }))
}

// ---------------------------------------------------------------------------
// Release review
// ---------------------------------------------------------------------------

const RELEASE_STATUSES: &[&str] = &[
    "SUBMITTED",
    "STAGE1_RUNNING",
    "STAGE1_CORRECTION",
    "STAGE1_PASSED",
    "STAGE2_RUNNING",
    "STAGE2_REVIEW",
    "STAGE2_CORRECTION",
    "STAGE2_PASSED",
    "STAGE3_PREPARING",
    "STAGE3_CORRECTION",
    "READY_FOR_DELIVERY",
    "ON_HOLD_RIGHTS",
    "WITHDRAWN",
];

pub async fn list_releases(s: &AppState, h: &HeaderMap, p: Page) -> Result<Value> {
    staff(s, h, false).await?;
    let status = p.status.as_deref().unwrap_or("STAGE2_REVIEW");
    if !RELEASE_STATUSES.contains(&status) {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    let (limit, offset) = p.bounds();
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
           'id', r.id, 'org_id', r.org_id, 'org_name', o.name, 'title', r.title,
           'release_type', r.release_type, 'status', r.status, 'revision_id', r.current_revision_id,
           'artist', ar.body #>> '{release,draft,artist}',
           'release_date', ar.body #>> '{release,draft,release_date}',
           'submitted_at', ar.created_at,
           'platforms', COALESCE(ar.body #> '{release,draft,platforms}', '[]'::jsonb))
         FROM catalog.releases r
         JOIN identity.orgs o ON o.id=r.org_id
         LEFT JOIN catalog.application_revisions ar ON ar.org_id=r.org_id AND ar.id=r.current_revision_id
         WHERE r.status=$1 AND r.archived_at IS NULL
         ORDER BY ar.created_at NULLS LAST, r.id
         LIMIT $2 OFFSET $3",
    )
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(&s.pool)
    .await?;
    let items: Vec<Value> = items
        .into_iter()
        .map(|mut v| {
            let codes = platform_codes(&v["platforms"]);
            v["platforms"] = json!(codes);
            v
        })
        .collect();
    Ok(json!({"items": items, "limit": limit, "offset": offset}))
}

/// Studio slugs -> D-codes for staff screens (unknown values dropped).
fn platform_codes(v: &Value) -> Vec<&'static str> {
    dsp_registry::requested(&json!({"platforms": v}))
        .unwrap_or_default()
        .into_iter()
        .map(Dsp::code)
        .collect()
}

struct OpenCheck {
    code: String,
    status: String,
    detail: String,
}

/// The checks the last Stage 2 decision left open on `revision`, with their
/// current effective status (latest override wins, as in `review::decide`).
/// The `stage2.decision` audit row names exactly the codes that held it.
async fn open_checks(c: &mut PgConnection, revision: Uuid) -> Result<Vec<OpenCheck>> {
    let reason: Option<String> = sqlx::query_scalar(
        "SELECT reason_code FROM operations.audit_events
         WHERE action='stage2.decision' AND resource_id=$1 ORDER BY occurred_at DESC, id DESC LIMIT 1",
    )
    .bind(revision)
    .fetch_optional(&mut *c)
    .await?;
    let Some(reason) = reason else {
        return Ok(Vec::new());
    };
    let codes: Vec<String> = reason
        .split_once(':')
        .map(|(_, list)| list)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect();
    if codes.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT DISTINCT ON (cr.check_code) cr.check_code, cr.status, cr.detail,
                (SELECT o.proposed_status FROM rights.review_overrides o
                  WHERE o.revision_id=cr.revision_id AND o.check_code=cr.check_code
                    AND (o.expires_at IS NULL OR o.expires_at>now())
                  ORDER BY o.created_at DESC, o.id DESC LIMIT 1) AS overridden
         FROM operations.check_results cr
         WHERE cr.revision_id=$1 AND cr.check_code = ANY($2)
         ORDER BY cr.check_code, cr.created_at DESC",
    )
    .bind(revision)
    .bind(&codes)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let base: String = r.get("status");
            OpenCheck {
                code: r.get("check_code"),
                status: r.get::<Option<String>, _>("overridden").unwrap_or(base),
                detail: r.get::<Option<String>, _>("detail").unwrap_or_default(),
            }
        })
        .filter(|c| c.status != "PASS" && c.status != "NOT_APPLICABLE")
        .collect())
}

pub async fn release_detail(s: &AppState, h: &HeaderMap, release: Uuid) -> Result<Value> {
    staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let r = sqlx::query(
        "SELECT r.id, r.org_id, o.name AS org_name, r.title, r.release_type, r.status, r.upc,
                r.current_revision_id, ar.body AS revision, ar.created_at AS submitted_at
         FROM catalog.releases r JOIN identity.orgs o ON o.id=r.org_id
         LEFT JOIN catalog.application_revisions ar ON ar.org_id=r.org_id AND ar.id=r.current_revision_id
         WHERE r.id=$1",
    )
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = r.get("org_id");
    let revision: Option<Uuid> = r.get("current_revision_id");
    let body: Value = r.get::<Option<Value>, _>("revision").unwrap_or(Value::Null);
    let draft = &body["release"]["draft"];
    let checks: Vec<Value> = match revision {
        Some(rev) => sqlx::query_scalar(
            "SELECT jsonb_build_object('check_code',check_code,'status',status,'detail',detail,'rule_version',rule_version,'at',created_at)
             FROM (SELECT DISTINCT ON (check_code) * FROM operations.check_results
                   WHERE revision_id=$1 ORDER BY check_code, created_at DESC) c
             ORDER BY check_code",
        )
        .bind(rev)
        .fetch_all(&mut *tx)
        .await?,
        None => Vec::new(),
    };
    let open: Vec<Value> = match revision {
        Some(rev) => open_checks(&mut tx, rev)
            .await?
            .into_iter()
            .map(|c| json!({"check_code": c.code, "status": c.status, "detail": c.detail}))
            .collect(),
        None => Vec::new(),
    };
    let rev = revision.unwrap_or(Uuid::nil());
    let overrides: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'check_code',check_code,'original_status',original_status,
                'proposed_status',proposed_status,'reason',reason,'actor_user_id',actor_user_id,
                'second_approver_user_id',second_approver_user_id,'at',created_at)
         FROM rights.review_overrides WHERE org_id=$1 AND revision_id=$2 ORDER BY created_at",
    )
    .bind(org)
    .bind(rev)
    .fetch_all(&mut *tx)
    .await?;
    let notes: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'revision_id',revision_id,'check_code',check_code,'decision',decision,
                'note',note,'author_user_id',author_user_id,'at',created_at)
         FROM rights.review_notes WHERE org_id=$1 AND release_id=$2 ORDER BY created_at",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let approvals: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'check_codes',check_codes,'reason',reason,'requested_by',requested_by,
                'status',status,'decided_by',decided_by,'expires_at',expires_at,'at',created_at)
         FROM rights.staff_approvals WHERE org_id=$1 AND release_id=$2 ORDER BY created_at DESC LIMIT 20",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let documents: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'kind',kind,'title',title,'status',status,'review_note',review_note,
                'file_name',file_name,'asset_id',asset_id,'signed_at',signed_at,'row_version',row_version,'updated_at',updated_at)
         FROM portal.documents WHERE org_id=$1 AND release_id=$2 ORDER BY created_at",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let application: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('application_no',application_no,'form',form,'content_hash',content_hash,
                'signer_name',signer_name,'signer_role',signer_role,'agreements',agreements,'received_at',received_at)
         FROM portal.release_applications WHERE org_id=$1 AND release_id=$2 ORDER BY received_at DESC LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let staging: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('package_id',package_id,'dsp',dsp_code,'readiness',readiness,'approval',approval,
                'checks',checks,'route_status',route_status,'route_reason',route_reason,
                'ern_message_id',ern_message_id,'ern_sha256',ern_sha256,'ern_is_preview',ern_is_preview,
                'approval_by',approval_by,'approval_note',approval_note,'approval_at',approval_at,'staged_at',staged_at)
         FROM distribution.delivery_staging WHERE org_id=$1 AND release_id=$2 ORDER BY staged_at DESC, dsp_code",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let timeline: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('action',action,'reason',reason_code,'actor_user_id',actor_user_id,
                'actor_service',actor_service,'at',occurred_at)
         FROM operations.audit_events
         WHERE org_id=$1 AND (resource_id=$2 OR resource_id=$3)
         ORDER BY occurred_at DESC LIMIT 100",
    )
    .bind(org)
    .bind(release)
    .bind(rev)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(json!({
        "release": {
            "id": release, "org_id": org, "org_name": r.get::<String,_>("org_name"),
            "title": r.get::<String,_>("title"), "release_type": r.get::<String,_>("release_type"),
            "status": r.get::<String,_>("status"), "upc": r.get::<Option<String>,_>("upc"),
            "revision_id": revision, "submitted_at": r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("submitted_at"),
        },
        "application": {
            "artist": draft["artist"], "language": draft["language"], "genre": draft["genre"],
            "release_date": draft["release_date"], "original_date": draft["originalDate"],
            "label": draft["label"], "p_line": draft["p_line"], "c_line": draft["c_line"],
            "territories": draft["territories"], "platforms": platform_codes(&draft["platforms"]),
            "declarations": body["declarations"], "tracks": body["tracks"],
        },
        "signed_application": application,
        "checks": checks,
        "open_checks": open,
        "overrides": overrides,
        "notes": notes,
        "second_approvals": approvals,
        "documents": documents,
        "delivery_staging": staging,
        "timeline": timeline,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckNote {
    pub check_code: String,
    pub note: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInput {
    /// APPROVE | REQUEST_CORRECTION | REJECT
    pub action: String,
    /// The revision the reviewer looked at (optimistic guard).
    pub revision_id: Uuid,
    pub reason: String,
    /// Per-check notes shown to the artist (correction / rejection).
    #[serde(default)]
    pub notes: Vec<CheckNote>,
}

/// Needs a second staff reviewer to PASS.
fn sensitive(c: &OpenCheck) -> bool {
    c.status == "BLOCKED" || RIGHTS_MONEY_CLASSES.contains(&c.code.as_str())
}

async fn locked_review_release(
    c: &mut PgConnection,
    release: Uuid,
    revision: Uuid,
) -> Result<(Uuid, String)> {
    let row = sqlx::query(
        "SELECT org_id, status, current_revision_id FROM catalog.releases WHERE id=$1 FOR UPDATE",
    )
    .bind(release)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    if row.get::<Option<Uuid>, _>("current_revision_id") != Some(revision) {
        return Err(Error::Conflict);
    }
    let status: String = row.get("status");
    if status != "STAGE2_REVIEW" {
        return Err(Error::PolicyGate("RELEASE_NOT_IN_REVIEW"));
    }
    Ok((row.get("org_id"), status))
}

#[allow(clippy::too_many_arguments)]
async fn write_note(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    revision: Uuid,
    check_code: Option<&str>,
    decision: &str,
    note: &str,
    author: Uuid,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO rights.review_notes(id,org_id,release_id,revision_id,check_code,decision,note,author_user_id)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(release)
    .bind(revision)
    .bind(check_code)
    .bind(decision)
    .bind(note)
    .bind(author)
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// Apply PASS (or another status) overrides for `codes` and queue Stage 2.
#[allow(clippy::too_many_arguments)]
async fn apply_overrides(
    c: &mut PgConnection,
    org: Uuid,
    revision: Uuid,
    checks: &[OpenCheck],
    proposed: &str,
    reason: &str,
    actor: Uuid,
    second: Option<Uuid>,
) -> Result<bool> {
    let mut last = None;
    for chk in checks {
        last = Some(
            review::insert_override(
                c,
                org,
                revision,
                &chk.code,
                &chk.status,
                proposed,
                reason,
                actor,
                second,
            )
            .await?,
        );
    }
    match last {
        Some(cause) => review::enqueue_reevaluation(c, org, revision, cause).await,
        None => Ok(false),
    }
}

pub async fn decide(s: &AppState, h: &HeaderMap, release: Uuid, i: DecisionInput) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Review)?;
    let reason = i.reason.trim();
    if reason.is_empty() {
        return Err(Error::PolicyGate("DECISION_REASON_REQUIRED"));
    }
    note_ok(reason, review::MAX_OVERRIDE_REASON_CHARS)?;
    if i.notes.len() > 100 {
        return Err(Error::Invalid);
    }
    for n in &i.notes {
        note_ok(&n.note, 2000)?;
        crate::text_policy::check(&n.check_code)?;
    }
    let mut tx = s.pool.begin().await?;
    let (org, _) = locked_review_release(&mut tx, release, i.revision_id).await?;
    let open = open_checks(&mut tx, i.revision_id).await?;
    let user = st.actor.user;
    let request = st.actor.request;
    let out = match i.action.as_str() {
        "APPROVE" => {
            if open.iter().any(sensitive) {
                // Rights/money classes and BLOCKED findings need a second
                // staff reviewer: file one request per revision.
                let codes: Vec<String> = open.iter().map(|c| c.code.clone()).collect();
                let id = Uuid::new_v4();
                let n = sqlx::query(
                    "INSERT INTO rights.staff_approvals(id,org_id,release_id,revision_id,check_codes,reason,requested_by)
                     VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING",
                )
                .bind(id)
                .bind(org)
                .bind(release)
                .bind(i.revision_id)
                .bind(&codes)
                .bind(reason)
                .bind(user)
                .execute(&mut *tx)
                .await?
                .rows_affected();
                if n == 0 {
                    return Err(Error::PolicyGate("SECOND_APPROVAL_ALREADY_PENDING"));
                }
                operations::audit(
                    &mut tx,
                    Some(user),
                    Some(org),
                    Some(release),
                    "staff.approval_requested",
                    &codes.join(","),
                    request,
                )
                .await?;
                json!({"result": "PENDING_SECOND_APPROVAL", "approval_id": id, "check_codes": codes})
            } else {
                let queued = apply_overrides(
                    &mut tx,
                    org,
                    i.revision_id,
                    &open,
                    "PASS",
                    reason,
                    user,
                    None,
                )
                .await?;
                write_note(
                    &mut tx,
                    org,
                    release,
                    i.revision_id,
                    None,
                    "APPROVE",
                    reason,
                    user,
                )
                .await?;
                operations::audit(
                    &mut tx,
                    Some(user),
                    Some(org),
                    Some(release),
                    "staff.approved",
                    &format!("PASS:{}", open.len()),
                    request,
                )
                .await?;
                json!({"result": "APPLIED", "reevaluation_queued": queued, "passed": open.iter().map(|c| &c.code).collect::<Vec<_>>()})
            }
        }
        "REQUEST_CORRECTION" => {
            if open.is_empty() {
                return Err(Error::PolicyGate("NOTHING_TO_CORRECT"));
            }
            // Stricter status: one reviewer is enough. Every open check turns
            // into a correction so the decision leaves review for the artist.
            let queued = apply_overrides(
                &mut tx,
                org,
                i.revision_id,
                &open,
                "CORRECTION_REQUIRED",
                reason,
                user,
                None,
            )
            .await?;
            write_note(
                &mut tx,
                org,
                release,
                i.revision_id,
                None,
                "REQUEST_CORRECTION",
                reason,
                user,
            )
            .await?;
            for n in &i.notes {
                write_note(
                    &mut tx,
                    org,
                    release,
                    i.revision_id,
                    Some(n.check_code.as_str()),
                    "REQUEST_CORRECTION",
                    &n.note,
                    user,
                )
                .await?;
            }
            operations::audit(
                &mut tx,
                Some(user),
                Some(org),
                Some(release),
                "staff.correction_requested",
                &open
                    .iter()
                    .map(|c| c.code.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                request,
            )
            .await?;
            json!({"result": "APPLIED", "reevaluation_queued": queued})
        }
        "REJECT" => {
            write_note(
                &mut tx,
                org,
                release,
                i.revision_id,
                None,
                "REJECT",
                reason,
                user,
            )
            .await?;
            for n in &i.notes {
                write_note(
                    &mut tx,
                    org,
                    release,
                    i.revision_id,
                    Some(n.check_code.as_str()),
                    "REJECT",
                    &n.note,
                    user,
                )
                .await?;
            }
            sqlx::query(
                "UPDATE catalog.releases SET status='WITHDRAWN', row_version=row_version+1 WHERE id=$1 AND status='STAGE2_REVIEW'",
            )
            .bind(release)
            .execute(&mut *tx)
            .await?;
            operations::audit(
                &mut tx,
                Some(user),
                Some(org),
                Some(release),
                "staff.rejected",
                "STAGE2_REVIEW->WITHDRAWN",
                request,
            )
            .await?;
            json!({"result": "REJECTED", "status": "WITHDRAWN"})
        }
        _ => return Err(Error::InvalidCode("DECISION_ACTION_UNKNOWN")),
    };
    tx.commit().await?;
    Ok(out)
}

pub async fn decide_second_approval(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    approve: bool,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Review)?;
    let mut tx = s.pool.begin().await?;
    let a = sqlx::query(
        "SELECT org_id, release_id, revision_id, check_codes, reason, requested_by, status, expires_at>now() AS live
         FROM rights.staff_approvals WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if a.get::<String, _>("status") != "PENDING" || !a.get::<bool, _>("live") {
        return Err(Error::PolicyGate("APPROVAL_NOT_PENDING"));
    }
    let requester: Uuid = a.get("requested_by");
    let (org, release, revision): (Uuid, Uuid, Uuid) =
        (a.get("org_id"), a.get("release_id"), a.get("revision_id"));
    let user = st.actor.user;
    if !approve {
        sqlx::query(
            "UPDATE rights.staff_approvals SET status='DECLINED', decided_by=$2, decided_at=now() WHERE id=$1",
        )
        .bind(id)
        .bind(user)
        .execute(&mut *tx)
        .await?;
        operations::audit(
            &mut tx,
            Some(user),
            Some(org),
            Some(release),
            "staff.approval_declined",
            &id.to_string(),
            st.actor.request,
        )
        .await?;
        tx.commit().await?;
        return Ok(json!({"result": "DECLINED"}));
    }
    if requester == user {
        return Err(Error::PolicyGate("SECOND_APPROVER_MUST_DIFFER"));
    }
    locked_review_release(&mut tx, release, revision).await?;
    let reason: String = a.get("reason");
    let wanted: Vec<String> = a.get("check_codes");
    // Approve exactly what was requested and is still open.
    let open: Vec<OpenCheck> = open_checks(&mut tx, revision)
        .await?
        .into_iter()
        .filter(|c| wanted.contains(&c.code))
        .collect();
    let queued = apply_overrides(
        &mut tx,
        org,
        revision,
        &open,
        "PASS",
        &reason,
        requester,
        Some(user),
    )
    .await?;
    sqlx::query(
        "UPDATE rights.staff_approvals SET status='APPROVED', decided_by=$2, decided_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(user)
    .execute(&mut *tx)
    .await?;
    write_note(
        &mut tx, org, release, revision, None, "APPROVE", &reason, requester,
    )
    .await?;
    operations::audit(
        &mut tx,
        Some(user),
        Some(org),
        Some(release),
        "staff.approval_granted",
        &id.to_string(),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"result": "APPLIED", "reevaluation_queued": queued}))
}

pub async fn list_second_approvals(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff(s, h, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',a.id,'org_id',a.org_id,'release_id',a.release_id,'revision_id',a.revision_id,
                'title',r.title,'check_codes',a.check_codes,'reason',a.reason,'requested_by',a.requested_by,
                'expires_at',a.expires_at,'at',a.created_at)
         FROM rights.staff_approvals a JOIN catalog.releases r ON r.id=a.release_id
         WHERE a.status='PENDING' AND a.expires_at>now() ORDER BY a.created_at LIMIT 200",
    )
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"items": items}))
}

// ---------------------------------------------------------------------------
// Documents (agreements, rights proofs)
// ---------------------------------------------------------------------------

pub async fn list_documents(s: &AppState, h: &HeaderMap, p: Page) -> Result<Value> {
    staff(s, h, false).await?;
    let status = p.status.as_deref().unwrap_or("REVIEW");
    if ![
        "AWAITING_DOCUMENTS",
        "REVIEW",
        "PREPARED",
        "APPROVED",
        "NEEDS",
        "SIGNED",
    ]
    .contains(&status)
    {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    let (limit, offset) = p.bounds();
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',d.id,'org_id',d.org_id,'org_name',o.name,'release_id',d.release_id,
                'release_title',r.title,'kind',d.kind,'title',d.title,'status',d.status,'review_note',d.review_note,
                'file_name',d.file_name,'asset_id',d.asset_id,'row_version',d.row_version,'updated_at',d.updated_at)
         FROM portal.documents d JOIN identity.orgs o ON o.id=d.org_id
         LEFT JOIN catalog.releases r ON r.id=d.release_id
         WHERE d.status=$1 ORDER BY d.updated_at LIMIT $2 OFFSET $3",
    )
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"items": items, "limit": limit, "offset": offset}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentReview {
    /// APPROVED | NEEDS
    pub status: String,
    #[serde(default)]
    pub note: String,
    pub row_version: i64,
}

pub async fn review_document(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    i: DocumentReview,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Documents)?;
    if !matches!(i.status.as_str(), "APPROVED" | "NEEDS") {
        return Err(Error::InvalidCode("DOCUMENT_STATUS_INVALID"));
    }
    if i.status == "NEEDS" && i.note.trim().is_empty() {
        return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
    }
    note_ok(&i.note, 1000)?;
    let mut tx = s.pool.begin().await?;
    // Only documents waiting on staff can be decided: agreements in
    // REVIEW/PREPARED, rights proofs in REVIEW.
    let row = sqlx::query(
        "UPDATE portal.documents SET status=$2, review_note=$3, row_version=row_version+1, updated_at=now()
         WHERE id=$1 AND row_version=$4
           AND ((kind='AGREEMENT' AND status IN ('REVIEW','PREPARED')) OR (kind='RIGHTS_PROOF' AND status='REVIEW'))
         RETURNING org_id, row_version",
    )
    .bind(id)
    .bind(&i.status)
    .bind(i.note.trim())
    .bind(i.row_version)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    let org: Uuid = row.get("org_id");
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(id),
        "staff.document_reviewed",
        &i.status,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": i.status, "row_version": row.get::<i64,_>("row_version")}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofRequest {
    pub release_id: Uuid,
    pub title: String,
    #[serde(default)]
    pub body: String,
}

/// Ask an artist for a rights proof (AWAITING_DOCUMENTS; the insert trigger
/// notifies the org).
pub async fn request_proof(
    s: &AppState,
    h: &HeaderMap,
    org: Uuid,
    i: ProofRequest,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Documents)?;
    let title = i.title.trim();
    if title.is_empty() || title.chars().count() > 200 {
        return Err(Error::Invalid);
    }
    crate::text_policy::check(title)?;
    note_ok(&i.body, 20_000)?;
    let mut tx = s.pool.begin().await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.releases WHERE org_id=$1 AND id=$2)",
    )
    .bind(org)
    .bind(i.release_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(Error::NotFound);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.documents(id,org_id,release_id,kind,title,body,status) VALUES($1,$2,$3,'RIGHTS_PROOF',$4,$5,'AWAITING_DOCUMENTS')",
    )
    .bind(id)
    .bind(org)
    .bind(i.release_id)
    .bind(title)
    .bind(&i.body)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(id),
        "staff.proof_requested",
        "RIGHTS_PROOF",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "AWAITING_DOCUMENTS"}))
}

// ---------------------------------------------------------------------------
// Inquiries
// ---------------------------------------------------------------------------

pub async fn list_inquiries(s: &AppState, h: &HeaderMap, p: Page) -> Result<Value> {
    staff(s, h, false).await?;
    let status = p.status.as_deref().unwrap_or("OPEN");
    if !["OPEN", "ANSWERED", "CLOSED"].contains(&status) {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    let (limit, offset) = p.bounds();
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',i.id,'org_id',i.org_id,'org_name',o.name,'category',i.category,
                'release_id',i.release_id,'subject',i.subject,'status',i.status,'created_at',i.created_at,'updated_at',i.updated_at)
         FROM portal.inquiries i JOIN identity.orgs o ON o.id=i.org_id
         WHERE i.status=$1 ORDER BY i.updated_at LIMIT $2 OFFSET $3",
    )
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"items": items, "limit": limit, "offset": offset}))
}

pub async fn inquiry(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    staff(s, h, false).await?;
    let head: Value = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',i.id,'org_id',i.org_id,'org_name',o.name,'category',i.category,
                'release_id',i.release_id,'subject',i.subject,'status',i.status,'created_at',i.created_at)
         FROM portal.inquiries i JOIN identity.orgs o ON o.id=i.org_id WHERE i.id=$1",
    )
    .bind(id)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    let messages: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'author_kind',author_kind,'author_user',author_user,'body',body,'created_at',created_at)
         FROM portal.inquiry_messages WHERE inquiry_id=$1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"inquiry": head, "messages": messages}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub body: String,
}

pub async fn reply(s: &AppState, h: &HeaderMap, id: Uuid, i: Reply) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Inquiries)?;
    let body = i.body.trim();
    if body.is_empty() {
        return Err(Error::Invalid);
    }
    note_ok(body, 4000)?;
    let mut tx = s.pool.begin().await?;
    let org: Uuid = sqlx::query_scalar(
        "SELECT org_id FROM portal.inquiries WHERE id=$1 AND status<>'CLOSED' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::PolicyGate("INQUIRY_CLOSED"))?;
    // The insert trigger marks the thread ANSWERED and notifies the author.
    let mid: Uuid = sqlx::query_scalar(
        "INSERT INTO portal.inquiry_messages(inquiry_id,org_id,author_kind,author_user,body) VALUES($1,$2,'STAFF',$3,$4) RETURNING id",
    )
    .bind(id)
    .bind(org)
    .bind(st.actor.user)
    .bind(body)
    .fetch_one(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(id),
        "staff.inquiry_replied",
        "STAFF_REPLY",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": mid, "status": "ANSWERED"}))
}

// ---------------------------------------------------------------------------
// Delivery staging
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryPage {
    pub approval: Option<String>,
    pub readiness: Option<String>,
    pub dsp: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_deliveries(s: &AppState, h: &HeaderMap, p: DeliveryPage) -> Result<Value> {
    staff(s, h, false).await?;
    let approval = p.approval.as_deref().unwrap_or("PENDING");
    if !["PENDING", "APPROVED", "HELD"].contains(&approval) {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    if let Some(r) = p.readiness.as_deref()
        && !["CONTENT_BLOCKED", "AWAITING_PARTNER", "READY"].contains(&r)
    {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    if let Some(d) = p.dsp.as_deref()
        && Dsp::from_code(d).is_none()
    {
        return Err(Error::InvalidCode("DSP_UNKNOWN"));
    }
    let (limit, offset) = (
        p.limit.unwrap_or(50).clamp(1, 100),
        p.offset.unwrap_or(0).clamp(0, 10_000),
    );
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('package_id',s.package_id,'dsp',s.dsp_code,'org_id',s.org_id,'org_name',o.name,
                'release_id',s.release_id,'title',r.title,'readiness',s.readiness,'approval',s.approval,
                'route_status',s.route_status,'route_reason',s.route_reason,'ern_is_preview',s.ern_is_preview,
                'blockers',(SELECT COALESCE(jsonb_agg(c->>'code'),'[]'::jsonb) FROM jsonb_array_elements(s.checks) c WHERE c->>'severity'='BLOCKER'),
                'staged_at',s.staged_at)
         FROM distribution.delivery_staging s
         JOIN identity.orgs o ON o.id=s.org_id
         JOIN catalog.releases r ON r.org_id=s.org_id AND r.id=s.release_id
         WHERE s.approval=$1 AND ($2::text IS NULL OR s.readiness=$2) AND ($3::text IS NULL OR s.dsp_code=$3)
         ORDER BY s.staged_at, s.dsp_code LIMIT $4 OFFSET $5",
    )
    .bind(approval)
    .bind(p.readiness.as_deref())
    .bind(p.dsp.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(json!({"items": items, "limit": limit, "offset": offset}))
}

/// The exact ERN (or preview) staff approve, as XML.
pub async fn delivery_ern(
    s: &AppState,
    h: &HeaderMap,
    package: Uuid,
    code: &str,
) -> Result<String> {
    staff(s, h, false).await?;
    Dsp::from_code(code).ok_or(Error::NotFound)?;
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let xml: Option<String> = sqlx::query_scalar(
        "SELECT ern_xml FROM distribution.delivery_staging WHERE package_id=$1 AND dsp_code=$2",
    )
    .bind(package)
    .bind(code)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    tx.commit().await?;
    xml.ok_or(Error::NotFound)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryDecision {
    /// APPROVE | HOLD
    pub action: String,
    #[serde(default)]
    pub note: String,
    /// The ERN the operator reviewed; a re-stage in between changes it.
    pub ern_sha256: Option<String>,
}

pub async fn decide_delivery(
    s: &AppState,
    h: &HeaderMap,
    package: Uuid,
    code: &str,
    i: DeliveryDecision,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Delivery)?;
    Dsp::from_code(code).ok_or(Error::NotFound)?;
    note_ok(&i.note, 1000)?;
    let approval = match i.action.as_str() {
        "APPROVE" => "APPROVED",
        "HOLD" => {
            if i.note.trim().is_empty() {
                return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
            }
            "HELD"
        }
        _ => return Err(Error::InvalidCode("DECISION_ACTION_UNKNOWN")),
    };
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let row = sqlx::query(
        "SELECT org_id, release_id, readiness, ern_sha256 FROM distribution.delivery_staging
         WHERE package_id=$1 AND dsp_code=$2 FOR UPDATE",
    )
    .bind(package)
    .bind(code)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = row.get("org_id");
    if approval == "APPROVED" {
        if row.get::<String, _>("readiness") == "CONTENT_BLOCKED" {
            return Err(Error::PolicyGate("DELIVERY_CONTENT_BLOCKED"));
        }
        if i.ern_sha256.is_some() && i.ern_sha256 != row.get::<Option<String>, _>("ern_sha256") {
            return Err(Error::Conflict);
        }
    }
    sqlx::query(
        "UPDATE distribution.delivery_staging SET approval=$3, approval_by=$4, approval_note=$5, approval_at=now()
         WHERE package_id=$1 AND dsp_code=$2",
    )
    .bind(package)
    .bind(code)
    .bind(approval)
    .bind(st.actor.user)
    .bind(i.note.trim())
    .execute(&mut *tx)
    .await?;
    // An approval re-runs E-0 for the package: a live route sends now, a
    // pending one waits for the next re-stage after onboarding.
    let queued = if approval == "APPROVED" {
        operations::enqueue(
            &mut tx,
            "delivery",
            "delivery.enqueue",
            &json!({"package_id": package}),
            &format!("delivery.enqueue:{package}:approval:{}", Uuid::new_v4()),
            None,
        )
        .await?;
        true
    } else {
        false
    };
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(package),
        "staff.delivery_decided",
        &format!("{code}:{approval}"),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"package_id": package, "dsp": code, "approval": approval, "delivery_enqueue_queued": queued}),
    )
}

/// Re-evaluate a package (after onboarding progress or an issuer change).
pub async fn restage(s: &AppState, h: &HeaderMap, package: Uuid) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    let org: Uuid =
        sqlx::query_scalar("SELECT org_id FROM distribution.distribution_packages WHERE id=$1")
            .bind(package)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
    let job = operations::enqueue(
        &mut tx,
        "distribution",
        "delivery.stage",
        &json!({"package_id": package}),
        &format!("delivery.stage:{package}:restage:{}", Uuid::new_v4()),
        None,
    )
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(package),
        "staff.delivery_restaged",
        "RESTAGE",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"package_id": package, "job_id": job}))
}

// ---------------------------------------------------------------------------
// DSP overview
// ---------------------------------------------------------------------------

pub async fn dsps(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff(s, h, false).await?;
    let rows = sqlx::query(
        "SELECT p.partner_id, p.transport, p.activation_kind, p.route_kind, p.delivery_enabled,
                p.ddex_recipient_dpid IS NOT NULL AS recipient_dpid,
                COALESCE((p.capabilities->>'send_or_publish')::boolean,false) AS can_send,
                o.stage, to_jsonb(COALESCE(o.gaps, ARRAY[]::text[])) AS gaps
         FROM execution.adapter_profiles p
         LEFT JOIN LATERAL execution.partner_readiness(p.partner_id) o ON true
         WHERE p.partner_id = ANY($1)",
    )
    .bind(Dsp::ALL.iter().map(|d| d.code()).collect::<Vec<_>>())
    .fetch_all(&s.pool)
    .await?;
    let mut by_code: BTreeMap<String, Value> = rows
        .into_iter()
        .map(|r| {
            let code: String = r.get("partner_id");
            (
                code,
                json!({
                    "transport": r.get::<String,_>("transport"),
                    "activation_kind": r.get::<String,_>("activation_kind"),
                    "route_kind": r.get::<String,_>("route_kind"),
                    "delivery_enabled": r.get::<bool,_>("delivery_enabled"),
                    "recipient_dpid_registered": r.get::<bool,_>("recipient_dpid"),
                    "adapter_can_send": r.get::<bool,_>("can_send"),
                    "onboarding_stage": r.get::<Option<String>,_>("stage"),
                    "onboarding_gaps": r.get::<Value,_>("gaps"),
                }),
            )
        })
        .collect();
    let items: Vec<Value> = dsp_registry::catalog()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut v| {
            let code = v["code"].as_str().unwrap_or("").to_owned();
            v["route"] = by_code.remove(&code).unwrap_or(Value::Null);
            v
        })
        .collect();
    Ok(json!({"items": items}))
}

// ---------------------------------------------------------------------------
// Payout requests (read-only: money moves only through operations tooling)
// ---------------------------------------------------------------------------

pub async fn payout_requests(s: &AppState, h: &HeaderMap, p: Page) -> Result<Value> {
    let st = staff(s, h, false).await?;
    if st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    let status = p.status.as_deref().unwrap_or("REQUESTED");
    if !["REQUESTED", "ORDERED", "REJECTED", "CANCELLED"].contains(&status) {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    let (limit, offset) = p.bounds();
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',p.id,'org_id',p.org_id,'org_name',o.name,'amount',p.amount,'currency',p.currency,
                'status',p.status,'payout_order_id',p.payout_order_id,'created_at',p.created_at)
         FROM portal.payout_requests p JOIN identity.orgs o ON o.id=p.org_id
         WHERE p.status=$1 ORDER BY p.created_at LIMIT $2 OFFSET $3",
    )
    .bind(status)
    .bind(limit)
    .bind(offset)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"items": items, "limit": limit, "offset": offset}))
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

async fn h_me(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(me(&s, &h).await?))
}
async fn h_overview(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(overview(&s, &h).await?))
}
async fn h_releases(
    State(s): State<AppState>,
    Query(p): Query<Page>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list_releases(&s, &h, p).await?))
}
async fn h_release(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(release_detail(&s, &h, id).await?))
}
async fn h_decide(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    Ok(Json(decide(&s, &h, id, i).await?))
}
async fn h_approvals(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(list_second_approvals(&s, &h).await?))
}
async fn h_approval_approve(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(decide_second_approval(&s, &h, id, true).await?))
}
async fn h_approval_decline(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(decide_second_approval(&s, &h, id, false).await?))
}
async fn h_documents(
    State(s): State<AppState>,
    Query(p): Query<Page>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list_documents(&s, &h, p).await?))
}
async fn h_document_review(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DocumentReview>,
) -> Result<Json<Value>> {
    Ok(Json(review_document(&s, &h, id, i).await?))
}
async fn h_request_proof(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ProofRequest>,
) -> Result<Json<Value>> {
    Ok(Json(request_proof(&s, &h, org, i).await?))
}
async fn h_inquiries(
    State(s): State<AppState>,
    Query(p): Query<Page>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list_inquiries(&s, &h, p).await?))
}
async fn h_inquiry(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(inquiry(&s, &h, id).await?))
}
async fn h_reply(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<Reply>,
) -> Result<Json<Value>> {
    Ok(Json(reply(&s, &h, id, i).await?))
}
async fn h_deliveries(
    State(s): State<AppState>,
    Query(p): Query<DeliveryPage>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list_deliveries(&s, &h, p).await?))
}
async fn h_delivery_ern(
    State(s): State<AppState>,
    Path((package, code)): Path<(Uuid, String)>,
    h: HeaderMap,
) -> Result<([(axum::http::HeaderName, &'static str); 1], String)> {
    let xml = delivery_ern(&s, &h, package, &code).await?;
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "application/xml; charset=utf-8",
        )],
        xml,
    ))
}
async fn h_delivery_decide(
    State(s): State<AppState>,
    Path((package, code)): Path<(Uuid, String)>,
    h: HeaderMap,
    Json(i): Json<DeliveryDecision>,
) -> Result<Json<Value>> {
    Ok(Json(decide_delivery(&s, &h, package, &code, i).await?))
}
async fn h_restage(
    State(s): State<AppState>,
    Path(package): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(restage(&s, &h, package).await?))
}
async fn h_dsps(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(dsps(&s, &h).await?))
}
async fn h_payouts(
    State(s): State<AppState>,
    Query(p): Query<Page>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(payout_requests(&s, &h, p).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/me", get(h_me))
        .route("/api/staff/overview", get(h_overview))
        .route("/api/staff/releases", get(h_releases))
        .route("/api/staff/releases/{id}", get(h_release))
        .route("/api/staff/releases/{id}/decision", post(h_decide))
        .route("/api/staff/approvals", get(h_approvals))
        .route(
            "/api/staff/approvals/{id}/approve",
            post(h_approval_approve),
        )
        .route(
            "/api/staff/approvals/{id}/decline",
            post(h_approval_decline),
        )
        .route("/api/staff/documents", get(h_documents))
        .route("/api/staff/documents/{id}/review", post(h_document_review))
        .route("/api/staff/orgs/{org}/documents", post(h_request_proof))
        .route("/api/staff/inquiries", get(h_inquiries))
        .route("/api/staff/inquiries/{id}", get(h_inquiry))
        .route("/api/staff/inquiries/{id}/reply", post(h_reply))
        .route("/api/staff/deliveries", get(h_deliveries))
        .route("/api/staff/deliveries/{package}/restage", post(h_restage))
        .route(
            "/api/staff/deliveries/{package}/{code}/ern",
            get(h_delivery_ern),
        )
        .route(
            "/api/staff/deliveries/{package}/{code}/decision",
            post(h_delivery_decide),
        )
        .route("/api/staff/dsps", get(h_dsps))
        .route("/api/staff/payouts", get(h_payouts))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duties_follow_roles() {
        assert!(StaffRole::Admin.may(Duty::Delivery));
        assert!(StaffRole::Reviewer.may(Duty::Review));
        assert!(!StaffRole::Reviewer.may(Duty::Delivery));
        assert!(StaffRole::Operator.may(Duty::Delivery));
        assert!(!StaffRole::Operator.may(Duty::Review));
        assert!(StaffRole::Support.may(Duty::Inquiries));
        assert!(!StaffRole::Support.may(Duty::Documents));
    }

    #[test]
    fn sensitive_checks_need_second_reviewer() {
        let c = |code: &str, status: &str| OpenCheck {
            code: code.into(),
            status: status.into(),
            detail: String::new(),
        };
        assert!(sensitive(&c("S2_RIGHTS_SCOPE", "REVIEW_REQUIRED")));
        assert!(sensitive(&c("S2_INTEGRITY_DUP", "BLOCKED")));
        assert!(!sensitive(&c("S2_INTEGRITY_DUP", "REVIEW_REQUIRED")));
    }

    #[test]
    fn platform_codes_map_studio_slugs() {
        assert_eq!(
            platform_codes(&json!(["spotify", "melon"])),
            vec!["D-1", "D-5"]
        );
        assert!(platform_codes(&Value::Null).is_empty());
    }
}
