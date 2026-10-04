//! AUDENIQ staff work module (`/api/staff/work/*`): a general task engine
//! (assignee, reviewer, priority, due date, status, recurrence, related
//! entities), the unified work inbox, SLA policies with automatic
//! escalation and priority raise, the exception queue the automation feeds,
//! internal notes with @mentions and attachments, a light CRM, DSP
//! operations and audit-log based work analytics.
//!
//! Every call goes through [`staff::staff`] (ACTIVE staff role + access log);
//! every mutation is audited (`work.*`). Nothing here is visible to artists.
//! SLA rules mirror `web/studio/app/src/admin/work/sla.ts` so the screen and
//! the server agree on Due Soon / Overdue.
use crate::{
    api::AppState,
    error::{Error, Result},
    operations,
    staff::{self, Staff, StaffRole},
    text_policy,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, header},
    response::Response,
    routing::{get, post},
};
use base64::Engine as _;
use chrono::{DateTime, Datelike, Duration, FixedOffset, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row, postgres::PgRow};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Vocabulary
// ---------------------------------------------------------------------------

const TASK_TYPES: &[&str] = &[
    "URGENT_DELIVERY",
    "RELEASE",
    "RIGHTS",
    "CONTRACT",
    "DSP_INQUIRY",
    "SETTLEMENT",
    "LEGAL",
    "SUPPORT",
    "GENERAL",
];
const TASK_STATUSES: &[&str] = &[
    "TODO",
    "IN_PROGRESS",
    "IN_REVIEW",
    "BLOCKED",
    "DONE",
    "CANCELLED",
];
const PRIORITIES: &[&str] = &["LOW", "NORMAL", "HIGH", "URGENT"];
const ENTITY_KINDS: &[&str] = &[
    "release",
    "org",
    "contact",
    "dsp",
    "inquiry",
    "document",
    "exception",
    "payout",
    "task",
];
const EXCEPTION_KINDS: &[&str] = &[
    "QC",
    "RIGHTS_DOCS",
    "DSP_REJECTION",
    "METADATA_CONFLICT",
    "SETTLEMENT_MISMATCH",
    "DELIVERY_FAILURE",
];
const SEVERITIES: &[&str] = &["LOW", "MEDIUM", "HIGH", "CRITICAL"];
const CONTACT_KINDS: &[&str] = &["ARTIST", "LABEL", "DSP", "PARTNER"];
const CONTRACT_STAGES: &[&str] = &["NONE", "NEGOTIATING", "SENT", "SIGNED", "RENEWAL", "ENDED"];
const ACTIVITY_KINDS: &[&str] = &["CALL", "EMAIL", "MEETING", "NOTE"];
const HEALTHS: &[&str] = &["HEALTHY", "DEGRADED", "DOWN", "ONBOARDING"];
const ROLES: &[&str] = &["ADMIN", "REVIEWER", "OPERATOR", "SUPPORT"];

const MAX_UPLOAD: i32 = 2 * 1024 * 1024;
const MAX_PART: usize = 48 * 1024;
const MAX_ATTACHMENTS: usize = 3;

fn one_of(v: &str, set: &[&str], code: &'static str) -> Result<()> {
    if set.contains(&v) {
        Ok(())
    } else {
        Err(Error::InvalidCode(code))
    }
}

fn line(s: &str, max: usize) -> Result<String> {
    let t = s.trim();
    if t.chars().count() > max {
        return Err(Error::InvalidCode("TEXT_TOO_LONG"));
    }
    text_policy::check(t)?;
    Ok(t.to_string())
}

fn text(s: &str, max: usize) -> Result<String> {
    let t = s.trim();
    if t.chars().count() > max {
        return Err(Error::InvalidCode("TEXT_TOO_LONG"));
    }
    text_policy::check_multiline(t)?;
    Ok(t.to_string())
}

fn tags(v: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for t in v.iter().take(8) {
        let t = line(t, 30)?;
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityRef {
    pub kind: String,
    pub id: String,
    pub label: String,
}

fn related(v: &[EntityRef]) -> Result<Value> {
    if v.len() > 20 {
        return Err(Error::InvalidCode("TOO_MANY_RELATED"));
    }
    let mut out: Vec<EntityRef> = Vec::new();
    for r in v {
        one_of(&r.kind, ENTITY_KINDS, "RELATED_KIND_UNKNOWN")?;
        let id = line(&r.id, 80)?;
        if id.is_empty() {
            return Err(Error::InvalidCode("RELATED_ID_REQUIRED"));
        }
        let label = line(&r.label, 200)?;
        if !out.iter().any(|x| x.kind == r.kind && x.id == id) {
            out.push(EntityRef {
                kind: r.kind.clone(),
                id,
                label,
            });
        }
    }
    serde_json::to_value(out).map_err(|_| Error::Internal)
}

/// Assignee / reviewer / owner must be an ACTIVE staff member.
async fn active_staff(c: &mut PgConnection, user: Option<Uuid>) -> Result<()> {
    let Some(u) = user else { return Ok(()) };
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM identity.staff_members WHERE user_id=$1 AND status='ACTIVE')",
    )
    .bind(u)
    .fetch_one(&mut *c)
    .await?;
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidCode("NOT_STAFF"))
    }
}

async fn staff_scope(c: &mut PgConnection) -> Result<()> {
    sqlx::query("SELECT set_config('app.staff','on',true)")
        .execute(&mut *c)
        .await?;
    Ok(())
}

async fn audit(
    c: &mut PgConnection,
    st: &Staff,
    resource: Option<Uuid>,
    action: &str,
    reason: &str,
) -> Result<()> {
    operations::audit(
        c,
        Some(st.actor.user),
        None,
        resource,
        action,
        reason,
        st.actor.request,
    )
    .await
}

// ---------------------------------------------------------------------------
// SLA (same rules as the admin screen)
// ---------------------------------------------------------------------------

fn kst() -> FixedOffset {
    FixedOffset::east_opt(9 * 3600).expect("valid offset")
}
const OPEN_H: u32 = 9;
const CLOSE_H: u32 = 18;

fn at_hour(d: DateTime<FixedOffset>, h: u32) -> DateTime<Utc> {
    kst()
        .with_ymd_and_hms(d.year(), d.month(), d.day(), h, 0, 0)
        .single()
        .expect("fixed offset is unambiguous")
        .with_timezone(&Utc)
}
fn weekend(d: DateTime<FixedOffset>) -> bool {
    matches!(d.weekday(), Weekday::Sat | Weekday::Sun)
}
fn next_open(d: DateTime<FixedOffset>) -> DateTime<Utc> {
    at_hour(
        (at_hour(d, 0) + Duration::days(1)).with_timezone(&kst()),
        OPEN_H,
    )
}

/// `start` plus `minutes` of business time (weekdays 09–18, Korea time).
pub fn add_business_minutes(start: DateTime<Utc>, minutes: i64) -> DateTime<Utc> {
    let mut t = start;
    let mut left = Duration::minutes(minutes);
    for _ in 0..4000 {
        let d = t.with_timezone(&kst());
        let open = at_hour(d, OPEN_H);
        let close = at_hour(d, CLOSE_H);
        if weekend(d) || t >= close {
            t = next_open(d);
            continue;
        }
        if t < open {
            t = open;
        }
        let room = close - t;
        if left <= room {
            return t + left;
        }
        left -= room;
        t = close;
    }
    t + left
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlaPolicy {
    #[serde(rename = "type")]
    pub kind: String,
    pub target_minutes: i32,
    pub business_hours: bool,
    pub warn_ratio: f32,
    pub idle_alert_minutes: i32,
    pub escalate_to: String,
    pub auto_raise_priority: bool,
    pub enabled: bool,
}

pub fn sla_due(created: DateTime<Utc>, p: Option<&SlaPolicy>) -> Option<DateTime<Utc>> {
    let p = p.filter(|p| p.enabled)?;
    Some(if p.business_hours {
        add_business_minutes(created, p.target_minutes as i64)
    } else {
        created + Duration::minutes(p.target_minutes as i64)
    })
}

fn effective_due(due: Option<DateTime<Utc>>, sla: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (due, sla) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

pub fn sla_state(
    status: &str,
    created: DateTime<Utc>,
    completed: Option<DateTime<Utc>>,
    due: Option<DateTime<Utc>>,
    warn_ratio: f32,
    now: DateTime<Utc>,
) -> &'static str {
    let Some(due) = due else { return "NONE" };
    match status {
        "CANCELLED" => return "NONE",
        "DONE" => {
            return if completed.is_some_and(|c| c <= due) {
                "MET"
            } else {
                "MISSED"
            };
        }
        _ => {}
    }
    if now > due {
        return "OVERDUE";
    }
    let total = (due - created).num_seconds() as f64;
    let left = (due - now).num_seconds() as f64;
    if total > 0.0 && left <= total * warn_ratio as f64 {
        "DUE_SOON"
    } else {
        "OK"
    }
}

fn raise(p: &str) -> &'static str {
    match p {
        "LOW" => "NORMAL",
        "NORMAL" => "HIGH",
        _ => "URGENT",
    }
}
fn rank(p: &str) -> usize {
    PRIORITIES.iter().position(|x| *x == p).unwrap_or(1)
}

/// Escalation level and effective priority for an SLA state.
pub fn escalate(state: &str, priority: &str, auto_raise: bool) -> (i16, String) {
    match state {
        "OVERDUE" => (
            2,
            if auto_raise {
                "URGENT".into()
            } else {
                priority.into()
            },
        ),
        "DUE_SOON" => (
            1,
            if auto_raise {
                raise(priority).into()
            } else {
                priority.into()
            },
        ),
        _ => (0, priority.into()),
    }
}

