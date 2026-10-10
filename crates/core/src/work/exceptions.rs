//! Exception queue. Read-only adapters over the places failures already live
//! (current Stage checks, blocked delivery staging, overdue rights proofs,
//! rights holds, open reconciliation cases, failed delivery jobs, unmatched
//! royalty report lines, failing or dead-lettered automation jobs) plus problems people
//! report by hand. An item disappears by itself once its source recovers.
//!
//! `work.exception_triage` keeps only what people add, keyed by the source
//! record: assignee, acknowledgement, outcome, retries and the follow-up task.
//! RESOLVED / IGNORED hide an item until its source changes again.
//!
//! Who may act follows the source's existing duty: QC → REVIEW, rights proofs
//! → DOCUMENTS, delivery → DELIVERY, settlement and automation → ADMIN. Retry
//! goes through the existing delivery restage ([`staff::restage`]); dead
//! automation jobs are not re-run from here.
use super::{active_staff, audit, entities, line, one_of, text};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff::{self, Duty, Staff, StaffRole},
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
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::collections::HashMap;
use uuid::Uuid;

pub const SOURCES: &[&str] = &[
    "check",
    "staging",
    "document",
    "rights_hold",
    "reconciliation",
    "delivery_job",
    "report",
    "job",
    "manual",
];
pub const KINDS: &[&str] = &[
    "QC_FAILURE",
    "METADATA_CONFLICT",
    "MISSING_RIGHTS_DOCUMENT",
    "DSP_REJECTION",
    "DISTRIBUTION_FAILURE",
    "SETTLEMENT_MISMATCH",
    "AUTOMATION_FAILURE",
];
pub const SEVERITIES: &[&str] = &["CRITICAL", "HIGH", "MEDIUM", "LOW"];

/// Who may handle an exception of this kind (the source's existing duty).
pub fn may(st: &Staff, kind: &str) -> bool {
    match kind {
        "QC_FAILURE" => st.role.may(Duty::Review),
        "MISSING_RIGHTS_DOCUMENT" => st.role.may(Duty::Documents),
        "METADATA_CONFLICT" | "DSP_REJECTION" | "DISTRIBUTION_FAILURE" => {
            st.role.may(Duty::Delivery)
        }
        _ => st.role == StaffRole::Admin,
    }
}

/// Every source as one row shape. `$1` limits to one source, `$2` to one
/// source record (both NULL for the full queue).
const SQL: &str = "
(SELECT 'check' AS source, cr.id::text AS source_id, 'QC_FAILURE' AS kind,
        CASE cr.status WHEN 'BLOCKED' THEN 'HIGH' ELSE 'MEDIUM' END AS severity,
        r.title AS title, cr.check_code || COALESCE(' · ' || left(cr.detail, 300), '') AS detail,
        r.org_id, o.name AS org_name, NULL::text AS dsp, 'release' AS entity_kind, r.id::text AS entity_id,
        NULL::uuid AS package_id, cr.created_at AS detected_at
   FROM catalog.releases r JOIN identity.orgs o ON o.id=r.org_id
   JOIN LATERAL (SELECT DISTINCT ON (c.check_code) c.* FROM operations.check_results c
                  WHERE c.revision_id=r.current_revision_id ORDER BY c.check_code, c.created_at DESC) cr ON true
  WHERE ($1::text IS NULL OR $1='check') AND ($2::text IS NULL OR cr.id::text=$2)
    AND r.archived_at IS NULL AND cr.status IN ('BLOCKED','TECHNICAL_RETRY')
    AND r.status IN ('SUBMITTED','STAGE1_RUNNING','STAGE1_CORRECTION','STAGE2_RUNNING','STAGE2_REVIEW',
                     'STAGE2_CORRECTION','STAGE3_PREPARING','STAGE3_CORRECTION')
  ORDER BY cr.created_at DESC LIMIT 200)
