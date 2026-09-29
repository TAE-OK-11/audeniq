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
//!
//! The release review queue also holds every release whose signed
//! application (the AGREEMENT document) still waits on staff: automatic
//! checks can pass all the way to READY_FOR_DELIVERY, but delivery waits for
//! the signed agreement (0048), and the agreement is decided here, with the
//! release, not in the document queue. There APPROVE clears the agreement for
//! signing, REQUEST_CORRECTION sends the release back to the artist
//! (READY_FOR_DELIVERY -> STAGE3_CORRECTION) and REJECT closes it
//! (READY_FOR_DELIVERY -> WITHDRAWN). The document queue keeps only the
//! rights proofs staff asked for.
use crate::{
    api::AppState,
    auth::{self, Actor},
    dsp_registry::{self, Dsp},
    error::{Error, Result},
    operations, review,
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

/// `r` (catalog.releases) waits on a staff decision in 발매 심사: parked by
/// Stage 2, or through the automatic checks with its signed application
/// (AGREEMENT) not yet decided. Mid-pipeline releases show up once the
/// pipeline settles, so the reviewer always sees final check results.
/// (A macro so the SQL stays a compile-time constant.)
macro_rules! awaiting_decision {
    () => {
        "(r.status='STAGE2_REVIEW' OR (r.status='READY_FOR_DELIVERY' AND EXISTS(
          SELECT 1 FROM portal.documents d WHERE d.org_id=r.org_id AND d.release_id=r.id
           AND d.kind='AGREEMENT' AND d.status IN ('REVIEW','PREPARED'))))"
    };
}