async fn policies(c: &mut PgConnection) -> Result<Vec<SlaPolicy>> {
    let rows = sqlx::query(
        "SELECT type,target_minutes,business_hours,warn_ratio,idle_alert_minutes,escalate_to,auto_raise_priority,enabled
           FROM work.sla_policies ORDER BY array_position($1::text[], type)",
    )
    .bind(TASK_TYPES)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| SlaPolicy {
            kind: r.get("type"),
            target_minutes: r.get("target_minutes"),
            business_hours: r.get("business_hours"),
            warn_ratio: r.get("warn_ratio"),
            idle_alert_minutes: r.get("idle_alert_minutes"),
            escalate_to: r.get("escalate_to"),
            auto_raise_priority: r.get("auto_raise_priority"),
            enabled: r.get("enabled"),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

macro_rules! task_cols {
    () => {
        "t.id,t.seq,t.title,t.description,t.type,t.status,t.priority,t.priority_raised,t.assignee_id,t.reviewer_id,
  t.due_at,t.sla_due_at,t.escalation_level,t.changes_requested,t.recurrence,t.related,t.tags,t.created_by,t.created_at,t.updated_at,
  t.assigned_at,t.started_at,t.completed_at,t.row_version,
  (SELECT count(*) FROM work.notes n WHERE n.target_kind='task' AND n.target_id=t.id::text) AS note_count"
    };
}

#[derive(Debug, Clone)]
struct Task {
    id: Uuid,
    seq: i64,
    title: String,
    description: String,
    kind: String,
    status: String,
    priority: String,
    priority_raised: Option<String>,
    assignee_id: Option<Uuid>,
    reviewer_id: Option<Uuid>,
    due_at: Option<DateTime<Utc>>,
    sla_due_at: Option<DateTime<Utc>>,
    escalation_level: i16,
    changes_requested: bool,
    recurrence: Option<Value>,
    related: Value,
    tags: Vec<String>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    assigned_at: Option<DateTime<Utc>>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    row_version: i32,
    note_count: i64,
}

impl Task {
    fn from_row(r: &PgRow) -> Self {
        Self {
            id: r.get("id"),
            seq: r.get("seq"),
            title: r.get("title"),
            description: r.get("description"),
            kind: r.get("type"),
            status: r.get("status"),
            priority: r.get("priority"),
            priority_raised: r.get("priority_raised"),
            assignee_id: r.get("assignee_id"),
            reviewer_id: r.get("reviewer_id"),
            due_at: r.get("due_at"),
            sla_due_at: r.get("sla_due_at"),
            escalation_level: r.get("escalation_level"),
            changes_requested: r.get("changes_requested"),
            recurrence: r.get("recurrence"),
            related: r.get("related"),
            tags: r.get("tags"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            assigned_at: r.get("assigned_at"),
            started_at: r.get("started_at"),
            completed_at: r.get("completed_at"),
            row_version: r.get("row_version"),
            note_count: r.get("note_count"),
        }
    }
    fn open(&self) -> bool {
        !matches!(self.status.as_str(), "DONE" | "CANCELLED")
    }
    fn due(&self) -> Option<DateTime<Utc>> {
        effective_due(self.due_at, self.sla_due_at)
    }
    fn state(&self, pol: &[SlaPolicy], now: DateTime<Utc>) -> &'static str {
        let warn = pol
            .iter()
            .find(|p| p.kind == self.kind)
            .map_or(0.25, |p| p.warn_ratio);
        sla_state(
            &self.status,
            self.created_at,
            self.completed_at,
            self.due(),
            warn,
            now,
        )
    }
    fn json(&self, pol: &[SlaPolicy], now: DateTime<Utc>) -> Value {
        let eff = self
            .priority_raised
            .clone()
            .unwrap_or_else(|| self.priority.clone());
        json!({
            "id": self.id, "key": format!("WRK-{}", self.seq), "title": self.title, "description": self.description,
            "type": self.kind, "status": self.status, "priority": eff,
            "priority_raised_from": self.priority_raised.as_ref().map(|_| self.priority.clone()),
            "assignee_id": self.assignee_id, "reviewer_id": self.reviewer_id,
            "due_at": self.due_at, "sla_due_at": self.sla_due_at, "sla_state": self.state(pol, now),
            "escalation_level": self.escalation_level, "recurrence": self.recurrence, "related": self.related,
            "tags": self.tags, "created_by": self.created_by, "created_at": self.created_at, "updated_at": self.updated_at,
            "assigned_at": self.assigned_at, "started_at": self.started_at, "completed_at": self.completed_at,
            "note_count": self.note_count, "row_version": self.row_version,
            "changes_requested": self.changes_requested,
        })
    }
}

async fn event(
    c: &mut PgConnection,
    task: Uuid,
    actor: Option<Uuid>,
    action: &str,
    detail: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO work.task_events(id,task_id,actor_id,action,detail) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(task)
    .bind(actor)
    .bind(action)
    .bind(detail.map(|d| d.chars().take(2000).collect::<String>()))
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// Apply automatic escalation to open tasks whose SLA state moved: Due Soon
/// raises the priority one step (level 1), Overdue makes it URGENT and
/// escalates to the policy's role (level 2). Idempotent; recorded once per
/// level as a task event.
async fn sweep(c: &mut PgConnection, pol: &[SlaPolicy], now: DateTime<Utc>) -> Result<()> {
    let rows = sqlx::query(concat!("SELECT ", task_cols!(), " FROM work.tasks t WHERE t.status NOT IN ('DONE','CANCELLED')
           AND COALESCE(t.due_at,t.sla_due_at) IS NOT NULL ORDER BY LEAST(t.due_at,t.sla_due_at) LIMIT 2000"))
    .fetch_all(&mut *c)
    .await?;
    for r in rows {
        let t = Task::from_row(&r);
        let p = pol.iter().find(|p| p.kind == t.kind);
        let state = t.state(pol, now);
        let (level, prio) = escalate(state, &t.priority, p.is_some_and(|p| p.auto_raise_priority));
        if level <= t.escalation_level {
            continue;
        }
        let raised = (prio != t.priority).then_some(prio.clone());
        sqlx::query("UPDATE work.tasks SET escalation_level=$2, priority_raised=$3, row_version=row_version+1 WHERE id=$1")
            .bind(t.id)
            .bind(level)
            .bind(&raised)
            .execute(&mut *c)
            .await?;
        if raised.is_some() && raised != t.priority_raised {
            event(c, t.id, None, "PRIORITY_RAISED", Some(&prio)).await?;
        }
        if level == 2 {
            let to = p.map_or("ADMIN", |p| p.escalate_to.as_str());
            event(c, t.id, None, "ESCALATED", Some(to)).await?;
        }
    }
    Ok(())
}

async fn load_task(c: &mut PgConnection, id: Uuid, lock: bool) -> Result<Task> {
    let q = if lock {
        sqlx::query(concat!(
            "SELECT ",
            task_cols!(),
            " FROM work.tasks t WHERE t.id=$1 FOR UPDATE OF t"
        ))
    } else {
        sqlx::query(concat!(
            "SELECT ",
            task_cols!(),
            " FROM work.tasks t WHERE t.id=$1"
        ))
    };
    let r = q
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Task::from_row(&r))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskQuery {
    pub view: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub assignee: Option<Uuid>,
    pub q: Option<String>,
    pub related_kind: Option<String>,
    pub related_id: Option<String>,
}

pub async fn list_tasks(s: &AppState, h: &HeaderMap, q: TaskQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let me = st.actor.user;
    let mut tx = s.pool.begin().await?;
    let pol = policies(&mut tx).await?;
    let now = Utc::now();
    sweep(&mut tx, &pol, now).await?;
    let view = q.view.as_deref().unwrap_or("mine");
    one_of(view, &["mine", "review", "created", "all"], "VIEW_UNKNOWN")?;
    let status = q.status.as_deref().unwrap_or("open");
    one_of(status, &["open", "done", "all"], "STATUS_UNKNOWN")?;
    if let Some(k) = q.kind.as_deref().filter(|k| !k.is_empty()) {
        one_of(k, TASK_TYPES, "TASK_TYPE_UNKNOWN")?;
    }
    let term =
        q.q.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(80).collect::<String>());
    let rel = match (q.related_kind.as_deref(), q.related_id.as_deref()) {
        (Some(k), Some(i)) => {
            one_of(k, ENTITY_KINDS, "RELATED_KIND_UNKNOWN")?;
            Some(json!([{"kind": k, "id": i}]))
        }
        _ => None,
    };
    let rows = sqlx::query(concat!("SELECT ", task_cols!(), " FROM work.tasks t
          WHERE ($1='all' OR ($1='mine' AND t.assignee_id=$2) OR ($1='review' AND t.reviewer_id=$2) OR ($1='created' AND t.created_by=$2))
            AND ($3='all' OR ($3='open' AND t.status NOT IN ('DONE','CANCELLED')) OR ($3='done' AND t.status IN ('DONE','CANCELLED')))
            AND ($4::text IS NULL OR $4='' OR t.type=$4)
            AND ($5::uuid IS NULL OR t.assignee_id=$5)
            AND ($6::text IS NULL OR t.title ILIKE '%'||$6||'%' OR t.description ILIKE '%'||$6||'%' OR ('WRK-'||t.seq)=upper($6) OR $6=ANY(t.tags))
            AND ($7::jsonb IS NULL OR t.related @> $7)
          ORDER BY (t.status IN ('DONE','CANCELLED')), array_position($8::text[], COALESCE(t.priority_raised,t.priority)) DESC,
                   LEAST(t.due_at,t.sla_due_at) NULLS LAST, t.created_at DESC
          LIMIT 300"))
    .bind(view)
    .bind(me)
    .bind(status)
    .bind(q.kind.as_deref())
    .bind(q.assignee)
    .bind(term)
    .bind(rel)
    .bind(PRIORITIES)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| Task::from_row(r).json(&pol, now))
        .collect();
    Ok(json!({ "items": items }))
}

pub async fn get_task(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    let pol = policies(&mut tx).await?;
    let now = Utc::now();
    sweep(&mut tx, &pol, now).await?;
    let t = load_task(&mut tx, id, false).await?;
    let events = sqlx::query(
        "SELECT at,actor_id,action,detail FROM work.task_events WHERE task_id=$1 ORDER BY at DESC, id LIMIT 200",
    )
    .bind(id)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let events: Vec<Value> = events
        .iter()
        .map(|r| {
            json!({"at": r.get::<DateTime<Utc>,_>("at"), "actor_id": r.get::<Option<Uuid>,_>("actor_id"),
                   "action": r.get::<String,_>("action"), "detail": r.get::<Option<String>,_>("detail")})
        })
        .collect();
    Ok(json!({ "task": t.json(&pol, now), "events": events }))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    pub freq: String,
    pub interval: i32,
    #[serde(default)]
    pub next_at: Option<DateTime<Utc>>,
}

fn recurrence(r: Option<&Recurrence>, from: DateTime<Utc>) -> Result<Option<Value>> {
    let Some(r) = r else { return Ok(None) };
    one_of(
        &r.freq,
        &["DAILY", "WEEKLY", "MONTHLY"],
        "RECURRENCE_UNKNOWN",
    )?;
    if !(1..=12).contains(&r.interval) {
        return Err(Error::InvalidCode("RECURRENCE_INTERVAL"));
    }
    Ok(Some(
        json!({"freq": r.freq, "interval": r.interval, "next_at": next_occurrence(&r.freq, r.interval, from)}),
    ))
}

fn next_occurrence(freq: &str, interval: i32, from: DateTime<Utc>) -> DateTime<Utc> {
    match freq {
        "DAILY" => from + Duration::days(interval as i64),
        "WEEKLY" => from + Duration::weeks(interval as i64),
        _ => from
            .checked_add_months(chrono::Months::new(interval as u32))
            .unwrap_or(from + Duration::days(30 * interval as i64)),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub priority: String,
    #[serde(default)]
    pub assignee_id: Option<Uuid>,
    #[serde(default)]
    pub reviewer_id: Option<Uuid>,
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    #[serde(default)]
    pub related: Vec<EntityRef>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn create_task(s: &AppState, h: &HeaderMap, i: TaskInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let title = line(&i.title, 200)?;
    if title.is_empty() {
        return Err(Error::InvalidCode("TITLE_REQUIRED"));
    }
    let description = text(&i.description, 8000)?;
    one_of(&i.kind, TASK_TYPES, "TASK_TYPE_UNKNOWN")?;
    one_of(&i.priority, PRIORITIES, "PRIORITY_UNKNOWN")?;
    let rel = related(&i.related)?;
    let tg = tags(&i.tags)?;
    let now = Utc::now();
    let rec = recurrence(i.recurrence.as_ref(), now)?;
    let mut tx = s.pool.begin().await?;
    active_staff(&mut tx, i.assignee_id).await?;
    active_staff(&mut tx, i.reviewer_id).await?;
    let pol = policies(&mut tx).await?;
    let sla = sla_due(now, pol.iter().find(|p| p.kind == i.kind));
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.tasks(id,title,description,type,priority,assignee_id,reviewer_id,due_at,sla_due_at,recurrence,related,tags,created_by,created_at,updated_at,assigned_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$14,CASE WHEN $6::uuid IS NULL THEN NULL ELSE $14 END)",
    )
    .bind(id).bind(&title).bind(&description).bind(&i.kind).bind(&i.priority).bind(i.assignee_id).bind(i.reviewer_id)
    .bind(i.due_at).bind(sla).bind(rec).bind(rel).bind(&tg).bind(st.actor.user).bind(now)
    .execute(&mut *tx)
    .await?;
    event(&mut tx, id, Some(st.actor.user), "CREATED", None).await?;
    audit(&mut tx, &st, Some(id), "work.task_created", &i.kind).await?;
    let t = load_task(&mut tx, id, false).await?;
    tx.commit().await?;
    Ok(t.json(&pol, now))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskPatch {
    pub row_version: i32,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub priority: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub assignee_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub reviewer_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub due_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub recurrence: Option<Option<Recurrence>>,
    pub related: Option<Vec<EntityRef>>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    pub review: Option<String>,
    pub review_note: Option<String>,
}

/// `null` (clear) vs absent (leave as is).
fn double_option<'de, T, D>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

pub async fn update_task(s: &AppState, h: &HeaderMap, id: Uuid, p: TaskPatch) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let me = st.actor.user;
    let now = Utc::now();
    let mut tx = s.pool.begin().await?;
    let pol = policies(&mut tx).await?;
    let mut t = load_task(&mut tx, id, true).await?;
    if t.row_version != p.row_version {
        return Err(Error::Conflict);
    }
    let mut events: Vec<(&'static str, Option<String>)> = Vec::new();

    if let Some(v) = &p.title {
        let v = line(v, 200)?;
        if v.is_empty() {
            return Err(Error::InvalidCode("TITLE_REQUIRED"));
        }
        if v != t.title {
            t.title = v;
            events.push(("EDITED", None));
        }
    }
    if let Some(v) = &p.description {
        let v = text(v, 8000)?;
        if v != t.description {
            t.description = v;
            if !events.iter().any(|e| e.0 == "EDITED") {
                events.push(("EDITED", None));
            }
        }
    }
    if let Some(v) = &p.kind {
        one_of(v, TASK_TYPES, "TASK_TYPE_UNKNOWN")?;
        if *v != t.kind {
            t.kind = v.clone();
            t.sla_due_at = sla_due(t.created_at, pol.iter().find(|x| x.kind == t.kind));
            t.escalation_level = 0;
            t.priority_raised = None;
        }
    }
    if let Some(v) = &p.priority {
        one_of(v, PRIORITIES, "PRIORITY_UNKNOWN")?;
        // a person choosing the priority clears the SLA raise
        if *v != t.priority || t.priority_raised.is_some() {
            t.priority = v.clone();
            t.priority_raised = None;
            events.push(("PRIORITY", Some(v.clone())));
        }
    }
    if let Some(v) = p.assignee_id {
        active_staff(&mut tx, v).await?;
        if v != t.assignee_id {
            t.assignee_id = v;
            t.assigned_at = v.map(|_| now);
            let name = match v {
                Some(u) => {
                    sqlx::query_scalar::<_, String>(
                        "SELECT split_part(email,'@',1) FROM identity.users WHERE id=$1",
                    )
                    .bind(u)
                    .fetch_optional(&mut *tx)
                    .await?
                }
                None => None,
            };
            events.push(("ASSIGNED", name));
        }
    }
    if let Some(v) = p.reviewer_id {
        active_staff(&mut tx, v).await?;
        t.reviewer_id = v;
    }
    if let Some(v) = p.due_at
        && v != t.due_at
    {
        t.due_at = v;
        events.push(("DUE", v.map(|d| d.to_rfc3339())));
    }
    if let Some(v) = &p.recurrence {
        t.recurrence = recurrence(v.as_ref(), now)?;
    }
    if let Some(v) = &p.related {
        t.related = related(v)?;
    }
    if let Some(v) = &p.tags {
        t.tags = tags(v)?;
    }

    // Reviewer decision on a task waiting in review.
    let mut recur = false;
    if let Some(r) = p.review.as_deref() {
        if t.status != "IN_REVIEW" {
            return Err(Error::PolicyGate("TASK_NOT_IN_REVIEW"));
        }
        if t.reviewer_id != Some(me) && st.role != StaffRole::Admin {
            return Err(Error::Forbidden);
        }
        let note = p
            .review_note
            .as_deref()
            .map(|n| text(n, 2000))
            .transpose()?
            .filter(|n| !n.is_empty());
        match r {
            "APPROVE" => {
                t.status = "DONE".into();
                t.completed_at = Some(now);
                t.changes_requested = false;
                recur = true;
                events.push(("APPROVED", note));
            }
            "REQUEST_CHANGES" => {
                if note.is_none() {
                    return Err(Error::InvalidCode("REVIEW_NOTE_REQUIRED"));
                }
                t.status = "IN_PROGRESS".into();
                t.changes_requested = true;
                events.push(("CHANGES_REQUESTED", note));
            }
            _ => return Err(Error::InvalidCode("REVIEW_UNKNOWN")),
        }
    } else if let Some(next) = p.status.as_deref() {
        one_of(next, TASK_STATUSES, "STATUS_UNKNOWN")?;
        if next != t.status {
            let mut next = next.to_string();
            // With a reviewer (someone else), "done" asks for review first.
            if next == "DONE" && t.status != "IN_REVIEW" && t.reviewer_id.is_some_and(|r| r != me) {
                next = "IN_REVIEW".into();
            }
            if next == "IN_REVIEW" && t.reviewer_id.is_none() {
                return Err(Error::PolicyGate("TASK_NO_REVIEWER"));
            }
            if t.status == "IN_REVIEW"
                && next == "DONE"
                && t.reviewer_id.is_some_and(|r| r != me)
                && st.role != StaffRole::Admin
            {
                return Err(Error::PolicyGate("TASK_REVIEW_PENDING"));
            }
            match next.as_str() {
                "IN_PROGRESS" => {
                    t.started_at.get_or_insert(now);
                    t.completed_at = None;
                }
                "TODO" | "BLOCKED" => t.completed_at = None,
                "IN_REVIEW" => t.changes_requested = false,
                "DONE" => {
                    t.completed_at = Some(now);
                    t.changes_requested = false;
                    recur = true;
                }
                "CANCELLED" => t.completed_at = Some(now),
                _ => {}
            }
            events.push((
                if next == "IN_REVIEW" {
                    "REVIEW_REQUESTED"
                } else {
                    "STATUS"
                },
                Some(next.clone()),
            ));
            t.status = next;
        }
    }

    sqlx::query(
        "UPDATE work.tasks SET title=$2,description=$3,type=$4,status=$5,priority=$6,priority_raised=$7,assignee_id=$8,reviewer_id=$9,
            due_at=$10,sla_due_at=$11,escalation_level=$12,changes_requested=$13,recurrence=$14,related=$15,tags=$16,assigned_at=$17,
            started_at=$18,completed_at=$19,updated_at=$20,row_version=row_version+1
          WHERE id=$1",
    )
    .bind(t.id).bind(&t.title).bind(&t.description).bind(&t.kind).bind(&t.status).bind(&t.priority).bind(&t.priority_raised)
    .bind(t.assignee_id).bind(t.reviewer_id).bind(t.due_at).bind(t.sla_due_at).bind(t.escalation_level).bind(t.changes_requested)
    .bind(&t.recurrence).bind(&t.related).bind(&t.tags).bind(t.assigned_at).bind(t.started_at).bind(t.completed_at).bind(now)
    .execute(&mut *tx)
    .await?;
    for (a, d) in &events {
        event(&mut tx, t.id, Some(me), a, d.as_deref()).await?;
    }

    // Recurring task done → the next one, same people and links.
    if recur
        && t.status == "DONE"
        && let Some(r) = t.recurrence.clone()
    {
        let freq = r["freq"].as_str().unwrap_or("WEEKLY").to_string();
        let interval = r["interval"].as_i64().unwrap_or(1) as i32;
        let next_id = Uuid::new_v4();
        let due = t.due_at.map(|d| next_occurrence(&freq, interval, d));
        let sla = sla_due(now, pol.iter().find(|x| x.kind == t.kind));
        let rec = json!({"freq": freq, "interval": interval, "next_at": next_occurrence(&freq, interval, now)});
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO work.tasks(id,title,description,type,priority,assignee_id,reviewer_id,due_at,sla_due_at,recurrence,related,tags,created_by,created_at,updated_at,assigned_at,recurred_from)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$14,CASE WHEN $6::uuid IS NULL THEN NULL ELSE $14 END,$15) RETURNING seq",
        )
        .bind(next_id).bind(&t.title).bind(&t.description).bind(&t.kind).bind(&t.priority).bind(t.assignee_id).bind(t.reviewer_id)
        .bind(due).bind(sla).bind(rec).bind(&t.related).bind(&t.tags).bind(me).bind(now).bind(t.id)
        .fetch_one(&mut *tx)
        .await?;
        event(
            &mut tx,
            next_id,
            Some(me),
            "CREATED",
            Some(&format!("WRK-{} 반복", t.seq)),
        )
        .await?;
        event(
            &mut tx,
            t.id,
            Some(me),
            "RECURRED",
            Some(&format!("WRK-{seq}")),
        )
        .await?;
    }
    audit(
        &mut tx,
        &st,
        Some(t.id),
        "work.task_updated",
        events.first().map_or("EDIT", |e| e.0),
    )
    .await?;
    let t = load_task(&mut tx, t.id, false).await?;
    tx.commit().await?;
    Ok(t.json(&pol, now))
}

// ---------------------------------------------------------------------------
// People
// ---------------------------------------------------------------------------

pub async fn people(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let rows = sqlx::query(
        "SELECT sm.user_id, u.email, sm.role FROM identity.staff_members sm JOIN identity.users u ON u.id=sm.user_id
          WHERE sm.status='ACTIVE' AND u.status='ACTIVE' ORDER BY u.email",
    )
    .fetch_all(&s.pool)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let email: String = r.get("email");
            let name = email.split('@').next().unwrap_or("").to_string();
            json!({"user_id": r.get::<Uuid,_>("user_id"), "email": email, "name": name, "role": r.get::<String,_>("role")})
        })
        .collect();
    Ok(json!({ "items": items }))
}

// ---------------------------------------------------------------------------
// Inbox
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct InboxItem {
    id: String,
    kind: &'static str,
    title: String,
    summary: String,
    priority: String,
    due_at: Option<DateTime<Utc>>,
    at: DateTime<Utc>,
    link: String,
    read: bool,
    #[serde(rename = "ref")]
    refer: Option<Value>,
}

async fn build_inbox(
    c: &mut PgConnection,
    st: &Staff,
    pol: &[SlaPolicy],
    now: DateTime<Utc>,
) -> Result<Vec<InboxItem>> {
    let me = st.actor.user;
    let role = st.role.as_str();
    let mut out: Vec<InboxItem> = Vec::new();

    let rows = sqlx::query(concat!("SELECT ", task_cols!(), " FROM work.tasks t WHERE t.status NOT IN ('DONE','CANCELLED')
           AND (t.assignee_id=$1 OR t.reviewer_id=$1 OR EXISTS(SELECT 1 FROM work.sla_policies p WHERE p.type=t.type AND p.escalate_to=$2))
         LIMIT 1000"))
    .bind(me)
    .bind(role)
    .fetch_all(&mut *c)
    .await?;
    for r in &rows {
        let t = Task::from_row(r);
        let key = format!("WRK-{}", t.seq);
        let prio = t
            .priority_raised
            .clone()
            .unwrap_or_else(|| t.priority.clone());
        let link = format!("/admin/tasks/{}", t.id);
        let refer = Some(json!({"kind": "task", "id": t.id, "label": key}));
        let state = t.state(pol, now);
        let mk = |id: String,
                  kind: &'static str,
                  summary: String,
                  priority: String,
                  at: DateTime<Utc>| InboxItem {
            id,
            kind,
            title: t.title.clone(),
            summary,
            priority,
            due_at: t.due(),
            at,
            link: link.clone(),
            read: false,
            refer: refer.clone(),
        };
        if t.assignee_id == Some(me) && t.status != "IN_REVIEW" {
            if t.changes_requested {
                out.push(mk(
                    format!("cr:{}:{}", t.id, t.row_version),
                    "CORRECTION",
                    format!("{key} · 검토자가 보완을 요청했어요"),
                    prio.clone(),
                    t.updated_at,
                ));
            } else {
                out.push(mk(
                    format!("as:{}", t.id),
                    "ASSIGNED",
                    format!("{key} · 나에게 배정"),
                    prio.clone(),
                    t.assigned_at.unwrap_or(t.created_at),
                ));
            }
        }
        if t.reviewer_id == Some(me) && t.status == "IN_REVIEW" {
            out.push(mk(
                format!("rv:{}:{}", t.id, t.row_version),
                "REVIEW_REQUEST",
                format!("{key} · 검토 승인 요청"),
                prio.clone(),
                t.updated_at,
            ));
        }
        let escalated_to_me = pol
            .iter()
            .any(|p| p.kind == t.kind && p.escalate_to == role);
        if state == "OVERDUE" && (t.assignee_id == Some(me) || escalated_to_me) {
            out.push(mk(
                format!("sla:{}", t.id),
                "SLA_BREACH",
                format!("{key} · SLA 기한을 넘겼어요"),
                "URGENT".into(),
                t.due().unwrap_or(t.created_at),
            ));
        }
    }

    // Mentions
    let rows = sqlx::query(
        "SELECT n.id, n.target_kind, n.target_id, n.body, n.created_at, split_part(u.email,'@',1) AS who
           FROM work.notes n JOIN identity.users u ON u.id=n.author_id
          WHERE $1 = ANY(n.mentions) AND NOT n.resolved ORDER BY n.created_at DESC LIMIT 200",
    )
    .bind(me)
    .fetch_all(&mut *c)
    .await?;
    for r in rows {
        let kind: String = r.get("target_kind");
        let target: String = r.get("target_id");
        let body: String = r.get("body");
        let link = match kind.as_str() {
            "task" => format!("/admin/tasks/{target}"),
            "release" => format!("/admin/reviews/{target}"),
            "contact" => format!("/admin/crm/{target}"),
            "dsp" => format!("/admin/dsp-ops?dsp={target}"),
            "inquiry" => format!("/admin/inquiries/{target}"),
            "exception" => format!("/admin/exceptions?id={target}"),
            _ => "/admin/inbox".into(),
        };
        out.push(InboxItem {
            id: format!("mn:{}", r.get::<Uuid, _>("id")),
            kind: "MENTION",
            title: format!("{}님이 나를 언급했어요", r.get::<String, _>("who")),
            summary: body.chars().take(90).collect(),
            priority: "NORMAL".into(),
            due_at: None,
            at: r.get("created_at"),
            link,
            read: false,
            refer: Some(json!({"kind": kind, "id": target, "label": ""})),
        });
    }

    // Exceptions: unassigned ones for everyone, assigned ones for the assignee.
    let rows = sqlx::query(
        "SELECT id,kind,severity,title,source,occurrences,last_seen FROM work.exceptions
          WHERE status IN ('OPEN','ASSIGNED') AND (assignee_id IS NULL OR assignee_id=$1) ORDER BY last_seen DESC LIMIT 200",
    )
    .bind(me)
    .fetch_all(&mut *c)
    .await?;
    for r in rows {
        let id: Uuid = r.get("id");
        let kind: String = r.get("kind");
        let sev: String = r.get("severity");
        let n: i32 = r.get("occurrences");
        out.push(InboxItem {
            id: format!("ex:{id}:{n}"),
            kind: if kind == "RIGHTS_DOCS" {
                "RIGHTS"
            } else {
                "AUTOMATION_FAILED"
            },
            title: r.get("title"),
            summary: format!(
                "{} · {}",
                r.get::<String, _>("source"),
                if n > 1 {
                    format!("{n}회 반복")
                } else {
                    "1회".into()
                }
            ),
            priority: match sev.as_str() {
                "CRITICAL" => "URGENT",
                "HIGH" => "HIGH",
                _ => "NORMAL",
            }
            .into(),
            due_at: None,
            at: r.get("last_seen"),
            link: format!("/admin/exceptions?id={id}"),
            read: false,
            refer: Some(
                json!({"kind": "exception", "id": id, "label": r.get::<String,_>("title")}),
            ),
        });
    }

    staff_scope(c).await?;
    // Unanswered inquiries — for staff who answer them. Waits past the SUPPORT SLA are urgent.
    if st.role.may(staff::Duty::Inquiries) {
        let support = pol.iter().find(|p| p.kind == "SUPPORT");
        let rows = sqlx::query(
            "SELECT q.id,q.subject,q.category,q.created_at,o.name AS org FROM portal.inquiries q JOIN identity.orgs o ON o.id=q.org_id
              WHERE q.status='OPEN' ORDER BY q.created_at LIMIT 100",
        )
        .fetch_all(&mut *c)
        .await?;
        for r in rows {
            let id: Uuid = r.get("id");
            let created: DateTime<Utc> = r.get("created_at");
            let due = sla_due(created, support);
            let hours = (now - created).num_hours();
            out.push(InboxItem {
                id: format!("iq:{id}"),
                kind: "INQUIRY",
                title: r.get("subject"),
                summary: format!("{} · {}시간째 답변 대기", r.get::<String, _>("org"), hours),
                priority: if due.is_some_and(|d| d < now) {
                    "HIGH"
                } else {
                    "NORMAL"
                }
                .into(),
                due_at: due,
                at: created,
                link: format!("/admin/inquiries/{id}"),
                read: false,
                refer: Some(
                    json!({"kind": "inquiry", "id": id, "label": r.get::<String,_>("subject")}),
                ),
            });
        }
    }
    // Releases on rights hold — for reviewers.
    if st.role.may(staff::Duty::Review) {
        let rows = sqlx::query(
            "SELECT r.id, r.title, COALESCE((SELECT max(a.occurred_at) FROM operations.audit_events a WHERE a.resource_id=r.id), r.created_at) AS at
               FROM catalog.releases r WHERE r.status='ON_HOLD_RIGHTS' AND r.archived_at IS NULL ORDER BY at LIMIT 100",
        )
        .fetch_all(&mut *c)
        .await?;
        for r in rows {
            let id: Uuid = r.get("id");
            out.push(InboxItem {
                id: format!("rt:{id}"),
                kind: "RIGHTS",
                title: format!("{} — 권리 보류", r.get::<String, _>("title")),
                summary: "권리 확인이 끝나야 배급이 이어져요".into(),
                priority: "HIGH".into(),
                due_at: None,
                at: r.get("at"),
                link: format!("/admin/reviews/{id}"),
                read: false,
                refer: Some(
                    json!({"kind": "release", "id": id, "label": r.get::<String,_>("title")}),
                ),
            });
        }
    }

    let ids: Vec<String> = out.iter().map(|i| i.id.clone()).collect();
    let read: Vec<String> = sqlx::query_scalar(
        "SELECT item_id FROM work.inbox_reads WHERE user_id=$1 AND item_id = ANY($2)",
    )
    .bind(me)
    .bind(&ids)
    .fetch_all(&mut *c)
    .await?;
    for i in &mut out {
        i.read = read.contains(&i.id);
    }
    out.sort_by(|a, b| {
        a.read
            .cmp(&b.read)
            .then(rank(&b.priority).cmp(&rank(&a.priority)))
            .then(b.at.cmp(&a.at))
    });
    Ok(out)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxQuery {
    pub kind: Option<String>,
    pub unread: Option<bool>,
}

pub async fn inbox(s: &AppState, h: &HeaderMap, q: InboxQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    let pol = policies(&mut tx).await?;
    let now = Utc::now();
    sweep(&mut tx, &pol, now).await?;
    let items = build_inbox(&mut tx, &st, &pol, now).await?;
    tx.commit().await?;
    let kind = q.kind.as_deref().filter(|k| !k.is_empty());
    let items: Vec<&InboxItem> = items
        .iter()
        .filter(|i| kind.is_none_or(|k| i.kind == k) && (!q.unread.unwrap_or(false) || !i.read))
        .collect();
    Ok(json!({ "items": items }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    pub ids: Vec<String>,
}

pub async fn read_inbox(s: &AppState, h: &HeaderMap, i: ReadInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if i.ids.len() > 500 || i.ids.iter().any(|x| x.is_empty() || x.len() > 120) {
        return Err(Error::Invalid);
    }
    sqlx::query(
        "INSERT INTO work.inbox_reads(user_id,item_id) SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
    )
    .bind(st.actor.user)
    .bind(&i.ids)
    .execute(&s.pool)
    .await?;
    Ok(json!({ "ok": true }))
}

pub async fn summary(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let me = st.actor.user;
    let mut tx = s.pool.begin().await?;
    let pol = policies(&mut tx).await?;
    let now = Utc::now();
    sweep(&mut tx, &pol, now).await?;
    let items = build_inbox(&mut tx, &st, &pol, now).await?;
    let open = sqlx::query(concat!(
        "SELECT ",
        task_cols!(),
        " FROM work.tasks t WHERE t.status NOT IN ('DONE','CANCELLED') LIMIT 5000"
    ))
    .fetch_all(&mut *tx)
    .await?;
    let exceptions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.exceptions WHERE status IN ('OPEN','ASSIGNED')",
    )
    .fetch_one(&mut *tx)
    .await?;
    let follow: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.contacts WHERE next_follow_up_at <= now() + interval '1 day' AND (owner_id=$1 OR owner_id IS NULL)",
    )
    .bind(me)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    let tasks: Vec<Task> = open.iter().map(Task::from_row).collect();
    let mine: Vec<&Task> = tasks.iter().filter(|t| t.assignee_id == Some(me)).collect();
    let mut by_kind = serde_json::Map::new();
    for i in items.iter().filter(|i| !i.read) {
        let n = by_kind.get(i.kind).and_then(Value::as_i64).unwrap_or(0);
        by_kind.insert(i.kind.to_string(), json!(n + 1));
    }
    Ok(json!({
        "inbox_unread": items.iter().filter(|i| !i.read).count(),
        "inbox_total": items.len(),
        "my_open": mine.len(),
        "due_soon": mine.iter().filter(|t| t.state(&pol, now) == "DUE_SOON").count(),
        "overdue": tasks.iter().filter(|t| t.state(&pol, now) == "OVERDUE").count(),
        "exceptions_open": exceptions,
        "follow_ups_due": follow,
        "by_kind": by_kind,
    }))
}

// ---------------------------------------------------------------------------
// Notes and attachments
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteQuery {
    pub target_kind: String,
    pub target_id: String,
}

async fn note_json(c: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<Value>> {
    let rows = sqlx::query(
        "SELECT n.id,n.target_kind,n.target_id,n.author_id,n.body,n.mentions,n.resolved,n.resolved_by,n.resolved_at,n.created_at,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('id',a.id,'name',a.name,'size',octet_length(a.data),'content_type',a.content_type) ORDER BY a.created_at)
                            FROM work.note_attachments a WHERE a.note_id=n.id),'[]') AS attachments
           FROM work.notes n WHERE n.id = ANY($1) ORDER BY n.created_at",
    )
    .bind(ids)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({
                "id": r.get::<Uuid,_>("id"), "target_kind": r.get::<String,_>("target_kind"), "target_id": r.get::<String,_>("target_id"),
                "author_id": r.get::<Uuid,_>("author_id"), "body": r.get::<String,_>("body"), "mentions": r.get::<Vec<Uuid>,_>("mentions"),
                "attachments": r.get::<Value,_>("attachments"), "resolved": r.get::<bool,_>("resolved"),
                "resolved_by": r.get::<Option<Uuid>,_>("resolved_by"), "resolved_at": r.get::<Option<DateTime<Utc>>,_>("resolved_at"),
                "created_at": r.get::<DateTime<Utc>,_>("created_at"),
            })
        })
        .collect())
}

pub async fn notes(s: &AppState, h: &HeaderMap, q: NoteQuery) -> Result<Value> {
    staff::staff(s, h, false).await?;
    one_of(&q.target_kind, ENTITY_KINDS, "TARGET_KIND_UNKNOWN")?;
    let mut c = s.pool.acquire().await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM work.notes WHERE target_kind=$1 AND target_id=$2 ORDER BY created_at LIMIT 500",
    )
    .bind(&q.target_kind)
    .bind(&q.target_id)
    .fetch_all(&mut *c)
    .await?;
    Ok(json!({ "items": note_json(&mut c, &ids).await? }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteInput {
    pub target_kind: String,
    pub target_id: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub mentions: Vec<Uuid>,
    #[serde(default)]
    pub attachments: Vec<Uuid>,
}

pub async fn add_note(s: &AppState, h: &HeaderMap, i: NoteInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    one_of(&i.target_kind, ENTITY_KINDS, "TARGET_KIND_UNKNOWN")?;
    let target = line(&i.target_id, 80)?;
    let body = text(&i.body, 4000)?;
    if body.is_empty() && i.attachments.is_empty() {
        return Err(Error::InvalidCode("NOTE_EMPTY"));
    }
    if i.attachments.len() > MAX_ATTACHMENTS || i.mentions.len() > 20 {
        return Err(Error::InvalidCode("NOTE_TOO_MANY"));
    }
    let mut tx = s.pool.begin().await?;
    let mut mentions = i.mentions.clone();
    mentions.sort();
    mentions.dedup();
    for m in &mentions {
        active_staff(&mut tx, Some(*m)).await?;
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO work.notes(id,target_kind,target_id,author_id,body,mentions) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id)
        .bind(&i.target_kind)
        .bind(&target)
        .bind(st.actor.user)
        .bind(&body)
        .bind(&mentions)
        .execute(&mut *tx)
        .await?;
    for up in &i.attachments {
        // Only a finished upload of this user becomes an attachment.
        let moved = sqlx::query(
            "WITH u AS (DELETE FROM work.uploads WHERE id=$1 AND owner_id=$2 AND octet_length(data)=size
                          AND encode(sha256(data),'hex')=sha256 RETURNING name,content_type,data,sha256)
             INSERT INTO work.note_attachments(id,note_id,name,content_type,data,sha256) SELECT $1,$3,name,content_type,data,sha256 FROM u",
        )
        .bind(up)
        .bind(st.actor.user)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if moved.rows_affected() != 1 {
            return Err(Error::InvalidCode("UPLOAD_INCOMPLETE"));
        }
    }
    if i.target_kind == "task"
        && let Ok(task) = Uuid::parse_str(&target)
    {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work.tasks WHERE id=$1)")
                .bind(task)
                .fetch_one(&mut *tx)
                .await?;
        if exists {
            event(&mut tx, task, Some(st.actor.user), "NOTE", None).await?;
        }
    }
    audit(&mut tx, &st, Some(id), "work.note_added", &i.target_kind).await?;
    let out = note_json(&mut tx, &[id]).await?;
    tx.commit().await?;
    out.into_iter()
        .next()
        .map(Ok)
        .unwrap_or(Err(Error::Internal))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveInput {
    pub resolved: bool,
}

pub async fn resolve_note(s: &AppState, h: &HeaderMap, id: Uuid, i: ResolveInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query(
        "UPDATE work.notes SET resolved=$2, resolved_by=CASE WHEN $2 THEN $3 END, resolved_at=CASE WHEN $2 THEN now() END WHERE id=$1",
    )
    .bind(id)
    .bind(i.resolved)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    if n.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.note_resolved",
        if i.resolved { "RESOLVED" } else { "REOPENED" },
    )
    .await?;
    let out = note_json(&mut tx, &[id]).await?;
    tx.commit().await?;
    out.into_iter()
        .next()
        .map(Ok)
        .unwrap_or(Err(Error::Internal))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadInput {
    pub name: String,
    pub content_type: String,
    pub size: i32,
    pub sha256: String,
}

pub async fn start_upload(s: &AppState, h: &HeaderMap, i: UploadInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let name = line(&i.name, 200)?;
    let ct = line(&i.content_type, 120)?;
    if name.is_empty() || ct.is_empty() || !(1..=MAX_UPLOAD).contains(&i.size) {
        return Err(Error::InvalidCode("UPLOAD_INVALID"));
    }
    if i.sha256.len() != 64
        || !i
            .sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::InvalidCode("UPLOAD_INVALID"));
    }
    let mut tx = s.pool.begin().await?;
    // forget stale, unclaimed uploads
    sqlx::query("DELETE FROM work.uploads WHERE created_at < now() - interval '1 day'")
        .execute(&mut *tx)
        .await?;
    let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM work.uploads WHERE owner_id=$1")
        .bind(st.actor.user)
        .fetch_one(&mut *tx)
        .await?;
    if pending >= 10 {
        return Err(Error::RateLimited);
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO work.uploads(id,owner_id,name,content_type,size,sha256) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(id).bind(st.actor.user).bind(&name).bind(&ct).bind(i.size).bind(&i.sha256)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(json!({ "id": id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartInput {
    pub index: i32,
    pub data: String,
}

pub async fn upload_part(s: &AppState, h: &HeaderMap, id: Uuid, i: PartInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(i.data.as_bytes())
        .map_err(|_| Error::InvalidCode("UPLOAD_PART_INVALID"))?;
    if bytes.is_empty() || bytes.len() > MAX_PART {
        return Err(Error::InvalidCode("UPLOAD_PART_INVALID"));
    }
    // Parts arrive in order; a retried part with the same index is a no-op.
    let n = sqlx::query(
        "UPDATE work.uploads SET data = data || $4, parts = parts + 1
          WHERE id=$1 AND owner_id=$2 AND parts=$3 AND octet_length(data) + octet_length($4) <= size",
    )
    .bind(id)
    .bind(st.actor.user)
    .bind(i.index)
    .bind(&bytes)
    .execute(&s.pool)
    .await?;
    if n.rows_affected() == 0 {
        let parts: Option<i32> =
            sqlx::query_scalar("SELECT parts FROM work.uploads WHERE id=$1 AND owner_id=$2")
                .bind(id)
                .bind(st.actor.user)
                .fetch_optional(&s.pool)
                .await?;
        return match parts {
            None => Err(Error::NotFound),
            Some(p) if p > i.index => Ok(json!({ "parts": p })),
            Some(_) => Err(Error::Conflict),
        };
    }
    Ok(json!({ "parts": i.index + 1 }))
}

/// Attachments are served as downloads only (never rendered inline).
pub async fn attachment(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Response> {
    let st = staff::staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    let r = sqlx::query("SELECT name,data FROM work.note_attachments WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    audit(&mut tx, &st, Some(id), "work.attachment_viewed", "DOWNLOAD").await?;
    tx.commit().await?;
    let name: String = r.get("name");
    let data: Vec<u8> = r.get("data");
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let mut res = Response::new(Body::from(data));
    let hs = res.headers_mut();
    hs.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    hs.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    hs.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!(
        "attachment; filename=\"attachment\"; filename*=UTF-8''{encoded}"
    )) {
        hs.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

// ---------------------------------------------------------------------------
// Exception queue
// ---------------------------------------------------------------------------

macro_rules! exception_cols {
    () => {
        "id,kind,severity,status,title,detail,source,related,assignee_id,task_id,occurrences,first_seen,last_seen,resolved_at,resolution"
    };
}

fn exception_json(r: &PgRow) -> Value {
    json!({
        "id": r.get::<Uuid,_>("id"), "kind": r.get::<String,_>("kind"), "severity": r.get::<String,_>("severity"),
        "status": r.get::<String,_>("status"), "title": r.get::<String,_>("title"), "detail": r.get::<String,_>("detail"),
        "source": r.get::<String,_>("source"), "related": r.get::<Value,_>("related"),
        "assignee_id": r.get::<Option<Uuid>,_>("assignee_id"), "task_id": r.get::<Option<Uuid>,_>("task_id"),
        "occurrences": r.get::<i32,_>("occurrences"), "first_seen": r.get::<DateTime<Utc>,_>("first_seen"),
        "last_seen": r.get::<DateTime<Utc>,_>("last_seen"), "resolved_at": r.get::<Option<DateTime<Utc>>,_>("resolved_at"),
        "resolution": r.get::<Option<String>,_>("resolution"),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExceptionQuery {
    pub kind: Option<String>,
    pub status: Option<String>,
}

pub async fn exceptions(s: &AppState, h: &HeaderMap, q: ExceptionQuery) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let kind = q.kind.as_deref().filter(|k| !k.is_empty());
    if let Some(k) = kind {
        one_of(k, EXCEPTION_KINDS, "EXCEPTION_KIND_UNKNOWN")?;
    }
    let status = q
        .status
        .as_deref()
        .filter(|k| !k.is_empty())
        .unwrap_or("active");
    one_of(
        status,
        &["active", "OPEN", "ASSIGNED", "RESOLVED", "IGNORED"],
        "STATUS_UNKNOWN",
    )?;
    let rows = sqlx::query(concat!(
        "SELECT ",
        exception_cols!(),
        " FROM work.exceptions
          WHERE ($1::text IS NULL OR kind=$1)
            AND (($2='active' AND status IN ('OPEN','ASSIGNED')) OR status=$2)
          ORDER BY array_position($3::text[], severity) DESC, last_seen DESC LIMIT 300"
    ))
    .bind(kind)
    .bind(status)
    .bind(SEVERITIES)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({ "items": rows.iter().map(exception_json).collect::<Vec<_>>() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExceptionAction {
    pub action: String,
    pub assignee_id: Option<Uuid>,
    pub resolution: Option<String>,
    pub priority: Option<String>,
}

fn task_type_for(kind: &str) -> &'static str {
    match kind {
        "RIGHTS_DOCS" => "RIGHTS",
        "SETTLEMENT_MISMATCH" => "SETTLEMENT",
        "DSP_REJECTION" => "DSP_INQUIRY",
        "DELIVERY_FAILURE" => "URGENT_DELIVERY",
        _ => "RELEASE",
    }
}

pub async fn act_exception(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    a: ExceptionAction,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let me = st.actor.user;
    let mut tx = s.pool.begin().await?;
    let r = sqlx::query(concat!(
        "SELECT ",
        exception_cols!(),
        " FROM work.exceptions WHERE id=$1 FOR UPDATE"
    ))
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let status: String = r.get("status");
    let active = status == "OPEN" || status == "ASSIGNED";
    match a.action.as_str() {
        "ASSIGN" => {
            let who = a
                .assignee_id
                .ok_or(Error::InvalidCode("ASSIGNEE_REQUIRED"))?;
            active_staff(&mut tx, Some(who)).await?;
            if !active {
                return Err(Error::PolicyGate("EXCEPTION_CLOSED"));
            }
            sqlx::query("UPDATE work.exceptions SET assignee_id=$2, status='ASSIGNED' WHERE id=$1")
                .bind(id)
                .bind(who)
                .execute(&mut *tx)
                .await?;
        }
        "RESOLVE" | "IGNORE" => {
            let res = text(a.resolution.as_deref().unwrap_or(""), 1000)?;
            if res.is_empty() {
                return Err(Error::InvalidCode("RESOLUTION_REQUIRED"));
            }
            if !active {
                return Err(Error::PolicyGate("EXCEPTION_CLOSED"));
            }
            sqlx::query("UPDATE work.exceptions SET status=$2, resolution=$3, resolved_at=now(), resolved_by=$4 WHERE id=$1")
                .bind(id)
                .bind(if a.action == "RESOLVE" { "RESOLVED" } else { "IGNORED" })
                .bind(&res)
                .bind(me)
                .execute(&mut *tx)
                .await?;
        }
        "REOPEN" => {
            sqlx::query(
                "UPDATE work.exceptions SET status=CASE WHEN assignee_id IS NULL THEN 'OPEN' ELSE 'ASSIGNED' END, resolution=NULL, resolved_at=NULL, resolved_by=NULL WHERE id=$1",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        "TO_TASK" => {
            if r.get::<Option<Uuid>, _>("task_id").is_some() {
                return Err(Error::Conflict);
            }
            if !active {
                return Err(Error::PolicyGate("EXCEPTION_CLOSED"));
            }
            let assignee = a.assignee_id.or(Some(me));
            active_staff(&mut tx, assignee).await?;
            let kind: String = r.get("kind");
            let sev: String = r.get("severity");
            let prio = match a.priority.as_deref() {
                Some(p) => {
                    one_of(p, PRIORITIES, "PRIORITY_UNKNOWN")?;
                    p.to_string()
                }
                None => match sev.as_str() {
                    "CRITICAL" => "URGENT",
                    "HIGH" => "HIGH",
                    _ => "NORMAL",
                }
                .to_string(),
            };
            let ty = task_type_for(&kind);
            let pol = policies(&mut tx).await?;
            let now = Utc::now();
            let title: String = r.get("title");
            let mut rel: Value = r.get("related");
            if let Some(arr) = rel.as_array_mut() {
                arr.push(json!({"kind": "exception", "id": id.to_string(), "label": title.chars().take(200).collect::<String>()}));
                arr.truncate(20);
            }
            let tid = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO work.tasks(id,title,description,type,priority,assignee_id,sla_due_at,related,created_by,created_at,updated_at,assigned_at)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10,$10)",
            )
            .bind(tid).bind(title.chars().take(200).collect::<String>()).bind(r.get::<String, _>("detail")).bind(ty).bind(&prio)
            .bind(assignee).bind(sla_due(now, pol.iter().find(|p| p.kind == ty))).bind(rel).bind(me).bind(now)
            .execute(&mut *tx)
            .await?;
            event(
                &mut tx,
                tid,
                Some(me),
                "CREATED",
                Some("예외 처리에서 넘김"),
            )
            .await?;
            sqlx::query("UPDATE work.exceptions SET task_id=$2, assignee_id=$3, status='ASSIGNED' WHERE id=$1")
                .bind(id).bind(tid).bind(assignee).execute(&mut *tx).await?;
        }
        _ => return Err(Error::InvalidCode("ACTION_UNKNOWN")),
    }
    audit(&mut tx, &st, Some(id), "work.exception_action", &a.action).await?;
    let r = sqlx::query(concat!(
        "SELECT ",
        exception_cols!(),
        " FROM work.exceptions WHERE id=$1"
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(exception_json(&r))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExceptionReport {
    pub kind: String,
    pub severity: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub related: Vec<EntityRef>,
}

/// Manual report (e.g. a settlement mismatch found while reconciling). The
/// automation reports through `work.report_exception` in the database.
pub async fn report_exception(s: &AppState, h: &HeaderMap, i: ExceptionReport) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    one_of(&i.kind, EXCEPTION_KINDS, "EXCEPTION_KIND_UNKNOWN")?;
    one_of(&i.severity, SEVERITIES, "SEVERITY_UNKNOWN")?;
    let title = line(&i.title, 300)?;
    if title.is_empty() {
        return Err(Error::InvalidCode("TITLE_REQUIRED"));
    }
    let detail = text(&i.detail, 4000)?;
    let rel = related(&i.related)?;
    let mut tx = s.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar("SELECT work.report_exception($1,$2,$3,$4,$5,$6,$7)")
        .bind(&i.kind)
        .bind(&i.severity)
        .bind(&title)
        .bind(&detail)
        .bind(format!("staff:{}", st.role.as_str().to_lowercase()))
        .bind(format!("manual:{}", Uuid::new_v4()))
        .bind(rel)
        .fetch_one(&mut *tx)
        .await?;
    audit(&mut tx, &st, Some(id), "work.exception_reported", &i.kind).await?;
    let r = sqlx::query(concat!(
        "SELECT ",
        exception_cols!(),
        " FROM work.exceptions WHERE id=$1"
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(exception_json(&r))
}

// ---------------------------------------------------------------------------
// SLA policies
// ---------------------------------------------------------------------------

pub async fn list_sla(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    Ok(json!({ "items": policies(&mut c).await? }))
}

pub async fn save_sla(s: &AppState, h: &HeaderMap, kind: String, p: SlaPolicy) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    if p.kind != kind {
        return Err(Error::Invalid);
    }
    one_of(&p.kind, TASK_TYPES, "TASK_TYPE_UNKNOWN")?;
    one_of(&p.escalate_to, ROLES, "ROLE_UNKNOWN")?;
    if !(15..=100_000).contains(&p.target_minutes)
        || !(10..=100_000).contains(&p.idle_alert_minutes)
        || !(p.warn_ratio > 0.0 && p.warn_ratio < 1.0)
    {
        return Err(Error::InvalidCode("SLA_INVALID"));
    }
    let mut tx = s.pool.begin().await?;
    sqlx::query(
        "UPDATE work.sla_policies SET target_minutes=$2,business_hours=$3,warn_ratio=$4,idle_alert_minutes=$5,escalate_to=$6,
            auto_raise_priority=$7,enabled=$8,updated_by=$9,updated_at=now() WHERE type=$1",
    )
    .bind(&p.kind).bind(p.target_minutes).bind(p.business_hours).bind(p.warn_ratio).bind(p.idle_alert_minutes)
    .bind(&p.escalate_to).bind(p.auto_raise_priority).bind(p.enabled).bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    // Re-derive the SLA deadline of open tasks of this type.
    let open: Vec<(Uuid, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id,created_at FROM work.tasks WHERE type=$1 AND status NOT IN ('DONE','CANCELLED') FOR UPDATE",
    )
    .bind(&p.kind)
    .fetch_all(&mut *tx)
    .await?;
    for (id, created) in open {
        sqlx::query("UPDATE work.tasks SET sla_due_at=$2, escalation_level=0, priority_raised=NULL WHERE id=$1")
            .bind(id)
            .bind(sla_due(created, Some(&p)))
            .execute(&mut *tx)
            .await?;
    }
    audit(&mut tx, &st, None, "work.sla_updated", &p.kind).await?;
    tx.commit().await?;
    serde_json::to_value(&p).map_err(|_| Error::Internal)
}

// ---------------------------------------------------------------------------
// CRM Lite
// ---------------------------------------------------------------------------

macro_rules! contact_cols {
    () => {
        "id,kind,name,company,person,email,phone,owner_id,last_contact_at,next_follow_up_at,contract_stage,tags,org_id,dsp_code,memo,created_at,updated_at"
    };
}

fn contact_json(r: &PgRow) -> Value {
    json!({
        "id": r.get::<Uuid,_>("id"), "kind": r.get::<String,_>("kind"), "name": r.get::<String,_>("name"),
        "company": r.get::<String,_>("company"), "person": r.get::<String,_>("person"), "email": r.get::<String,_>("email"),
        "phone": r.get::<String,_>("phone"), "owner_id": r.get::<Option<Uuid>,_>("owner_id"),
        "last_contact_at": r.get::<Option<DateTime<Utc>>,_>("last_contact_at"),
        "next_follow_up_at": r.get::<Option<DateTime<Utc>>,_>("next_follow_up_at"),
        "contract_stage": r.get::<String,_>("contract_stage"), "tags": r.get::<Vec<String>,_>("tags"),
        "org_id": r.get::<Option<Uuid>,_>("org_id"), "dsp_code": r.get::<Option<String>,_>("dsp_code"),
        "memo": r.get::<String,_>("memo"), "created_at": r.get::<DateTime<Utc>,_>("created_at"),
        "updated_at": r.get::<DateTime<Utc>,_>("updated_at"),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactQuery {
    pub kind: Option<String>,
    pub q: Option<String>,
    pub due: Option<bool>,
}

pub async fn contacts(s: &AppState, h: &HeaderMap, q: ContactQuery) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let kind = q.kind.as_deref().filter(|k| !k.is_empty());
    if let Some(k) = kind {
        one_of(k, CONTACT_KINDS, "CONTACT_KIND_UNKNOWN")?;
    }
    let term =
        q.q.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(80).collect::<String>());
    let rows = sqlx::query(concat!("SELECT ", contact_cols!(), " FROM work.contacts
          WHERE ($1::text IS NULL OR kind=$1)
            AND ($2::text IS NULL OR name ILIKE '%'||$2||'%' OR company ILIKE '%'||$2||'%' OR person ILIKE '%'||$2||'%' OR email ILIKE '%'||$2||'%' OR $2=ANY(tags))
            AND (NOT $3 OR next_follow_up_at <= now() + interval '1 day')
          ORDER BY next_follow_up_at NULLS LAST, name LIMIT 500"))
    .bind(kind)
    .bind(term)
    .bind(q.due.unwrap_or(false))
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({ "items": rows.iter().map(contact_json).collect::<Vec<_>>() }))
}

pub async fn contact(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    let pol = policies(&mut tx).await?;
    let r = sqlx::query(concat!(
        "SELECT ",
        contact_cols!(),
        " FROM work.contacts WHERE id=$1"
    ))
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let dsp: Option<String> = r.get("dsp_code");
    let acts = sqlx::query("SELECT id,kind,summary,by_user,at FROM work.contact_activities WHERE contact_id=$1 ORDER BY at DESC LIMIT 200")
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
    let tasks = sqlx::query(concat!("SELECT ", task_cols!(), " FROM work.tasks t WHERE t.related @> $1 OR ($2::text IS NOT NULL AND t.related @> jsonb_build_array(jsonb_build_object('kind','dsp','id',$2::text)))
          ORDER BY (t.status IN ('DONE','CANCELLED')), t.created_at DESC LIMIT 50"))
    .bind(json!([{"kind": "contact", "id": id.to_string()}]))
    .bind(dsp)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let now = Utc::now();
    Ok(json!({
        "contact": contact_json(&r),
        "activities": acts.iter().map(|a| json!({"id": a.get::<Uuid,_>("id"), "kind": a.get::<String,_>("kind"),
            "summary": a.get::<String,_>("summary"), "by": a.get::<Uuid,_>("by_user"), "at": a.get::<DateTime<Utc>,_>("at")})).collect::<Vec<_>>(),
        "tasks": tasks.iter().map(|t| Task::from_row(t).json(&pol, now)).collect::<Vec<_>>(),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactInput {
    pub kind: Option<String>,
    pub name: Option<String>,
    pub company: Option<String>,
    pub person: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub owner_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub next_follow_up_at: Option<Option<DateTime<Utc>>>,
    pub contract_stage: Option<String>,
    pub tags: Option<Vec<String>>,
    pub memo: Option<String>,
}

pub async fn save_contact(
    s: &AppState,
    h: &HeaderMap,
    id: Option<Uuid>,
    i: ContactInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let cur = match id {
        Some(id) => Some(
            sqlx::query(concat!(
                "SELECT ",
                contact_cols!(),
                " FROM work.contacts WHERE id=$1 FOR UPDATE"
            ))
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?,
        ),
        None => None,
    };
    let get = |k: &str| {
        cur.as_ref()
            .map(|r| r.get::<String, _>(k))
            .unwrap_or_default()
    };
    let kind = i.kind.clone().unwrap_or_else(|| {
        if cur.is_some() {
            get("kind")
        } else {
            String::new()
        }
    });
    one_of(&kind, CONTACT_KINDS, "CONTACT_KIND_UNKNOWN")?;
    let name = line(i.name.as_deref().unwrap_or(&get("name")), 120)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    let company = line(i.company.as_deref().unwrap_or(&get("company")), 120)?;
    let person = line(i.person.as_deref().unwrap_or(&get("person")), 120)?;
    let email = line(i.email.as_deref().unwrap_or(&get("email")), 200)?;
    if !email.is_empty() && !email.contains('@') {
        return Err(Error::InvalidCode("EMAIL_INVALID"));
    }
    let phone = line(i.phone.as_deref().unwrap_or(&get("phone")), 40)?;
    let stage = i.contract_stage.clone().unwrap_or_else(|| {
        if cur.is_some() {
            get("contract_stage")
        } else {
            "NONE".into()
        }
    });
    one_of(&stage, CONTRACT_STAGES, "CONTRACT_STAGE_UNKNOWN")?;
    let memo = text(i.memo.as_deref().unwrap_or(&get("memo")), 2000)?;
    let tg = match &i.tags {
        Some(t) => tags(t)?,
        None => cur
            .as_ref()
            .map(|r| r.get::<Vec<String>, _>("tags"))
            .unwrap_or_default(),
    };
    let owner = match i.owner_id {
        Some(v) => v,
        None => cur
            .as_ref()
            .and_then(|r| r.get::<Option<Uuid>, _>("owner_id")),
    };
    active_staff(&mut tx, owner).await?;
    let follow = match i.next_follow_up_at {
        Some(v) => v,
        None => cur
            .as_ref()
            .and_then(|r| r.get::<Option<DateTime<Utc>>, _>("next_follow_up_at")),
    };
    let id = id.unwrap_or_else(Uuid::new_v4);
    sqlx::query(
        "INSERT INTO work.contacts(id,kind,name,company,person,email,phone,owner_id,next_follow_up_at,contract_stage,tags,memo)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
         ON CONFLICT (id) DO UPDATE SET kind=$2,name=$3,company=$4,person=$5,email=$6,phone=$7,owner_id=$8,next_follow_up_at=$9,
           contract_stage=$10,tags=$11,memo=$12,updated_at=now()",
    )
    .bind(id).bind(&kind).bind(&name).bind(&company).bind(&person).bind(&email).bind(&phone).bind(owner).bind(follow)
    .bind(&stage).bind(&tg).bind(&memo)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        if cur.is_some() {
            "work.contact_updated"
        } else {
            "work.contact_created"
        },
        &kind,
    )
    .await?;
    let r = sqlx::query(concat!(
        "SELECT ",
        contact_cols!(),
        " FROM work.contacts WHERE id=$1"
    ))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(contact_json(&r))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityInput {
    pub kind: String,
    pub summary: String,
    #[serde(default, deserialize_with = "double_option")]
    pub next_follow_up_at: Option<Option<DateTime<Utc>>>,
}

pub async fn add_activity(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    i: ActivityInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    one_of(&i.kind, ACTIVITY_KINDS, "ACTIVITY_KIND_UNKNOWN")?;
    let summary = text(&i.summary, 2000)?;
    if summary.is_empty() {
        return Err(Error::InvalidCode("SUMMARY_REQUIRED"));
    }
    let mut tx = s.pool.begin().await?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work.contacts WHERE id=$1)")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if !exists {
        return Err(Error::NotFound);
    }
    let aid = Uuid::new_v4();
    let at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO work.contact_activities(id,contact_id,kind,summary,by_user) VALUES($1,$2,$3,$4,$5) RETURNING at",
    )
    .bind(aid).bind(id).bind(&i.kind).bind(&summary).bind(st.actor.user)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE work.contacts SET last_contact_at = CASE WHEN $2 <> 'NOTE' THEN $3 ELSE last_contact_at END,
            next_follow_up_at = CASE WHEN $4 THEN $5 ELSE next_follow_up_at END, updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(&i.kind)
    .bind(at)
    .bind(i.next_follow_up_at.is_some())
    .bind(i.next_follow_up_at.flatten())
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &st, Some(id), "work.contact_activity", &i.kind).await?;
    tx.commit().await?;
    Ok(json!({"id": aid, "kind": i.kind, "summary": summary, "by": st.actor.user, "at": at}))
}

// ---------------------------------------------------------------------------
// DSP operations
// ---------------------------------------------------------------------------

fn integration_kind(transport: Option<&str>, format: &str) -> &'static str {
    match transport.map(|t| t.to_ascii_lowercase()) {
        None => "MANUAL",
        Some(t) if t.contains("sftp") => "DDEX_SFTP",
        Some(t)
            if format == "DDEX" && (t.contains("http") || t.contains("api") || t == "partner") =>
        {
            "DDEX_API"
        }
        Some(t) if t == "mock" => {
            if format == "DDEX" {
                "DDEX_SFTP"
            } else {
                "API"
            }
        }
        Some(_) => "API",
    }
}

pub async fn dsp_ops(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    let rows = sqlx::query(
        r#"
        WITH jobs AS (
          SELECT j.partner_id AS code, j.id, j.status, j.created_at,
                 EXISTS(SELECT 1 FROM execution.ack_events a WHERE a.job_id=j.id AND a.outcome='REJECTED') AS rejected,
                 (SELECT min(a.received_at) FROM execution.ack_events a WHERE a.job_id=j.id AND a.outcome IN ('ACCEPTED','LIVE')) AS accepted_at
            FROM execution.delivery_jobs j WHERE j.created_at > now() - interval '56 days'
        )
        SELECT d.code, d.name, d.region, d.delivery_format,
               o.owner_id, COALESCE(o.contract_stage,'NONE') AS contract_stage, o.contract_renewal_at, COALESCE(o.sla_hours,24) AS sla_hours, o.health AS health_override,
               p.transport, COALESCE(p.delivery_enabled,false) AS delivery_enabled,
               (SELECT count(*) FROM jobs WHERE jobs.code=d.code AND created_at > now()-interval '30 days') AS deliveries_30d,
               (SELECT count(*) FROM jobs WHERE jobs.code=d.code AND created_at > now()-interval '30 days' AND (status IN ('FAILED','DEAD_LETTER') OR rejected)) AS failures_30d,
               (SELECT count(*) FROM jobs WHERE jobs.code=d.code AND created_at > now()-interval '1 day') AS deliveries_1d,
               (SELECT count(*) FROM jobs WHERE jobs.code=d.code AND created_at > now()-interval '1 day' AND (status IN ('FAILED','DEAD_LETTER') OR rejected)) AS failures_1d,
               (SELECT COALESCE(jsonb_agg(w ORDER BY wk),'[]') FROM (
                  SELECT g AS wk, (SELECT CASE WHEN count(*)=0 THEN 0 ELSE round(100.0*count(*) FILTER (WHERE status IN ('FAILED','DEAD_LETTER') OR rejected)/count(*),1) END
                                    FROM jobs WHERE jobs.code=d.code AND created_at > now()-((g+1)*interval '7 days') AND created_at <= now()-(g*interval '7 days')) AS w
                    FROM generate_series(7,0,-1) g) s) AS trend,
               (SELECT round((avg(extract(epoch FROM accepted_at-created_at))/3600.0)::numeric,1)::float8 FROM jobs WHERE jobs.code=d.code AND accepted_at IS NOT NULL) AS avg_approval_hours,
               (SELECT COALESCE(jsonb_agg(jsonb_build_object('reason',reason,'count',n) ORDER BY n DESC),'[]') FROM (
                  SELECT COALESCE(NULLIF(da.response->>'reason',''),NULLIF(da.response->>'message',''),NULLIF(da.response->>'code',''),'사유 미기재') AS reason, count(*) AS n
                    FROM execution.delivery_attempts da JOIN execution.delivery_jobs j ON j.id=da.job_id
                   WHERE j.partner_id=d.code AND da.outcome='REJECTED' AND da.created_at > now()-interval '30 days'
                   GROUP BY 1 ORDER BY 2 DESC LIMIT 5) r) AS rejections,
               GREATEST((SELECT max(a.received_at) FROM execution.ack_events a WHERE a.partner_id=d.code),
                        (SELECT max(da.created_at) FROM execution.delivery_attempts da JOIN execution.delivery_jobs j ON j.id=da.job_id WHERE j.partner_id=d.code)) AS last_sync_at
          FROM distribution.dsp_registry d
          LEFT JOIN work.dsp_ops o ON o.code=d.code
          LEFT JOIN execution.adapter_profiles p ON p.dsp_id=d.dsp_id OR p.partner_id=d.code
         WHERE d.status='ACTIVE'
         ORDER BY d.code
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;
    let pol = policies(&mut tx).await?;
    let now = Utc::now();
    let inq = sqlx::query(concat!("SELECT ", task_cols!(), " FROM work.tasks t WHERE t.type='DSP_INQUIRY' AND t.created_at > now()-interval '90 days' ORDER BY t.created_at DESC LIMIT 1000"))
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let inquiries: Vec<Task> = inq.iter().map(Task::from_row).collect();
    let mut items = Vec::new();
    for r in rows {
        let code: String = r.get("code");
        let mine: Vec<&Task> = inquiries
            .iter()
            .filter(|t| {
                t.related.as_array().is_some_and(|a| {
                    a.iter()
                        .any(|x| x["kind"] == "dsp" && x["id"] == code.as_str())
                })
            })
            .collect();
        let n30: i64 = r.get("deliveries_30d");
        let f30: i64 = r.get("failures_30d");
        let n1: i64 = r.get("deliveries_1d");
        let f1: i64 = r.get("failures_1d");
        let transport: Option<String> = r.get("transport");
        let enabled: bool = r.get("delivery_enabled");
        let derived = if transport.is_none() || (!enabled && n30 == 0) {
            "ONBOARDING"
        } else if n1 >= 2 && f1 * 2 >= n1 {
            "DOWN"
        } else if n30 > 0 && f30 * 20 >= n30 {
            "DEGRADED"
        } else {
            "HEALTHY"
        };
        let health: String = r
            .get::<Option<String>, _>("health_override")
            .unwrap_or_else(|| derived.to_string());
        let trend: Value = r.get("trend");
        items.push(json!({
            "code": code, "name": r.get::<String,_>("name"), "region": r.get::<String,_>("region"),
            "contract_stage": r.get::<String,_>("contract_stage"), "contract_renewal_at": r.get::<Option<DateTime<Utc>>,_>("contract_renewal_at"),
            "integration": integration_kind(transport.as_deref(), &r.get::<String,_>("delivery_format")),
            "health": health, "last_sync_at": r.get::<Option<DateTime<Utc>>,_>("last_sync_at"),
            "owner_id": r.get::<Option<Uuid>,_>("owner_id"), "sla_hours": r.get::<i32,_>("sla_hours"),
            "deliveries_30d": n30, "failures_30d": f30,
            "failure_trend": trend.as_array().map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect::<Vec<_>>()).unwrap_or_default(),
            "avg_approval_hours": r.get::<Option<f64>,_>("avg_approval_hours"),
            "rejections": r.get::<Value,_>("rejections"),
            "open_inquiries": mine.iter().filter(|t| t.open()).count(),
            "recent_inquiries": mine.iter().take(3).map(|t| json!({"id": t.id, "subject": t.title, "at": t.created_at,
                "status": if t.open() { "OPEN" } else { "CLOSED" }})).collect::<Vec<_>>(),
            "sla_breaches_30d": mine.iter().filter(|t| t.created_at > now - Duration::days(30) && matches!(t.state(&pol, now), "OVERDUE" | "MISSED")).count(),
        }));
    }
    Ok(json!({ "items": items }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DspOpsPatch {
    #[serde(default, deserialize_with = "double_option")]
    pub owner_id: Option<Option<Uuid>>,
    pub contract_stage: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub contract_renewal_at: Option<Option<DateTime<Utc>>>,
    pub sla_hours: Option<i32>,
    #[serde(default, deserialize_with = "double_option")]
    pub health: Option<Option<String>>,
}

pub async fn update_dsp_ops(
    s: &AppState,
    h: &HeaderMap,
    code: String,
    p: DspOpsPatch,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if !matches!(st.role, StaffRole::Admin | StaffRole::Operator) {
        return Err(Error::Forbidden);
    }
    if let Some(s) = &p.contract_stage {
        one_of(s, CONTRACT_STAGES, "CONTRACT_STAGE_UNKNOWN")?;
    }
    if let Some(Some(hh)) = &p.health {
        one_of(hh, HEALTHS, "HEALTH_UNKNOWN")?;
    }
    if p.sla_hours.is_some_and(|n| !(1..=720).contains(&n)) {
        return Err(Error::InvalidCode("SLA_INVALID"));
    }
    let mut tx = s.pool.begin().await?;
    let known: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM distribution.dsp_registry WHERE code=$1)")
            .bind(&code)
            .fetch_one(&mut *tx)
            .await?;
    if !known {
        return Err(Error::NotFound);
    }
    if let Some(o) = p.owner_id {
        active_staff(&mut tx, o).await?;
    }
    sqlx::query("INSERT INTO work.dsp_ops(code) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(&code)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE work.dsp_ops SET
            owner_id = CASE WHEN $2 THEN $3 ELSE owner_id END,
            contract_stage = COALESCE($4, contract_stage),
            contract_renewal_at = CASE WHEN $5 THEN $6 ELSE contract_renewal_at END,
            sla_hours = COALESCE($7, sla_hours),
            health = CASE WHEN $8 THEN $9 ELSE health END,
            updated_by=$10, updated_at=now()
          WHERE code=$1",
    )
    .bind(&code)
    .bind(p.owner_id.is_some())
    .bind(p.owner_id.flatten())
    .bind(&p.contract_stage)
    .bind(p.contract_renewal_at.is_some())
    .bind(p.contract_renewal_at.flatten())
    .bind(p.sla_hours)
    .bind(p.health.is_some())
    .bind(p.health.clone().flatten())
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &st, None, "work.dsp_ops_updated", &code).await?;
    tx.commit().await?;
    let all = dsp_ops(s, h).await?;
    all["items"]
        .as_array()
        .and_then(|a| a.iter().find(|d| d["code"] == code.as_str()).cloned())
        .ok_or(Error::NotFound)
}

// ---------------------------------------------------------------------------
// Analytics (audit-log based)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyticsQuery {
    pub days: Option<i32>,
}

pub async fn analytics(s: &AppState, h: &HeaderMap, q: AnalyticsQuery) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let days = q.days.unwrap_or(30).clamp(7, 90);
    let mut tx = s.pool.begin().await?;
    staff_scope(&mut tx).await?;
    // Manual = actions by staff (decisions, replies, task changes); auto = actions by services.
    let daily = sqlx::query(
        "SELECT to_char(d,'YYYY-MM-DD') AS date,
                (SELECT count(*) FROM operations.audit_events a WHERE a.occurred_at >= d AND a.occurred_at < d + interval '1 day'
                   AND a.actor_user_id IN (SELECT user_id FROM identity.staff_members) AND (a.action LIKE 'staff.%' OR a.action LIKE 'work.%')
                   AND a.action NOT IN ('staff.document_viewed','work.attachment_viewed')) AS manual,
                (SELECT count(*) FROM operations.audit_events a WHERE a.occurred_at >= d AND a.occurred_at < d + interval '1 day' AND a.actor_service IS NOT NULL) AS auto,
                (SELECT count(*) FROM work.tasks t WHERE t.created_at >= d AND t.created_at < d + interval '1 day') AS created
           FROM generate_series(date_trunc('day', now() AT TIME ZONE 'Asia/Seoul') - (($1-1) * interval '1 day'),
                                date_trunc('day', now() AT TIME ZONE 'Asia/Seoul'), interval '1 day') AS g(local),
                LATERAL (SELECT (g.local AT TIME ZONE 'Asia/Seoul') AS d) x
          ORDER BY d",
    )
    .bind(days)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| { tracing::warn!(error=%e, "work analytics daily"); Error::Database(e) })?;
    let people = sqlx::query(
        "SELECT sm.user_id,
                (SELECT count(*) FROM operations.audit_events a WHERE a.actor_user_id=sm.user_id AND a.occurred_at > now() - ($1 * interval '1 day')
                   AND (a.action LIKE 'staff.%' OR a.action LIKE 'work.%') AND a.action NOT IN ('staff.document_viewed','work.attachment_viewed')) AS done,
                COALESCE((SELECT avg(extract(epoch FROM t.completed_at - COALESCE(t.assigned_at,t.created_at)))/60 FROM work.tasks t
                   WHERE t.assignee_id=sm.user_id AND t.status='DONE' AND t.completed_at > now() - ($1 * interval '1 day')),0)::float8 AS avg_minutes,
                (SELECT count(*) FROM work.tasks t WHERE t.assignee_id=sm.user_id AND t.status NOT IN ('DONE','CANCELLED')) AS open,
                (SELECT count(*) FROM work.tasks t WHERE t.assignee_id=sm.user_id AND t.status NOT IN ('DONE','CANCELLED') AND LEAST(t.due_at,t.sla_due_at) < now()) AS overdue
           FROM identity.staff_members sm WHERE sm.status='ACTIVE'",
    )
    .bind(days)
    .fetch_all(&mut *tx)
    .await?;
    let totals = sqlx::query(
        "SELECT
           COALESCE((SELECT avg(extract(epoch FROM completed_at - COALESCE(assigned_at,created_at)))/60 FROM work.tasks
              WHERE status='DONE' AND completed_at > now() - ($1 * interval '1 day')),0)::float8 AS avg_cycle,
           (SELECT count(*) FROM operations.audit_events WHERE action IN ('staff.rejected','staff.correction_requested') AND occurred_at > now() - ($1 * interval '1 day')) AS rejected,
           (SELECT count(*) FROM operations.audit_events WHERE action IN ('staff.rejected','staff.correction_requested','staff.approved') AND occurred_at > now() - ($1 * interval '1 day')) AS decided,
           (SELECT count(*) FROM work.tasks WHERE status='DONE' AND completed_at > now() - ($1 * interval '1 day') AND COALESCE(due_at,sla_due_at) IS NOT NULL) AS with_due,
           (SELECT count(*) FROM work.tasks WHERE status='DONE' AND completed_at > now() - ($1 * interval '1 day') AND completed_at <= LEAST(due_at,sla_due_at)) AS met",
    )
    .bind(days)
    .fetch_one(&mut *tx)
    .await?;
    // Dwell per task stage, from the task event log.
    let stages = sqlx::query(
        "WITH ev AS (
           SELECT e.task_id, e.at, CASE WHEN e.action='CREATED' THEN 'TODO' WHEN e.action='REVIEW_REQUESTED' THEN 'IN_REVIEW'
                    WHEN e.action IN ('APPROVED') THEN 'DONE' WHEN e.action='CHANGES_REQUESTED' THEN 'IN_PROGRESS' ELSE e.detail END AS st
             FROM work.task_events e
            WHERE e.action IN ('CREATED','STATUS','REVIEW_REQUESTED','APPROVED','CHANGES_REQUESTED') AND e.at > now() - ($1 * interval '1 day')),
         spans AS (
           SELECT st, extract(epoch FROM COALESCE(lead(at) OVER (PARTITION BY task_id ORDER BY at), now()) - at)/3600 AS hours FROM ev)
         SELECT s.st, COALESCE(avg(sp.hours),0)::float8 AS avg_hours,
                COALESCE(percentile_cont(0.9) WITHIN GROUP (ORDER BY sp.hours),0)::float8 AS p90_hours,
                (SELECT count(*) FROM work.tasks t WHERE t.status=s.st) AS wip
           FROM unnest(ARRAY['TODO','IN_PROGRESS','IN_REVIEW','BLOCKED']) AS s(st)
           LEFT JOIN spans sp ON sp.st=s.st
          GROUP BY s.st",
    )
    .bind(days)
    .fetch_all(&mut *tx)
    .await?;
    let review_wait = sqlx::query(
        "SELECT count(*) AS wip, COALESCE(avg(extract(epoch FROM now()-ar.created_at))/3600,0)::float8 AS avg_hours,
                COALESCE(percentile_cont(0.9) WITHIN GROUP (ORDER BY extract(epoch FROM now()-ar.created_at)/3600),0)::float8 AS p90_hours
           FROM catalog.releases r JOIN catalog.application_revisions ar ON ar.org_id=r.org_id AND ar.id=r.current_revision_id
          WHERE r.status='STAGE2_REVIEW' AND r.archived_at IS NULL",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let dsp = sqlx::query(
        "SELECT d.code, d.name, count(j.id) AS total,
                count(j.id) FILTER (WHERE j.status IN ('FAILED','DEAD_LETTER')
                   OR EXISTS(SELECT 1 FROM execution.ack_events a WHERE a.job_id=j.id AND a.outcome='REJECTED')) AS failures
           FROM distribution.dsp_registry d JOIN execution.delivery_jobs j ON j.partner_id=d.code AND j.created_at > now() - ($1 * interval '1 day')
          GROUP BY d.code, d.name HAVING count(j.id) > 0",
    )
    .bind(days)
    .fetch_all(&mut *tx)
    .await?;
    let by_type = sqlx::query(
        "SELECT type, count(*) AS n FROM work.tasks WHERE status='DONE' AND completed_at > now() - ($1 * interval '1 day') GROUP BY type",
    )
    .bind(days)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    let daily: Vec<Value> = daily
        .iter()
        .map(|r| json!({"date": r.get::<String,_>("date"), "manual": r.get::<i64,_>("manual"), "auto": r.get::<i64,_>("auto"), "created": r.get::<i64,_>("created")}))
        .collect();
    let (m, a): (i64, i64) = daily.iter().fold((0, 0), |(m, a), d| {
        (
            m + d["manual"].as_i64().unwrap_or(0),
            a + d["auto"].as_i64().unwrap_or(0),
        )
    });
    let label = |k: &str| match k {
        "TODO" => "시작 전",
        "IN_PROGRESS" => "진행",
        "IN_REVIEW" => "검토 대기",
        "BLOCKED" => "막힘",
        _ => "기타",
    };
    let mut stage_items: Vec<Value> = Vec::new();
    if let Some(rw) = review_wait {
        stage_items.push(json!({"key": "release_review", "label": "발매 심사 대기", "avg_hours": rw.get::<f64,_>("avg_hours"),
            "p90_hours": rw.get::<f64,_>("p90_hours"), "wip": rw.get::<i64,_>("wip")}));
    }
    for k in ["TODO", "IN_PROGRESS", "IN_REVIEW", "BLOCKED"] {
        if let Some(r) = stages.iter().find(|r| r.get::<String, _>("st") == k) {
            stage_items.push(json!({"key": k.to_lowercase(), "label": format!("업무 · {}", label(k)), "avg_hours": r.get::<f64,_>("avg_hours"),
                "p90_hours": r.get::<f64,_>("p90_hours"), "wip": r.get::<i64,_>("wip")}));
        }
    }
    let mut dsp_items: Vec<Value> = dsp
        .iter()
        .map(|r| {
            let total: i64 = r.get("total");
            let f: i64 = r.get("failures");
            json!({"code": r.get::<String,_>("code"), "name": r.get::<String,_>("name"), "rate": f as f64 / total.max(1) as f64, "failures": f})
        })
        .collect();
    dsp_items.sort_by(|x, y| {
        y["rate"]
            .as_f64()
            .partial_cmp(&x["rate"].as_f64())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let decided: i64 = totals.get("decided");
    let with_due: i64 = totals.get("with_due");
    Ok(json!({
        "days": days,
        "daily": daily,
        "people": people.iter().map(|r| json!({"user_id": r.get::<Uuid,_>("user_id"), "done": r.get::<i64,_>("done"),
            "avg_minutes": r.get::<f64,_>("avg_minutes"), "open": r.get::<i64,_>("open"), "overdue": r.get::<i64,_>("overdue")})).collect::<Vec<_>>(),
        "avg_cycle_minutes": totals.get::<f64,_>("avg_cycle"),
        "stages": stage_items,
        "reject_rate": if decided > 0 { totals.get::<i64,_>("rejected") as f64 / decided as f64 } else { 0.0 },
        "auto_rate": if m + a > 0 { a as f64 / (m + a) as f64 } else { 0.0 },
        "sla_met_rate": if with_due > 0 { totals.get::<i64,_>("met") as f64 / with_due as f64 } else { 1.0 },
        "dsp_failure": dsp_items,
        "by_type": by_type.iter().map(|r| json!({"type": r.get::<String,_>("type"), "count": r.get::<i64,_>("n")})).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn h_summary(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(summary(&s, &h).await?))
}
async fn h_people(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(people(&s, &h).await?))
}
async fn h_inbox(
    State(s): State<AppState>,
    Query(q): Query<InboxQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(inbox(&s, &h, q).await?))
}
async fn h_read(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ReadInput>,
) -> Result<Json<Value>> {
    Ok(Json(read_inbox(&s, &h, i).await?))
}
async fn h_tasks(
    State(s): State<AppState>,
    Query(q): Query<TaskQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list_tasks(&s, &h, q).await?))
}
async fn h_create_task(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<TaskInput>,
) -> Result<Json<Value>> {
    Ok(Json(create_task(&s, &h, i).await?))
}
async fn h_task(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(get_task(&s, &h, id).await?))
}
async fn h_update_task(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<TaskPatch>,
) -> Result<Json<Value>> {
    Ok(Json(update_task(&s, &h, id, i).await?))
}
async fn h_notes(
    State(s): State<AppState>,
    Query(q): Query<NoteQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(notes(&s, &h, q).await?))
}
async fn h_add_note(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<NoteInput>,
) -> Result<Json<Value>> {
    Ok(Json(add_note(&s, &h, i).await?))
}
async fn h_resolve_note(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ResolveInput>,
) -> Result<Json<Value>> {
    Ok(Json(resolve_note(&s, &h, id, i).await?))
}
async fn h_upload(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<UploadInput>,
) -> Result<Json<Value>> {
    Ok(Json(start_upload(&s, &h, i).await?))
}
async fn h_upload_part(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<PartInput>,
) -> Result<Json<Value>> {
    Ok(Json(upload_part(&s, &h, id, i).await?))
}
async fn h_attachment(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Response> {
    attachment(&s, &h, id).await
}
async fn h_exceptions(
    State(s): State<AppState>,
    Query(q): Query<ExceptionQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(exceptions(&s, &h, q).await?))
}
async fn h_report_exception(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ExceptionReport>,
) -> Result<Json<Value>> {
    Ok(Json(report_exception(&s, &h, i).await?))
}
async fn h_act_exception(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ExceptionAction>,
) -> Result<Json<Value>> {
    Ok(Json(act_exception(&s, &h, id, i).await?))
}
async fn h_sla(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(list_sla(&s, &h).await?))
}
async fn h_save_sla(
    State(s): State<AppState>,
    Path(kind): Path<String>,
    h: HeaderMap,
    Json(p): Json<SlaPolicy>,
) -> Result<Json<Value>> {
    Ok(Json(save_sla(&s, &h, kind, p).await?))
}
async fn h_contacts(
    State(s): State<AppState>,
    Query(q): Query<ContactQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(contacts(&s, &h, q).await?))
}
async fn h_create_contact(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ContactInput>,
) -> Result<Json<Value>> {
    Ok(Json(save_contact(&s, &h, None, i).await?))
}
async fn h_contact(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(contact(&s, &h, id).await?))
}
async fn h_update_contact(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ContactInput>,
) -> Result<Json<Value>> {
    Ok(Json(save_contact(&s, &h, Some(id), i).await?))
}
async fn h_activity(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ActivityInput>,
) -> Result<Json<Value>> {
    Ok(Json(add_activity(&s, &h, id, i).await?))
}
async fn h_dsp_ops(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(dsp_ops(&s, &h).await?))
}
async fn h_update_dsp_ops(
    State(s): State<AppState>,
    Path(code): Path<String>,
    h: HeaderMap,
    Json(p): Json<DspOpsPatch>,
) -> Result<Json<Value>> {
    Ok(Json(update_dsp_ops(&s, &h, code, p).await?))
}
async fn h_analytics(
    State(s): State<AppState>,
    Query(q): Query<AnalyticsQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(analytics(&s, &h, q).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/summary", get(h_summary))
        .route("/api/staff/work/people", get(h_people))
        .route("/api/staff/work/inbox", get(h_inbox))
        .route("/api/staff/work/inbox/read", post(h_read))
        .route("/api/staff/work/tasks", get(h_tasks).post(h_create_task))
        .route(
            "/api/staff/work/tasks/{id}",
            get(h_task).post(h_update_task),
        )
        .route("/api/staff/work/notes", get(h_notes).post(h_add_note))
        .route("/api/staff/work/notes/{id}/resolve", post(h_resolve_note))
        .route("/api/staff/work/uploads", post(h_upload))
        // a base64 part of up to 48 KiB plus JSON framing
        .route(
            "/api/staff/work/uploads/{id}/parts",
            post(h_upload_part).layer(DefaultBodyLimit::max(80 * 1024)),
        )
        .route("/api/staff/work/attachments/{id}", get(h_attachment))
        .route(
            "/api/staff/work/exceptions",
            get(h_exceptions).post(h_report_exception),
        )
        .route("/api/staff/work/exceptions/{id}", post(h_act_exception))
        .route("/api/staff/work/sla", get(h_sla))
        .route("/api/staff/work/sla/{kind}", axum::routing::put(h_save_sla))
        .route(
            "/api/staff/work/contacts",
            get(h_contacts).post(h_create_contact),
        )
        .route(
            "/api/staff/work/contacts/{id}",
            get(h_contact).post(h_update_contact),
        )
        .route("/api/staff/work/contacts/{id}/activities", post(h_activity))
        .route("/api/staff/work/dsp-ops", get(h_dsp_ops))
        .route("/api/staff/work/dsp-ops/{code}", post(h_update_dsp_ops))
        .route("/api/staff/work/analytics", get(h_analytics))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kst(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(&format!("{s}+09:00"))
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn business_minutes_skip_nights_and_weekends() {
        // 2026-10-05 is a Monday
        assert_eq!(
            add_business_minutes(kst("2026-10-05T10:00:00"), 120),
            kst("2026-10-05T12:00:00")
        );
        assert_eq!(
            add_business_minutes(kst("2026-10-05T17:00:00"), 120),
            kst("2026-10-06T10:00:00")
        );
        assert_eq!(
            add_business_minutes(kst("2026-10-09T19:30:00"), 540),
            kst("2026-10-12T18:00:00")
        );
        assert_eq!(
            add_business_minutes(kst("2026-10-05T07:00:00"), 30),
            kst("2026-10-05T09:30:00")
        );
    }

    #[test]
    fn sla_states_and_escalation_match_the_screen() {
        let c = kst("2026-10-05T09:00:00");
        let due = Some(c + Duration::hours(4));
        assert_eq!(
            sla_state("TODO", c, None, due, 0.25, c + Duration::hours(1)),
            "OK"
        );
        assert_eq!(
            sla_state("TODO", c, None, due, 0.25, c + Duration::minutes(210)),
            "DUE_SOON"
        );
        assert_eq!(
            sla_state("TODO", c, None, due, 0.25, c + Duration::hours(5)),
            "OVERDUE"
        );
        assert_eq!(
            sla_state("DONE", c, Some(c + Duration::hours(3)), due, 0.25, c),
            "MET"
        );
        assert_eq!(
            sla_state("DONE", c, Some(c + Duration::hours(6)), due, 0.25, c),
            "MISSED"
        );
        assert_eq!(sla_state("CANCELLED", c, Some(c), due, 0.25, c), "NONE");
        assert_eq!(escalate("DUE_SOON", "NORMAL", true), (1, "HIGH".into()));
        assert_eq!(escalate("OVERDUE", "LOW", true), (2, "URGENT".into()));
        assert_eq!(escalate("OVERDUE", "LOW", false), (2, "LOW".into()));
        assert_eq!(escalate("OK", "LOW", true), (0, "LOW".into()));
    }

    #[test]
    fn urgent_delivery_is_two_wall_clock_hours_rights_one_business_day() {
        let urgent = SlaPolicy {
            kind: "URGENT_DELIVERY".into(),
            target_minutes: 120,
            business_hours: false,
            warn_ratio: 0.25,
            idle_alert_minutes: 30,
            escalate_to: "OPERATOR".into(),
            auto_raise_priority: true,
            enabled: true,
        };
        let rights = SlaPolicy {
            kind: "RIGHTS".into(),
            target_minutes: 540,
            business_hours: true,
            ..urgent.clone()
        };
        assert_eq!(
            sla_due(kst("2026-10-05T22:00:00"), Some(&urgent)),
            Some(kst("2026-10-06T00:00:00"))
        );
        assert_eq!(
            sla_due(kst("2026-10-05T14:00:00"), Some(&rights)),
            Some(kst("2026-10-06T14:00:00"))
        );
        assert_eq!(
            sla_due(
                kst("2026-10-05T14:00:00"),
                Some(&SlaPolicy {
                    enabled: false,
                    ..rights
                })
            ),
            None
        );
    }

    #[test]
    fn integration_kind_from_adapter_transport() {
        assert_eq!(integration_kind(None, "DDEX"), "MANUAL");
        assert_eq!(integration_kind(Some("sftp"), "DDEX"), "DDEX_SFTP");
        assert_eq!(integration_kind(Some("partner"), "DDEX"), "DDEX_API");
        assert_eq!(integration_kind(Some("partner"), "PARTNER_SPEC"), "API");
    }
}