UNION ALL
(SELECT 'staging', s.package_id::text || ':' || s.dsp_code, 'METADATA_CONFLICT', 'HIGH', r.title,
        COALESCE((SELECT string_agg(c->>'code', ', ') FROM jsonb_array_elements(s.checks) c WHERE c->>'severity'='BLOCKER'), ''),
        s.org_id, o.name, s.dsp_code, 'release', r.id::text, s.package_id, s.staged_at
   FROM distribution.delivery_staging s
   JOIN catalog.releases r ON r.id=s.release_id AND r.current_revision_id=s.revision_id
   JOIN identity.orgs o ON o.id=s.org_id
  WHERE ($1::text IS NULL OR $1='staging') AND ($2::text IS NULL OR s.package_id::text || ':' || s.dsp_code=$2)
    AND s.readiness='CONTENT_BLOCKED' AND r.status<>'WITHDRAWN'
  ORDER BY s.staged_at DESC LIMIT 200)
UNION ALL
(SELECT 'document', d.id::text, 'MISSING_RIGHTS_DOCUMENT', 'MEDIUM', d.title,
        COALESCE(r.title, '') || CASE d.status WHEN 'NEEDS' THEN ' · 보완 요청 후 응답 없음' ELSE ' · 제출 대기' END,
        d.org_id, o.name, NULL, 'document', d.id::text, NULL, d.updated_at
   FROM portal.documents d JOIN identity.orgs o ON o.id=d.org_id LEFT JOIN catalog.releases r ON r.id=d.release_id
  WHERE ($1::text IS NULL OR $1='document') AND ($2::text IS NULL OR d.id::text=$2)
    AND d.kind='RIGHTS_PROOF' AND d.status IN ('AWAITING_DOCUMENTS','NEEDS') AND d.updated_at < now() - interval '3 days'
  ORDER BY d.updated_at LIMIT 200)
UNION ALL
(SELECT 'rights_hold', r.id::text, 'MISSING_RIGHTS_DOCUMENT', 'HIGH', r.title, '권리 보류 (ON_HOLD_RIGHTS)',
        r.org_id, o.name, NULL, 'release', r.id::text, NULL,
        COALESCE((SELECT ar.created_at FROM catalog.application_revisions ar WHERE ar.org_id=r.org_id AND ar.id=r.current_revision_id), r.created_at)
   FROM catalog.releases r JOIN identity.orgs o ON o.id=r.org_id
  WHERE ($1::text IS NULL OR $1='rights_hold') AND ($2::text IS NULL OR r.id::text=$2)
    AND r.status='ON_HOLD_RIGHTS' AND r.archived_at IS NULL
  LIMIT 200)
UNION ALL
(SELECT 'reconciliation', rc.id::text,
        CASE rc.kind WHEN 'PARTNER_REJECTED' THEN 'DSP_REJECTION' ELSE 'DISTRIBUTION_FAILURE' END,
        CASE WHEN rc.kind IN ('PARTNER_REJECTED','SENT_UNKNOWN') THEN 'HIGH' ELSE 'MEDIUM' END,
        COALESCE(r.title, '배급 패키지'), rc.kind || COALESCE(' · ' || (rc.detail->>'code'), ''),
        rc.org_id, o.name, j.partner_id, CASE WHEN r.id IS NULL THEN NULL ELSE 'release' END, r.id::text, j.package_id, rc.created_at
   FROM execution.reconciliation_cases rc JOIN identity.orgs o ON o.id=rc.org_id
   LEFT JOIN execution.delivery_jobs j ON j.id=rc.job_id
   LEFT JOIN distribution.distribution_packages p ON p.id=j.package_id
   LEFT JOIN catalog.releases r ON r.id=p.canonical_release_id
  WHERE ($1::text IS NULL OR $1='reconciliation') AND ($2::text IS NULL OR rc.id::text=$2)
    AND rc.status='OPEN'
  ORDER BY rc.created_at DESC LIMIT 200)
UNION ALL
(SELECT 'delivery_job', j.id::text, 'DISTRIBUTION_FAILURE', CASE j.status WHEN 'DEAD_LETTER' THEN 'CRITICAL' ELSE 'HIGH' END,
        COALESCE(r.title, '배급 패키지'), j.status || COALESCE(' · ' || left(j.last_error, 300), ''),
        j.org_id, o.name, j.partner_id, CASE WHEN r.id IS NULL THEN NULL ELSE 'release' END, r.id::text, j.package_id, j.updated_at
   FROM execution.delivery_jobs j JOIN identity.orgs o ON o.id=j.org_id
   LEFT JOIN distribution.distribution_packages p ON p.id=j.package_id
   LEFT JOIN catalog.releases r ON r.id=p.canonical_release_id
  WHERE ($1::text IS NULL OR $1='delivery_job') AND ($2::text IS NULL OR j.id::text=$2)
    AND j.status IN ('FAILED','DEAD_LETTER')
    AND NOT EXISTS(SELECT 1 FROM execution.delivery_jobs n WHERE n.package_id=j.package_id AND n.partner_id=j.partner_id AND n.created_at>j.created_at)
    AND NOT EXISTS(SELECT 1 FROM execution.reconciliation_cases c WHERE c.job_id=j.id AND c.status='OPEN')
  ORDER BY j.updated_at DESC LIMIT 200)