pub async fn overview(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    const SQL: &str = concat!(
        "SELECT
           (SELECT count(*) FROM catalog.releases r WHERE r.archived_at IS NULL AND ",
        awaiting_decision!(),
        ") AS review,
           (SELECT count(*) FROM catalog.releases WHERE status LIKE '%\\_CORRECTION' AND archived_at IS NULL) AS correction,
           (SELECT count(*) FROM catalog.releases WHERE status IN ('SUBMITTED','STAGE1_RUNNING','STAGE1_PASSED','STAGE2_RUNNING','STAGE2_PASSED','STAGE3_PREPARING')) AS in_pipeline,
           (SELECT count(*) FROM rights.staff_approvals WHERE status='PENDING' AND expires_at>now()) AS second_approvals,
           (SELECT count(*) FROM portal.documents WHERE kind='RIGHTS_PROOF' AND status='REVIEW') AS documents,
           (SELECT count(*) FROM portal.inquiries WHERE status='OPEN') AS inquiries,
           (SELECT count(*) FROM distribution.delivery_staging s JOIN catalog.releases r ON r.id=s.release_id AND r.current_revision_id=s.revision_id
              WHERE s.approval='PENDING' AND s.readiness<>'CONTENT_BLOCKED' AND r.status<>'WITHDRAWN') AS deliveries_to_approve,
           (SELECT count(*) FROM distribution.delivery_staging s JOIN catalog.releases r ON r.id=s.release_id AND r.current_revision_id=s.revision_id
              WHERE s.readiness='CONTENT_BLOCKED' AND r.status<>'WITHDRAWN') AS deliveries_blocked,
           (SELECT count(DISTINCT s.release_id) FROM distribution.delivery_staging s
              JOIN catalog.releases r ON r.id=s.release_id AND r.current_revision_id=s.revision_id
              WHERE s.approval='PENDING'
              AND EXISTS(SELECT 1 FROM jsonb_array_elements(s.checks) c WHERE c->>'code' IN ('DSP_LOUDNESS_ADVISORY','DSP_CLIPPING_ADVISORY'))) AS audio_advisories,
           (SELECT count(*) FROM portal.payout_requests WHERE status='REQUESTED') AS payout_requests"
    );
    let row = sqlx::query(SQL).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let n = |k: &str| row.get::<i64, _>(k);
    Ok(json!({
        "review": n("review"), "correction": n("correction"), "in_pipeline": n("in_pipeline"),
        "second_approvals": n("second_approvals"), "documents": n("documents"),
        "inquiries": n("inquiries"), "deliveries_to_approve": n("deliveries_to_approve"),
        "deliveries_blocked": n("deliveries_blocked"), "audio_advisories": n("audio_advisories"),
        "payout_requests": n("payout_requests"),
    }))
}

// ---------------------------------------------------------------------------
// Release review
// ---------------------------------------------------------------------------

/// Queue filter for [`awaiting_decision!`] (the default).
const PENDING_FILTER: &str = "PENDING";

const RELEASE_STATUSES: &[&str] = &[
    PENDING_FILTER,
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
    let status = p.status.as_deref().unwrap_or(PENDING_FILTER);
    if !RELEASE_STATUSES.contains(&status) {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    let (limit, offset) = p.bounds();
    // `$1` is PENDING (awaiting a decision) or a release status.
    const SQL: &str = concat!(
        "SELECT jsonb_build_object(
           'id', r.id, 'org_id', r.org_id, 'org_name', o.name, 'title', r.title,
           'release_type', r.release_type, 'status', r.status, 'revision_id', r.current_revision_id,
           'artist', ar.body #>> '{release,draft,artist}',
           'release_date', ar.body #>> '{release,draft,release_date}',
           'submitted_at', ar.created_at,
           'platforms', COALESCE(ar.body #> '{release,draft,platforms}', '[]'::jsonb),
           'cover', NULLIF(r.draft->>'coverData', ''),
           'agreement', (SELECT d.status FROM portal.documents d
                          WHERE d.org_id=r.org_id AND d.release_id=r.id AND d.kind='AGREEMENT'))
         FROM catalog.releases r
         JOIN identity.orgs o ON o.id=r.org_id
         LEFT JOIN catalog.application_revisions ar ON ar.org_id=r.org_id AND ar.id=r.current_revision_id
         WHERE r.archived_at IS NULL AND (r.status=$1 OR ($1='PENDING' AND ",
        awaiting_decision!(),
        "))
         ORDER BY ar.created_at NULLS LAST, r.id
         LIMIT $2 OFFSET $3"
    );
    let items: Vec<Value> = sqlx::query_scalar(SQL)
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimelinePage {
    pub limit: Option<i64>,
}

/// Everything that happened to one release, in time order, from the tables
/// that already record it: audit events (release, packages, delivery jobs,
/// jobs, track assets), jobs (migration 0064 `release_id`, plus upload-time
/// analysis of its audio), non-passing checks, staff delivery decisions,
/// DSP send attempts and ACKs. Staff read it instead of interpreting
/// internal tables; the same `release_id` finds the worker and API logs.
pub async fn release_timeline(
    s: &AppState,
    h: &HeaderMap,
    release: Uuid,
    p: TimelinePage,
) -> Result<Value> {
    staff(s, h, false).await?;
    let limit = p.limit.unwrap_or(500).clamp(1, 2000);
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    // Delivery tables are org-scoped (RLS): read them as the release's org.
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let rows: Vec<(chrono::DateTime<chrono::Utc>, String, String, Value)> = sqlx::query_as(
        "WITH pk AS (
            SELECT dp.id FROM distribution.distribution_packages dp
              JOIN distribution.canonical_releases cr ON cr.id = dp.canonical_release_id
             WHERE cr.release_id = $1),
          dj AS (SELECT id, partner_id FROM execution.delivery_jobs WHERE package_id IN (SELECT id FROM pk)),
          ast AS (SELECT DISTINCT asset_id FROM catalog.tracks WHERE release_id = $1 AND asset_id IS NOT NULL),
          jb AS (
            SELECT id, kind, queue, status, attempts, last_error, created_at, run_at
              FROM operations.jobs WHERE release_id = $1
            UNION
            SELECT id, kind, queue, status, attempts, last_error, created_at, run_at
              FROM operations.jobs
             WHERE kind = 'asset.analyze' AND payload->>'asset_id' IN (SELECT asset_id::text FROM ast))
         SELECT at, source, kind, detail FROM (
           SELECT a.occurred_at AS at, 'audit' AS source, a.action AS kind,
                  jsonb_build_object('reason', a.reason_code, 'resource_id', a.resource_id,
                                     'actor_user_id', a.actor_user_id, 'actor_service', a.actor_service,
                                     'request_id', a.request_id) AS detail
             FROM operations.audit_events a
            WHERE a.resource_id = $1
               OR a.resource_id IN (SELECT id FROM pk)
               OR a.resource_id IN (SELECT id FROM dj)
               OR a.resource_id IN (SELECT id FROM jb)
               OR a.resource_id IN (SELECT asset_id FROM ast)
           UNION ALL
           SELECT created_at, 'job', kind,
                  jsonb_build_object('job_id', id, 'queue', queue, 'status', status,
                                     'attempts', attempts, 'last_error', last_error, 'run_at', run_at)
             FROM jb
           UNION ALL
           SELECT c.created_at, 'check', c.check_code,
                  jsonb_build_object('status', c.status, 'revision_id', c.revision_id, 'detail', c.detail)
             FROM operations.check_results c
             JOIN catalog.application_revisions r ON r.id = c.revision_id
            WHERE r.release_id = $1 AND c.status NOT IN ('PASS', 'NOT_APPLICABLE')
           UNION ALL
           SELECT s.approval_at, 'staff_decision', s.dsp_code,
                  jsonb_build_object('approval', s.approval, 'readiness', s.readiness,
                                     'note', s.approval_note, 'package_id', s.package_id)
             FROM distribution.delivery_staging s
            WHERE s.release_id = $1 AND s.approval_at IS NOT NULL
           UNION ALL
           SELECT t.created_at, 'dsp_request', dj.partner_id,
                  jsonb_build_object('attempt', t.attempt_no, 'outcome', t.outcome,
                                     'partner_message_id', t.partner_message_id, 'delivery_job_id', t.job_id)
             FROM execution.delivery_attempts t JOIN dj ON dj.id = t.job_id
           UNION ALL
           SELECT e.received_at, 'dsp_ack', e.partner_id,
                  jsonb_build_object('outcome', e.outcome, 'event_id', e.event_id,
                                     'applied', e.applied, 'delivery_job_id', e.job_id)
             FROM execution.ack_events e JOIN dj ON dj.id = e.job_id
         ) t
         ORDER BY at, source, kind
         LIMIT $2",
    )
    .bind(release)
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let truncated = rows.len() as i64 > limit;
    let items: Vec<Value> = rows
        .into_iter()
        .take(limit as usize)
        .map(|(at, source, kind, detail)| json!({"at": at, "source": source, "kind": kind, "detail": detail}))
        .collect();
    Ok(json!({"release_id": release, "org_id": org, "items": items, "truncated": truncated}))
}

pub async fn release_detail(s: &AppState, h: &HeaderMap, release: Uuid) -> Result<Value> {
    staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let r = sqlx::query(
        "SELECT r.id, r.org_id, o.name AS org_name, r.title, r.release_type, r.status, r.upc,
                r.current_revision_id, ar.body AS revision, ar.created_at AS submitted_at,
                NULLIF(r.draft->>'coverData', '') AS cover
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
    // Advisory Stage 1 findings (never blocking) the reviewer should see:
    // loudness outside the delivery target, short clip events, ...
    let advisories: Vec<Value> = checks
        .iter()
        .filter(|c| {
            c["status"] == "REVIEW_REQUIRED"
                && c["check_code"]
                    .as_str()
                    .is_some_and(|code| review::STAGE1_WARNING_CODES.contains(&code))
        })
        .cloned()
        .collect();
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
            "cover": r.get::<Option<String>,_>("cover"),
        },
        "application": {
            "artist": draft["artist"], "language": draft["language"], "genre": draft["genre"],
            "release_date": draft["release_date"], "original_date": draft["originalDate"],
            "label": draft["label"], "p_line": draft["p_line"], "c_line": draft["c_line"],
            "territories": draft["territories"], "platforms": platform_codes(&draft["platforms"]),
            "options": draft["options"],
            "declarations": body["declarations"], "tracks": body["tracks"],
        },
        "signed_application": application,
        "checks": checks,
        "open_checks": open,
        "advisories": advisories,
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

/// Recorded when a reviewer approves without writing a reason.
const APPROVE_DEFAULT_REASON: &str = "담당자 승인";

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

/// Needs a second staff reviewer to PASS: the same rule as member overrides
/// (docs/REVIEW_OVERRIDES.md) — every PASS except the low-risk codes, so a
/// duplicate master, fingerprint match, protected name or rights class is
/// never cleared by one person — plus anything BLOCKED.
fn sensitive(c: &OpenCheck) -> bool {
    c.status == "BLOCKED" || review::needs_second_approver(&c.code, "PASS")
}

/// Lock the release on the revision the reviewer looked at: (org, status).
async fn locked_release(
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
    Ok((row.get("org_id"), row.get("status")))
}

async fn locked_review_release(
    c: &mut PgConnection,
    release: Uuid,
    revision: Uuid,
) -> Result<(Uuid, String)> {
    let (org, status) = locked_release(c, release, revision).await?;
    if status != "STAGE2_REVIEW" {
        return Err(Error::PolicyGate("RELEASE_NOT_IN_REVIEW"));
    }
    Ok((org, status))
}

/// The release's signed application (AGREEMENT) still waiting on staff.
async fn pending_agreement(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM portal.documents
         WHERE org_id=$1 AND release_id=$2 AND kind='AGREEMENT' AND status IN ('REVIEW','PREPARED')
         FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?)
}

/// Decide the release's agreement together with the release. `REJECTED`
/// closes every unsigned agreement of the release so it can never be signed;
/// the other outcomes only touch one still waiting on staff.
async fn decide_agreement(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    status: &str,
    note: &str,
    actor: &Actor,
) -> Result<()> {
    let note: String = note.chars().take(1000).collect();
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE portal.documents SET status=$3, review_note=$4, row_version=row_version+1, updated_at=now()
         WHERE org_id=$1 AND release_id=$2 AND kind='AGREEMENT'
           AND (status IN ('REVIEW','PREPARED') OR ($3='REJECTED' AND status NOT IN ('SIGNED','REJECTED')))
         RETURNING id",
    )
    .bind(org)
    .bind(release)
    .bind(status)
    .bind(&note)
    .fetch_all(&mut *c)
    .await?;
    for id in ids {
        operations::audit(
            c,
            Some(actor.user),
            Some(org),
            Some(id),
            "staff.document_reviewed",
            status,
            actor.request,
        )
        .await?;
    }
    Ok(())
}

/// A release through the automatic checks (READY_FOR_DELIVERY) whose signed
/// application waits on staff. Nothing has been sent (delivery waits for the
/// SIGNED agreement), so a correction or rejection only has to move the
/// release: STAGE3_CORRECTION lets the artist fix and resubmit (the new
/// application puts the agreement back in review), WITHDRAWN closes it.
async fn decide_application(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    i: &DecisionInput,
    reason: &str,
    actor: &Actor,
) -> Result<Value> {
    let (agreement, next, audit_action) = match i.action.as_str() {
        "APPROVE" => ("APPROVED", None, "staff.approved"),
        "REQUEST_CORRECTION" => (
            "NEEDS",
            Some("STAGE3_CORRECTION"),
            "staff.correction_requested",
        ),
        "REJECT" => ("REJECTED", Some("WITHDRAWN"), "staff.rejected"),
        _ => return Err(Error::InvalidCode("DECISION_ACTION_UNKNOWN")),
    };
    write_note(
        c,
        org,
        release,
        i.revision_id,
        None,
        &i.action,
        reason,
        actor.user,
    )
    .await?;
    if next.is_some() {
        for n in &i.notes {
            write_note(
                c,
                org,
                release,
                i.revision_id,
                Some(n.check_code.as_str()),
                &i.action,
                &n.note,
                actor.user,
            )
            .await?;
        }
    }
    // The approval reason is the reviewer's record; the artist only needs
    // the correction / rejection reason.
    let note = if next.is_some() { reason } else { "" };
    decide_agreement(c, org, release, agreement, note, actor).await?;
    let result = match next {
        Some(next) => {
            let moved = sqlx::query(
                "UPDATE catalog.releases SET status=$2, row_version=row_version+1
                 WHERE id=$1 AND status='READY_FOR_DELIVERY'",
            )
            .bind(release)
            .bind(next)
            .execute(&mut *c)
            .await?
            .rows_affected();
            if moved == 0 {
                return Err(Error::Conflict);
            }
            json!({"result": if next == "WITHDRAWN" { "REJECTED" } else { "APPLIED" }, "status": next})
        }
        None => json!({"result": "APPLIED", "agreement": "APPROVED"}),
    };
    let reason_code = match next {
        Some(next) => format!("READY_FOR_DELIVERY->{next}"),
        None => "APPLICATION:APPROVED".to_owned(),
    };
    operations::audit(
        c,
        Some(actor.user),
        Some(org),
        Some(release),
        audit_action,
        &reason_code,
        actor.request,
    )
    .await?;
    Ok(result)
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
    // The approval reason is optional (the reviewer's own record); a
    // correction or rejection always tells the artist why.
    let reason = match i.reason.trim() {
        "" if i.action == "APPROVE" => APPROVE_DEFAULT_REASON,
        "" => return Err(Error::PolicyGate("DECISION_REASON_REQUIRED")),
        r => r,
    };
    note_ok(reason, review::MAX_OVERRIDE_REASON_CHARS)?;
    if i.notes.len() > 100 {
        return Err(Error::Invalid);
    }
    for n in &i.notes {
        note_ok(&n.note, 2000)?;
        crate::text_policy::check(&n.check_code)?;
    }
    let mut tx = s.pool.begin().await?;
    let (org, status) = locked_release(&mut tx, release, i.revision_id).await?;
    let agreement = pending_agreement(&mut tx, org, release).await?;
    if status != "STAGE2_REVIEW" {
        if status != "READY_FOR_DELIVERY" || agreement.is_none() {
            return Err(Error::PolicyGate("RELEASE_NOT_IN_REVIEW"));
        }
        let out = decide_application(&mut tx, org, release, &i, reason, &st.actor).await?;
        tx.commit().await?;
        return Ok(out);
    }
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
                decide_agreement(&mut tx, org, release, "APPROVED", "", &st.actor).await?;
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
            decide_agreement(&mut tx, org, release, "NEEDS", reason, &st.actor).await?;
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
            decide_agreement(&mut tx, org, release, "REJECTED", reason, &st.actor).await?;
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
    decide_agreement(&mut tx, org, release, "APPROVED", "", &st.actor).await?;
    tx.commit().await?;
    Ok(json!({"result": "APPLIED", "reevaluation_queued": queued}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReissueInput {
    pub reason: String,
}

/// A READY_FOR_DELIVERY release whose frozen package carries test-range
/// (VIRTUAL) codes goes back to the artist (STAGE3_CORRECTION) once a real
/// range is registered: the resubmission's Stage 3 retires the virtual codes
/// and issues real ones (migration 0046). Refused when nothing would change
/// or when the package already reached a contracted partner.
pub async fn reissue_identifiers(
    s: &AppState,
    h: &HeaderMap,
    release: Uuid,
    i: ReissueInput,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Review)?;
    let reason = i.reason.trim();
    if reason.is_empty() {
        return Err(Error::PolicyGate("DECISION_REASON_REQUIRED"));
    }
    note_ok(reason, 2000)?;
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query(
        "SELECT r.org_id, r.status, r.current_revision_id, cr.body AS snapshot, dp.id AS package_id
         FROM catalog.releases r
         JOIN distribution.canonical_releases cr ON cr.org_id=r.org_id AND cr.revision_id=r.current_revision_id
         JOIN distribution.distribution_packages dp ON dp.canonical_release_id=cr.id
         WHERE r.id=$1 FOR UPDATE OF r",
    )
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if row.get::<String, _>("status") != "READY_FOR_DELIVERY" {
        return Err(Error::PolicyGate("RELEASE_NOT_READY_FOR_DELIVERY"));
    }
    let org: Uuid = row.get("org_id");
    let snapshot: Value = row.get("snapshot");
    use crate::identifiers::{IdentifierKind, is_virtual};
    let virtual_upc = snapshot["upc"]
        .as_str()
        .is_some_and(|u| is_virtual(IdentifierKind::Upc, u));
    let virtual_isrc = snapshot["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["isrc"].as_str())
        .any(|v| is_virtual(IdentifierKind::Isrc, v));
    if !virtual_upc && !virtual_isrc {
        return Err(Error::PolicyGate("NO_VIRTUAL_IDENTIFIERS"));
    }
    let registered: Vec<String> = sqlx::query_scalar(
        "SELECT kind FROM distribution.identifier_issuers WHERE active AND mode='REGISTERED'",
    )
    .fetch_all(&mut *tx)
    .await?;
    if (virtual_upc && !registered.iter().any(|k| k == "UPC"))
        || (virtual_isrc && !registered.iter().any(|k| k == "ISRC"))
    {
        return Err(Error::PolicyGate("REGISTERED_ISSUER_MISSING"));
    }
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *tx)
        .await?;
    let package: Uuid = row.get("package_id");
    let sent_to_partner: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.delivery_jobs j JOIN execution.adapter_profiles p ON p.partner_id=j.partner_id
                        WHERE j.org_id=$1 AND j.package_id=$2 AND p.activation_kind='CONTRACTED')",
    )
    .bind(org)
    .bind(package)
    .fetch_one(&mut *tx)
    .await?;
    if sent_to_partner {
        return Err(Error::PolicyGate("PACKAGE_ALREADY_WITH_PARTNER"));
    }
    let revision: Uuid = row.get("current_revision_id");
    sqlx::query(
        "UPDATE catalog.releases SET status='STAGE3_CORRECTION', row_version=row_version+1 WHERE id=$1 AND status='READY_FOR_DELIVERY'",
    )
    .bind(release)
    .execute(&mut *tx)
    .await?;
    write_note(
        &mut tx,
        org,
        release,
        revision,
        None,
        "REQUEST_CORRECTION",
        reason,
        st.actor.user,
    )
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "staff.identifiers_reissue",
        "READY_FOR_DELIVERY->STAGE3_CORRECTION",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"release_id": release, "status": "STAGE3_CORRECTION"}))
}