UNION ALL
(SELECT 'report', rr.id::text, 'SETTLEMENT_MISMATCH', 'MEDIUM', rr.source_filename,
        '매칭되지 않은 줄 ' || u.n::text || '개', rr.org_id, o.name, rr.dsp_id, 'settlement_report', rr.id::text, NULL, rr.received_at
   FROM finance.royalty_reports rr JOIN identity.orgs o ON o.id=rr.org_id
   JOIN LATERAL (SELECT count(*) AS n FROM finance.report_lines l WHERE l.report_id=rr.id AND l.match_status='UNMATCHED') u ON u.n > 0
  WHERE ($1::text IS NULL OR $1='report') AND ($2::text IS NULL OR rr.id::text=$2)
  ORDER BY rr.received_at DESC LIMIT 200)
UNION ALL
(SELECT 'job', j.id::text, 'AUTOMATION_FAILURE', CASE j.status WHEN 'DEAD_LETTER' THEN 'HIGH' ELSE 'MEDIUM' END, j.kind,
        CASE j.status WHEN 'DEAD_LETTER' THEN '중단' ELSE '재시도 중 ' || j.attempts || '/' || j.max_attempts END
          || ' · ' || j.queue || COALESCE(' · ' || left(j.last_error, 300), ''),
        r.org_id, o.name, NULL, CASE WHEN r.id IS NULL THEN NULL ELSE 'release' END, r.id::text, NULL,
        COALESCE(j.dead_lettered_at, j.created_at)
   FROM operations.jobs j LEFT JOIN catalog.releases r ON r.id=j.release_id LEFT JOIN identity.orgs o ON o.id=r.org_id
  WHERE ($1::text IS NULL OR $1='job') AND ($2::text IS NULL OR j.id::text=$2)
    -- A job that already failed and is waiting to retry shows too: a release
    -- can read READY_FOR_DELIVERY while its delivery.stage keeps failing (BE-006).
    AND (j.status='DEAD_LETTER' OR (j.status IN ('QUEUED','RUNNING') AND j.attempts>0 AND j.last_error IS NOT NULL))
  ORDER BY j.dead_lettered_at DESC NULLS LAST, j.created_at DESC LIMIT 200)
UNION ALL
(SELECT 'manual', m.id::text, m.kind, m.severity, m.title, m.detail, NULL, NULL,
        CASE m.entity_kind WHEN 'dsp' THEN m.entity_id END, m.entity_kind, m.entity_id, NULL, m.created_at
   FROM work.exception_reports m
  WHERE ($1::text IS NULL OR $1='manual') AND ($2::text IS NULL OR m.id::text=$2)
    AND NOT EXISTS(SELECT 1 FROM work.exception_triage t WHERE t.source='manual' AND t.source_id=m.id::text AND t.status IN ('RESOLVED','IGNORED'))
  ORDER BY m.created_at DESC LIMIT 200)";

#[derive(Debug, Clone)]
pub struct Item {
    pub source: String,
    pub source_id: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub org_id: Option<Uuid>,
    pub org_name: Option<String>,
    pub dsp: Option<String>,
    pub entity_kind: Option<String>,
    pub entity_id: Option<String>,
    pub package_id: Option<Uuid>,
    pub detected_at: DateTime<Utc>,
    pub triage: Option<Triage>,
}

#[derive(Debug, Clone)]
pub struct Triage {
    pub assignee_id: Option<Uuid>,
    pub status: Option<String>,
    pub resolution: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolved_by: Option<Uuid>,
    pub retry_count: i32,
    pub last_retry_at: Option<DateTime<Utc>>,
    pub task_id: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
}

impl Triage {
    fn from_row(r: &PgRow) -> Self {
        Self {
            assignee_id: r.get("assignee_id"),
            status: r.get("status"),
            resolution: r.get("resolution"),
            resolved_at: r.get("resolved_at"),
            resolved_by: r.get("resolved_by"),
            retry_count: r.get("retry_count"),
            last_retry_at: r.get("last_retry_at"),
            task_id: r.get("task_id"),
            updated_at: r.get("updated_at"),
        }
    }
    fn json(&self) -> Value {
        json!({
            "assignee_id": self.assignee_id, "status": self.status, "resolution": self.resolution,
            "resolved_at": self.resolved_at, "resolved_by": self.resolved_by, "retry_count": self.retry_count,
            "last_retry_at": self.last_retry_at, "task_id": self.task_id, "updated_at": self.updated_at,
        })
    }
}

impl Item {
    fn from_row(r: &PgRow) -> Self {
        Self {
            source: r.get("source"),
            source_id: r.get("source_id"),
            kind: r.get("kind"),
            severity: r.get("severity"),
            title: r.get("title"),
            detail: r.get("detail"),
            org_id: r.get("org_id"),
            org_name: r.get("org_name"),
            dsp: r.get("dsp"),
            entity_kind: r.get("entity_kind"),
            entity_id: r.get("entity_id"),
            package_id: r.get("package_id"),
            detected_at: r.get("detected_at"),
            triage: None,
        }
    }
    /// Closed by a person and the source has not changed since.
    pub fn closed(&self) -> bool {
        self.triage.as_ref().is_some_and(|t| {
            matches!(t.status.as_deref(), Some("RESOLVED" | "IGNORED"))
                && t.resolved_at.is_some_and(|r| r >= self.detected_at)
        })
    }
    pub fn assignee(&self) -> Option<Uuid> {
        self.triage.as_ref().and_then(|t| t.assignee_id)
    }
    pub fn retryable(&self) -> bool {
        self.package_id.is_some()
            && matches!(
                self.source.as_str(),
                "staging" | "reconciliation" | "delivery_job"
            )
    }
    pub fn id(&self) -> String {
        format!("{}:{}", self.source, self.source_id)
    }
    pub fn json(&self, st: &Staff, sla: Option<(DateTime<Utc>, &'static str)>) -> Value {
        let dsp_name = self
            .dsp
            .as_deref()
            .and_then(crate::dsp_registry::Dsp::from_code)
            .map(|d| d.display_name());
        json!({
            "id": self.id(), "source": self.source, "source_id": self.source_id, "kind": self.kind,
            "severity": self.severity, "title": self.title, "detail": self.detail,
            "org_id": self.org_id, "org_name": self.org_name, "dsp": self.dsp, "dsp_name": dsp_name,
            "entity": self.entity_kind.as_ref().map(|k| json!({"kind": k, "id": self.entity_id})),
            "package_id": self.package_id, "detected_at": self.detected_at,
            "triage": self.triage.as_ref().map(Triage::json),
            "closed": self.closed(), "retryable": self.retryable(), "can_act": may(st, &self.kind),
            "sla": sla.map(|(due, state)| json!({"due_at": due, "state": state})),
        })
    }
}

async fn scope(c: &mut PgConnection) -> Result<()> {
    sqlx::query("SELECT set_config('app.staff','on',true)")
        .execute(&mut *c)
        .await?;
    Ok(())
}

/// Current exceptions from every source with their triage. Call inside a
/// transaction (the staff scope is transaction-local).
pub(crate) async fn collect(
    c: &mut PgConnection,
    source: Option<&str>,
    source_id: Option<&str>,
) -> Result<Vec<Item>> {
    scope(c).await?;
    let rows = sqlx::query(SQL)
        .bind(source)
        .bind(source_id)
        .fetch_all(&mut *c)
        .await?;
    let mut items: Vec<Item> = rows.iter().map(Item::from_row).collect();
    let keys: Vec<String> = items.iter().map(Item::id).collect();
    let triage: HashMap<String, Triage> = sqlx::query(
        "SELECT source || ':' || source_id AS k, assignee_id, status, resolution, resolved_at, resolved_by, retry_count,
                last_retry_at, task_id, updated_at
           FROM work.exception_triage WHERE source || ':' || source_id = ANY($1)",
    )
    .bind(&keys)
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(|r| (r.get::<String, _>("k"), Triage::from_row(r)))
    .collect();
    for i in items.iter_mut() {
        i.triage = triage.get(&i.id()).cloned();
    }
    let rank = |s: &str| SEVERITIES.iter().position(|x| *x == s).unwrap_or(9);
    items.sort_by(|a, b| {
        rank(&a.severity)
            .cmp(&rank(&b.severity))
            .then(a.detected_at.cmp(&b.detected_at))
    });
    Ok(items)
}

/// SLA of an exception: the EXCEPTION policy (a DSP's own one first).
pub(crate) fn sla_of(
    book: &super::sla::Book,
    i: &Item,
    now: DateTime<Utc>,
) -> Option<(DateTime<Utc>, &'static str)> {
    let p = book.policy("EXCEPTION", i.dsp.as_deref())?;
    let due = book.due(p, i.detected_at);
    Some((
        due,
        book.state(p.warn_ratio, i.detected_at, due, now).as_str(),
    ))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// open (default) · mine · unassigned · resolved · all
    pub view: Option<String>,
    pub kind: Option<String>,
    pub severity: Option<String>,
    pub source: Option<String>,
    pub dsp: Option<String>,
    pub q: Option<String>,
}

pub async fn list(s: &AppState, h: &HeaderMap, q: ListQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let view = q.view.as_deref().unwrap_or("open");
    one_of(
        view,
        &["open", "mine", "unassigned", "resolved", "all"],
        "VIEW_UNKNOWN",
    )?;
    let kind = q.kind.as_deref().filter(|v| !v.is_empty());
    if let Some(k) = kind {
        one_of(k, KINDS, "EXCEPTION_KIND_UNKNOWN")?;
    }
    let severity = q.severity.as_deref().filter(|v| !v.is_empty());
    if let Some(v) = severity {
        one_of(v, SEVERITIES, "SEVERITY_UNKNOWN")?;
    }
    let source = q.source.as_deref().filter(|v| !v.is_empty());
    if let Some(v) = source {
        one_of(v, SOURCES, "SOURCE_UNKNOWN")?;
    }
    let dsp = q.dsp.as_deref().filter(|v| !v.is_empty());
    let term =
        q.q.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| line(t, 80))
            .transpose()?
            .map(|t| t.to_lowercase());
    let mut tx = s.pool.begin().await?;
    let book = super::sla::Book::load(&mut tx).await?;
    if view == "resolved" {
        let rows = sqlx::query(
            "SELECT t.source, t.source_id, t.assignee_id, t.status, t.resolution, t.resolved_at, t.resolved_by, t.retry_count,
                    t.last_retry_at, t.task_id, t.updated_at, m.kind, m.title
               FROM work.exception_triage t
               LEFT JOIN work.exception_reports m ON t.source='manual' AND m.id::text=t.source_id
              WHERE t.status IN ('RESOLVED','IGNORED') ORDER BY t.resolved_at DESC LIMIT 200",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        let items: Vec<Value> = rows
            .iter()
            .map(|r| {
                let t = Triage::from_row(r);
                json!({
                    "id": format!("{}:{}", r.get::<String,_>("source"), r.get::<String,_>("source_id")),
                    "source": r.get::<String,_>("source"), "source_id": r.get::<String,_>("source_id"),
                    "kind": r.get::<Option<String>,_>("kind"), "title": r.get::<Option<String>,_>("title"),
                    "triage": t.json(),
                })
            })
            .collect();
        return Ok(json!({"items": items, "counts": {}}));
    }
    let all = collect(&mut tx, source, None).await?;
    tx.commit().await?;
    let now = Utc::now();
    let me = st.actor.user;
    let open: Vec<&Item> = all.iter().filter(|i| !i.closed()).collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for i in &open {
        *counts.entry(i.kind.as_str()).or_default() += 1;
    }
    let items: Vec<Value> = all
        .iter()
        .filter(|i| match view {
            "all" => true,
            "mine" => !i.closed() && i.assignee() == Some(me),
            "unassigned" => !i.closed() && i.assignee().is_none(),
            _ => !i.closed(),
        })
        .filter(|i| kind.is_none_or(|k| i.kind == k))
        .filter(|i| severity.is_none_or(|v| i.severity == v))
        .filter(|i| dsp.is_none_or(|d| i.dsp.as_deref() == Some(d)))
        .filter(|i| {
            term.as_deref().is_none_or(|t| {
                i.title.to_lowercase().contains(t)
                    || i.detail.to_lowercase().contains(t)
                    || i.org_name
                        .as_deref()
                        .is_some_and(|o| o.to_lowercase().contains(t))
            })
        })
        .map(|i| i.json(&st, sla_of(&book, i, now)))
        .collect();
    Ok(json!({
        "items": items,
        "counts": counts,
        "total": open.len(),
        "mine": open.iter().filter(|i| i.assignee() == Some(me)).count(),
    }))
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActInput {
    /// ASSIGN · ACK · RESOLVE · IGNORE · REOPEN · RETRY · TO_TASK
    pub action: String,
    #[serde(default, deserialize_with = "super::tasks::double_option")]
    pub assignee_id: Option<Option<Uuid>>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
}

fn task_type(kind: &str) -> &'static str {
    match kind {
        "QC_FAILURE" | "METADATA_CONFLICT" => "RELEASE",
        "MISSING_RIGHTS_DOCUMENT" => "RIGHTS",
        "DSP_REJECTION" | "DISTRIBUTION_FAILURE" => "URGENT_DELIVERY",
        "SETTLEMENT_MISMATCH" => "SETTLEMENT",
        _ => "GENERAL",
    }
}

fn task_priority(severity: &str) -> &'static str {
    match severity {
        "CRITICAL" => "URGENT",
        "HIGH" => "HIGH",
        "LOW" => "LOW",
        _ => "NORMAL",
    }
}