/// Cancel a release on the artist's request (an inquiry once the monthly
/// self-service limit is used up). Not counted against that limit.
pub async fn withdraw_on_request(
    s: &AppState,
    h: &HeaderMap,
    release: Uuid,
    i: ReissueInput,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Review)?;
    let reason = i.reason.trim();
    if reason.is_empty() {
        return Err(Error::PolicyGate("DECISION_REASON_REQUIRED"));
    }
    note_ok(reason, 2000)?;
    let mut tx = s.pool.begin().await?;
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    let out = crate::withdraw::withdraw(
        &mut tx,
        org,
        release,
        crate::withdraw::By::Staff,
        st.actor.user,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(out)
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
// Documents: the rights proofs staff asked for. Agreements (the signed
// release application) are decided with the release in `decide`.
// ---------------------------------------------------------------------------

pub async fn list_documents(s: &AppState, h: &HeaderMap, p: Page) -> Result<Value> {
    staff(s, h, false).await?;
    let status = p.status.as_deref().unwrap_or("REVIEW");
    if !["AWAITING_DOCUMENTS", "REVIEW", "APPROVED", "NEEDS"].contains(&status) {
        return Err(Error::InvalidCode("STATUS_UNKNOWN"));
    }
    let (limit, offset) = p.bounds();
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',d.id,'org_id',d.org_id,'org_name',o.name,'release_id',d.release_id,
                'release_title',r.title,'kind',d.kind,'title',d.title,'status',d.status,'review_note',d.review_note,
                'file_name',d.file_name,'asset_id',d.asset_id,'row_version',d.row_version,'updated_at',d.updated_at)
         FROM portal.documents d JOIN identity.orgs o ON o.id=d.org_id
         LEFT JOIN catalog.releases r ON r.id=d.release_id
         WHERE d.kind='RIGHTS_PROOF' AND d.status=$1 ORDER BY d.updated_at LIMIT $2 OFFSET $3",
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
    // Only submitted rights proofs are decided here; agreements go with the
    // release through `decide`.
    let row = sqlx::query(
        "UPDATE portal.documents SET status=$2, review_note=$3, row_version=row_version+1, updated_at=now()
         WHERE id=$1 AND row_version=$4 AND kind='RIGHTS_PROOF' AND status='REVIEW'
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
                'warnings',(SELECT COALESCE(jsonb_agg(c->>'code'),'[]'::jsonb) FROM jsonb_array_elements(s.checks) c WHERE c->>'severity'='WARNING'),
                'staged_at',s.staged_at)
         FROM distribution.delivery_staging s
         JOIN identity.orgs o ON o.id=s.org_id
         JOIN catalog.releases r ON r.org_id=s.org_id AND r.id=s.release_id
         -- Only the release's current revision: a superseded package (e.g.
         -- sent back for code re-issue) is history, not work.
         WHERE s.revision_id=r.current_revision_id
           AND s.approval=$1 AND ($2::text IS NULL OR s.readiness=$2) AND ($3::text IS NULL OR s.dsp_code=$3)
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
    // Staff see platform names; the internal code stays as the key.
    let items: Vec<Value> = items
        .into_iter()
        .map(|mut v| {
            if let Some(d) = v["dsp"].as_str().and_then(Dsp::from_code) {
                v["dsp_name"] = json!(d.display_name());
            }
            v
        })
        .collect();
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
    /// Required when the row carries audio advisories (loudness, clipping):
    /// they never block, but an operator must have seen them.
    #[serde(default)]
    pub acknowledge_warnings: bool,
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
        "SELECT s.org_id, s.release_id, s.readiness, s.ern_sha256,
                EXISTS(SELECT 1 FROM jsonb_array_elements(s.checks) c WHERE c->>'code' = ANY($3)) AS advisories,
                (r.current_revision_id = s.revision_id AND r.status='READY_FOR_DELIVERY') AS current
         FROM distribution.delivery_staging s JOIN catalog.releases r ON r.id=s.release_id
         WHERE s.package_id=$1 AND s.dsp_code=$2 FOR UPDATE OF s",
    )
    .bind(package)
    .bind(code)
    .bind(crate::delivery_staging::ACK_REQUIRED)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = row.get("org_id");
    if !row.get::<bool, _>("current") {
        return Err(Error::PolicyGate("STAGING_SUPERSEDED"));
    }
    if approval == "APPROVED" {
        if row.get::<String, _>("readiness") == "CONTENT_BLOCKED" {
            return Err(Error::PolicyGate("DELIVERY_CONTENT_BLOCKED"));
        }
        if i.ern_sha256.is_some() && i.ern_sha256 != row.get::<Option<String>, _>("ern_sha256") {
            return Err(Error::Conflict);
        }
        if row.get::<bool, _>("advisories") && !i.acknowledge_warnings {
            return Err(Error::PolicyGate("WARNINGS_NOT_ACKNOWLEDGED"));
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveEvidence {
    /// The partner's own id/URL for the release, when known.
    #[serde(default)]
    pub partner_release_id: Option<String>,
    pub note: String,
}

/// Record that a delivered release is live on a platform that never
/// reports it (evidence: the operator checked the partner catalogue). The
/// worker applies it (`delivery.mark_live`); only DELIVERED jobs qualify.
pub async fn record_live(
    s: &AppState,
    h: &HeaderMap,
    package: Uuid,
    code: &str,
    i: LiveEvidence,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    require(&st, Duty::Delivery)?;
    Dsp::from_code(code).ok_or(Error::NotFound)?;
    if i.note.trim().is_empty() {
        return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
    }
    note_ok(&i.note, 1000)?;
    let prid = i
        .partner_release_id
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    if let Some(p) = prid {
        if p.chars().count() > 300 {
            return Err(Error::InvalidCode("NOTE_TOO_LONG"));
        }
        crate::text_policy::check(p)?;
    }
    let mut tx = s.pool.begin().await?;
    let org: Uuid =
        sqlx::query_scalar("SELECT org_id FROM distribution.distribution_packages WHERE id=$1")
            .bind(package)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
    let job = operations::enqueue(
        &mut tx,
        "delivery",
        "delivery.mark_live",
        &json!({"package_id": package, "partner_id": code, "partner_release_id": prid,
                "staff_user_id": st.actor.user}),
        &format!("delivery.mark_live:{package}:{code}:{}", Uuid::new_v4()),
        None,
    )
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(package),
        "staff.delivery_live_recorded",
        &format!(
            "{code}:{}",
            i.note.trim().chars().take(200).collect::<String>()
        ),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"package_id": package, "dsp": code, "job_id": job}))
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
    let mut contract: BTreeMap<String, Value> = sqlx::query(
        "SELECT r.code, r.route, r.merlin_eligible, r.updated_by, r.updated_at,
                (SELECT NOT ('contract_signed' = ANY(m.gaps)) FROM execution.partner_readiness('merlin') m) AS merlin_signed,
                execution.platform_contract_live(r.code) AS contract_live
         FROM distribution.dsp_contract_routes r",
    )
    .fetch_all(&s.pool)
    .await?
    .into_iter()
    .map(|r| {
        (
            r.get::<String, _>("code"),
            json!({
                "route": r.get::<String, _>("route"),
                "merlin_eligible": r.get::<bool, _>("merlin_eligible"),
                "merlin_agreement_signed": r.get::<Option<bool>, _>("merlin_signed").unwrap_or(false),
                "contract_live": r.get::<bool, _>("contract_live"),
                "updated_by": r.get::<Option<String>, _>("updated_by"),
                "updated_at": r.get::<chrono::DateTime<chrono::Utc>, _>("updated_at"),
            }),
        )
    })
    .collect();
    let live_transmission = crate::launch::live_transmission_enabled();
    let items: Vec<Value> = dsp_registry::catalog()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|mut v| {
            let code = v["code"].as_str().unwrap_or("").to_owned();
            v["route"] = by_code.remove(&code).unwrap_or(Value::Null);
            v["contract_route"] = contract.remove(&code).unwrap_or(Value::Null);
            v
        })
        .collect();
    Ok(json!({"items": items, "live_transmission": live_transmission}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteChoice {
    /// DIRECT | MERLIN
    pub route: String,
    pub note: String,
}

/// ADMIN: choose a DSP's contract route (direct contract or Merlin).
pub async fn set_contract_route(
    s: &AppState,
    h: &HeaderMap,
    code: &str,
    i: RouteChoice,
) -> Result<Value> {
    let st = staff(s, h, true).await?;
    if st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    let dsp = Dsp::from_code(code).ok_or(Error::NotFound)?;
    if i.note.trim().is_empty() {
        return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
    }
    note_ok(&i.note, 1000)?;
    let route = i.route.trim().to_ascii_uppercase();
    if !matches!(route.as_str(), "DIRECT" | "MERLIN") {
        return Err(Error::InvalidCode("ROUTE_UNKNOWN"));
    }
    let mut tx = s.pool.begin().await?;
    let eligible: bool = sqlx::query_scalar(
        "SELECT merlin_eligible FROM distribution.dsp_contract_routes WHERE code=$1 FOR UPDATE",
    )
    .bind(code)
    .fetch_one(&mut *tx)
    .await?;
    if route == "MERLIN" && !eligible {
        return Err(Error::PolicyGate("MERLIN_NOT_AVAILABLE_FOR_DSP"));
    }
    let actor = st.actor.user.to_string();
    sqlx::query(
        "UPDATE distribution.dsp_contract_routes SET route=$2, updated_by=$3, updated_at=now() WHERE code=$1",
    )
    .bind(code)
    .bind(&route)
    .bind(&actor)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        None,
        None,
        "staff.dsp_contract_route",
        &format!(
            "{code}:{route}:{}",
            i.note.trim().chars().take(200).collect::<String>()
        ),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"dsp": code, "platform": dsp.display_name(), "route": route}))
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
async fn h_timeline(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(p): Query<TimelinePage>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(release_timeline(&s, &h, id, p).await?))
}
async fn h_decide(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    Ok(Json(decide(&s, &h, id, i).await?))
}
async fn h_reissue(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ReissueInput>,
) -> Result<Json<Value>> {
    Ok(Json(reissue_identifiers(&s, &h, id, i).await?))
}
async fn h_withdraw(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ReissueInput>,
) -> Result<Json<Value>> {
    Ok(Json(withdraw_on_request(&s, &h, id, i).await?))
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
async fn h_live(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((package, code)): Path<(Uuid, String)>,
    Json(i): Json<LiveEvidence>,
) -> Result<Json<Value>> {
    Ok(Json(record_live(&s, &h, package, &code, i).await?))
}
async fn h_restage(
    State(s): State<AppState>,
    Path(package): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(restage(&s, &h, package).await?))
}
async fn h_dsp_route(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(code): Path<String>,
    Json(i): Json<RouteChoice>,
) -> Result<Json<Value>> {
    Ok(Json(set_contract_route(&s, &h, &code, i).await?))
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
        .route("/api/staff/releases/{id}/timeline", get(h_timeline))
        .route("/api/staff/releases/{id}/decision", post(h_decide))
        .route(
            "/api/staff/releases/{id}/reissue-identifiers",
            post(h_reissue),
        )
        .route("/api/staff/releases/{id}/withdraw", post(h_withdraw))
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
        .route("/api/staff/deliveries/{package}/{code}/live", post(h_live))
        .route("/api/staff/dsps", get(h_dsps))
        .route("/api/staff/dsps/{code}/route", post(h_dsp_route))
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
        assert!(sensitive(&c("S2_INTEGRITY_DUP", "REVIEW_REQUIRED")));
        assert!(sensitive(&c(
            "AUDIO_SIMILAR_TO_EXISTING",
            "REVIEW_REQUIRED"
        )));
        assert!(!sensitive(&c("S2_META_CREDITS", "REVIEW_REQUIRED")));
        assert!(sensitive(&c("S2_META_CREDITS", "BLOCKED")));
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