/// The source record's id as an audit resource, when it is a UUID.
fn resource(source_id: &str) -> Option<Uuid> {
    Uuid::parse_str(source_id.get(..36).unwrap_or(source_id)).ok()
}

async fn upsert(c: &mut PgConnection, i: &Item, me: Uuid) -> Result<()> {
    sqlx::query(
        "INSERT INTO work.exception_triage(source, source_id, updated_by) VALUES($1,$2,$3)
         ON CONFLICT (source, source_id) DO NOTHING",
    )
    .bind(&i.source)
    .bind(&i.source_id)
    .bind(me)
    .execute(&mut *c)
    .await?;
    Ok(())
}

async fn find(c: &mut PgConnection, source: &str, id: &str) -> Result<Item> {
    collect(c, Some(source), Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(Error::NotFound)
}

pub async fn act(
    s: &AppState,
    h: &HeaderMap,
    source: String,
    source_id: String,
    i: ActInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    one_of(&source, SOURCES, "SOURCE_UNKNOWN")?;
    if source_id.is_empty() || source_id.len() > 120 {
        return Err(Error::NotFound);
    }
    let me = st.actor.user;
    let action = i.action.as_str();
    one_of(
        action,
        &[
            "ASSIGN", "ACK", "RESOLVE", "IGNORE", "REOPEN", "RETRY", "TO_TASK",
        ],
        "ACTION_UNKNOWN",
    )?;
    let mut tx = s.pool.begin().await?;
    let item = find(&mut tx, &source, &source_id).await?;
    if !may(&st, &item.kind) {
        return Err(Error::Forbidden);
    }
    upsert(&mut tx, &item, me).await?;
    let res = resource(&source_id);
    match action {
        "ASSIGN" => {
            let who = i
                .assignee_id
                .ok_or(Error::InvalidCode("ASSIGNEE_REQUIRED"))?;
            active_staff(&mut tx, who).await?;
            sqlx::query(
                "UPDATE work.exception_triage SET assignee_id=$3, updated_by=$4, updated_at=now() WHERE source=$1 AND source_id=$2",
            )
            .bind(&source)
            .bind(&source_id)
            .bind(who)
            .bind(me)
            .execute(&mut *tx)
            .await?;
        }
        "ACK" => {
            sqlx::query(
                "UPDATE work.exception_triage SET status='ACKNOWLEDGED', resolved_at=NULL, resolved_by=NULL,
                        assignee_id=COALESCE(assignee_id,$3), updated_by=$3, updated_at=now()
                  WHERE source=$1 AND source_id=$2",
            )
            .bind(&source)
            .bind(&source_id)
            .bind(me)
            .execute(&mut *tx)
            .await?;
        }
        "RESOLVE" | "IGNORE" => {
            let note = i
                .resolution
                .as_deref()
                .map(|r| text(r, 1000))
                .transpose()?
                .filter(|r| !r.is_empty());
            if action == "IGNORE" && note.is_none() {
                return Err(Error::InvalidCode("RESOLUTION_REQUIRED"));
            }
            sqlx::query(
                "UPDATE work.exception_triage SET status=$3, resolution=$4, resolved_at=now(), resolved_by=$5,
                        updated_by=$5, updated_at=now()
                  WHERE source=$1 AND source_id=$2",
            )
            .bind(&source)
            .bind(&source_id)
            .bind(if action == "RESOLVE" {
                "RESOLVED"
            } else {
                "IGNORED"
            })
            .bind(note)
            .bind(me)
            .execute(&mut *tx)
            .await?;
        }
        "REOPEN" => {
            sqlx::query(
                "UPDATE work.exception_triage SET status=NULL, resolved_at=NULL, resolved_by=NULL, updated_by=$3, updated_at=now()
                  WHERE source=$1 AND source_id=$2",
            )
            .bind(&source)
            .bind(&source_id)
            .bind(me)
            .execute(&mut *tx)
            .await?;
        }
        "RETRY" => {
            if !item.retryable() {
                return Err(Error::PolicyGate("EXCEPTION_NOT_RETRYABLE"));
            }
            if item
                .triage
                .as_ref()
                .and_then(|t| t.last_retry_at)
                .is_some_and(|t| t > Utc::now() - Duration::minutes(2))
            {
                return Err(Error::PolicyGate("RETRY_TOO_SOON"));
            }
            sqlx::query(
                "UPDATE work.exception_triage SET retry_count=retry_count+1, last_retry_at=now(), last_retry_by=$3,
                        status=COALESCE(status,'ACKNOWLEDGED'), assignee_id=COALESCE(assignee_id,$3), updated_by=$3, updated_at=now()
                  WHERE source=$1 AND source_id=$2",
            )
            .bind(&source)
            .bind(&source_id)
            .bind(me)
            .execute(&mut *tx)
            .await?;
        }
        _ => {}
    }
    if action != "TO_TASK" {
        audit(
            &mut tx,
            &st,
            res,
            &format!("work.exception_{}", action.to_lowercase()),
            &format!("{}:{}", source, item.kind),
        )
        .await?;
    }
    tx.commit().await?;

    if action == "RETRY" {
        // the existing delivery restage: its own duty check, job and audit
        let package = item.package_id.ok_or(Error::NotFound)?;
        staff::restage(s, h, package).await?;
    }
    if action == "TO_TASK" {
        if let Some(t) = item.triage.as_ref().and_then(|t| t.task_id) {
            let live: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM work.tasks WHERE id=$1 AND archived_at IS NULL)",
            )
            .bind(t)
            .fetch_one(&s.pool)
            .await?;
            if live {
                return Err(Error::PolicyGate("EXCEPTION_HAS_TASK"));
            }
        }
        let title = match i.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => t.to_string(),
            None => format!("[예외] {}", item.title).chars().take(200).collect(),
        };
        let priority = i
            .priority
            .clone()
            .unwrap_or_else(|| task_priority(&item.severity).into());
        let mut links = Vec::new();
        if let (Some(k), Some(id)) = (&item.entity_kind, &item.entity_id)
            && entities::parse(k, id).is_ok()
        {
            links.push(super::tasks::LinkInput {
                entity_kind: k.clone(),
                entity_id: id.clone(),
            });
        }
        let assignee = i.assignee_id.unwrap_or(item.assignee()).or(Some(me));
        let description = format!(
            "{}\n\n출처: {} · {}{}",
            item.detail,
            item.source,
            item.source_id,
            item.org_name
                .as_deref()
                .map(|o| format!(" · {o}"))
                .unwrap_or_default()
        );
        // the board that handles this kind of problem (automation rule), else the general board
        let rule: Option<(Uuid, Uuid)> = sqlx::query_as(
            "SELECT r.board_id, r.column_id FROM work.board_rules r JOIN work.boards b ON b.id=r.board_id AND b.archived_at IS NULL
              WHERE r.trigger=$1",
        )
        .bind(&item.kind)
        .fetch_optional(&s.pool)
        .await?;
        let created = super::tasks::create(
            s,
            h,
            super::tasks::CreateInput {
                board_id: rule.map(|r| r.0),
                column_id: rule.map(|r| r.1),
                title,
                description: description.chars().take(8000).collect(),
                work_type: task_type(&item.kind).into(),
                priority,
                parent_id: None,
                assignee_id: assignee,
                reviewer_id: None,
                due_at: None,
                recurrence: None,
                links,
                tags: vec!["예외".into()],
            },
        )
        .await?;
        let task: Uuid = created["task"]["id"]
            .as_str()
            .and_then(|v| Uuid::parse_str(v).ok())
            .ok_or(Error::NotFound)?;
        let mut tx = s.pool.begin().await?;
        sqlx::query(
            "UPDATE work.exception_triage SET task_id=$3, status=COALESCE(status,'ACKNOWLEDGED'),
                    assignee_id=COALESCE(assignee_id,$4), updated_by=$5, updated_at=now()
              WHERE source=$1 AND source_id=$2",
        )
        .bind(&source)
        .bind(&source_id)
        .bind(task)
        .bind(assignee)
        .bind(me)
        .execute(&mut *tx)
        .await?;
        audit(
            &mut tx,
            &st,
            res,
            "work.exception_to_task",
            &format!("{}:{}", source, item.kind),
        )
        .await?;
        tx.commit().await?;
    }

    let mut tx = s.pool.begin().await?;
    let book = super::sla::Book::load(&mut tx).await?;
    let after = collect(&mut tx, Some(&source), Some(&source_id)).await?;
    tx.commit().await?;
    let now = Utc::now();
    Ok(json!({
        "item": after.first().map(|x| x.json(&st, sla_of(&book, x, now))),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportInput {
    pub kind: String,
    pub severity: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub entity_kind: Option<String>,
    #[serde(default)]
    pub entity_id: Option<String>,
}

/// Report a problem that has no source record (e.g. a settlement mismatch).
pub async fn report(s: &AppState, h: &HeaderMap, i: ReportInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    one_of(&i.kind, KINDS, "EXCEPTION_KIND_UNKNOWN")?;
    one_of(&i.severity, SEVERITIES, "SEVERITY_UNKNOWN")?;
    let title = line(&i.title, 300)?;
    if title.is_empty() {
        return Err(Error::InvalidCode("TITLE_REQUIRED"));
    }
    let detail = text(&i.detail, 4000)?;
    let entity = match (i.entity_kind.as_deref(), i.entity_id.as_deref()) {
        (Some(k), Some(id)) if !k.is_empty() && !id.is_empty() => {
            let r = entities::parse(k, id)?;
            if !matches!(
                r.kind.as_str(),
                "release"
                    | "org"
                    | "artist"
                    | "label"
                    | "dsp"
                    | "inquiry"
                    | "document"
                    | "payout"
                    | "settlement_report"
            ) {
                return Err(Error::InvalidCode("ENTITY_KIND_UNKNOWN"));
            }
            Some(r)
        }
        _ => None,
    };
    let mut tx = s.pool.begin().await?;
    if let Some(r) = &entity {
        entities::require(&mut tx, r).await?;
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.exception_reports(id,kind,severity,title,detail,entity_kind,entity_id,created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(id)
    .bind(&i.kind)
    .bind(&i.severity)
    .bind(&title)
    .bind(&detail)
    .bind(entity.as_ref().map(|r| r.kind.clone()))
    .bind(entity.as_ref().map(|r| r.id.clone()))
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &st, Some(id), "work.exception_reported", &i.kind).await?;
    tx.commit().await?;
    Ok(json!({ "id": format!("manual:{id}") }))
}

async fn h_list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list(&s, &h, q).await?))
}
async fn h_act(
    State(s): State<AppState>,
    Path((source, id)): Path<(String, String)>,
    h: HeaderMap,
    Json(i): Json<ActInput>,
) -> Result<Json<Value>> {
    Ok(Json(act(&s, &h, source, id, i).await?))
}
async fn h_report(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ReportInput>,
) -> Result<Json<Value>> {
    Ok(Json(report(&s, &h, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/exceptions", get(h_list))
        .route("/api/staff/work/exceptions/reports", post(h_report))
        .route("/api/staff/work/exceptions/{source}/{id}", post(h_act))
}
