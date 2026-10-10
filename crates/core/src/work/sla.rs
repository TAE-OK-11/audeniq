//! SLA / deadline engine. Policies per work type (tasks) and per existing
//! queue (release review, second approval, document review, customer inquiry,
//! exception), optionally scoped to one DSP. Deadlines for the existing queues
//! are computed from the source's own start time and never stored; tasks keep
//! `sla_due_at` (from the policy at creation / type change) and an escalation
//! level that the sweep raises once per level, recorded as a task event.
//!
//! Business time uses `work.calendars` (fixed UTC offset, working hours,
//! workdays) and `work.calendar_holidays`; a policy without a calendar runs on
//! the 24-hour clock. Policies and calendars are ADMIN-only and audited.
use super::{audit, line, one_of};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff::{self, Staff, StaffRole},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::{DateTime, Datelike, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

/// Existing queues that carry an SLA (their start time comes from the source).
pub const QUEUE_TYPES: &[&str] = &[
    "RELEASE_REVIEW",
    "SECOND_APPROVAL",
    "DOCUMENT_REVIEW",
    "SUPPORT_INQUIRY",
    "EXCEPTION",
];
const ROLES: &[&str] = &["ADMIN", "REVIEWER", "OPERATOR", "SUPPORT"];

fn admin(st: &Staff) -> Result<()> {
    if st.role == StaffRole::Admin {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

// ---------------------------------------------------------------------------
// Calendars and business time
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Calendar {
    pub id: String,
    pub name: String,
    pub offset: i32,
    pub start: NaiveTime,
    pub end: NaiveTime,
    pub workdays: Vec<i16>,
    pub holidays: HashSet<NaiveDate>,
}

impl Calendar {
    fn tz(&self) -> FixedOffset {
        FixedOffset::east_opt(self.offset * 60).unwrap_or(FixedOffset::east_opt(0).unwrap())
    }
    fn working(&self, d: NaiveDate) -> bool {
        self.workdays
            .contains(&(d.weekday().number_from_monday() as i16))
            && !self.holidays.contains(&d)
    }
    /// `from` plus `minutes` of working time.
    pub fn add(&self, from: DateTime<Utc>, minutes: i64) -> DateTime<Utc> {
        let tz = self.tz();
        let mut cursor = from.with_timezone(&tz).naive_local();
        let mut left = Duration::minutes(minutes.max(0));
        for _ in 0..3700 {
            let day = cursor.date();
            if self.working(day) {
                let open = day.and_time(self.start);
                let close = day.and_time(self.end);
                let at = cursor.max(open);
                if at < close {
                    let avail = close - at;
                    if left <= avail {
                        return tz
                            .from_local_datetime(&(at + left))
                            .single()
                            .map(|d| d.with_timezone(&Utc))
                            .unwrap_or(from);
                    }
                    left -= avail;
                }
            }
            cursor = (day + Duration::days(1)).and_time(NaiveTime::MIN);
        }
        from + Duration::minutes(minutes)
    }
}

pub(crate) async fn calendars(c: &mut PgConnection) -> Result<HashMap<String, Calendar>> {
    let rows = sqlx::query(
        "SELECT id,name,utc_offset_minutes,work_start,work_end,workdays FROM work.calendars ORDER BY id",
    )
    .fetch_all(&mut *c)
    .await?;
    let hol = sqlx::query("SELECT calendar_id, day FROM work.calendar_holidays")
        .fetch_all(&mut *c)
        .await?;
    let mut out: HashMap<String, Calendar> = rows
        .iter()
        .map(|r| {
            let id: String = r.get("id");
            (
                id.clone(),
                Calendar {
                    id,
                    name: r.get("name"),
                    offset: r.get("utc_offset_minutes"),
                    start: r.get("work_start"),
                    end: r.get("work_end"),
                    workdays: r.get("workdays"),
                    holidays: HashSet::new(),
                },
            )
        })
        .collect();
    for h in hol {
        if let Some(c) = out.get_mut(&h.get::<String, _>("calendar_id")) {
            c.holidays.insert(h.get("day"));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Policies
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Policy {
    pub id: Uuid,
    pub work_type: String,
    pub scope_kind: Option<String>,
    pub scope_id: Option<String>,
    pub target_minutes: i32,
    pub calendar_id: Option<String>,
    pub warn_ratio: f32,
    pub idle_alert_minutes: i32,
    pub escalate_to: String,
    pub auto_raise_priority: bool,
    pub enabled: bool,
    pub updated_at: DateTime<Utc>,
}

impl Policy {
    fn json(&self) -> Value {
        json!({
            "id": self.id, "type": self.work_type, "scope_kind": self.scope_kind, "scope_id": self.scope_id,
            "target_minutes": self.target_minutes, "calendar_id": self.calendar_id, "warn_ratio": self.warn_ratio,
            "idle_alert_minutes": self.idle_alert_minutes, "escalate_to": self.escalate_to,
            "auto_raise_priority": self.auto_raise_priority, "enabled": self.enabled, "updated_at": self.updated_at,
        })
    }
}

/// Active policies and calendars, loaded once per request.
pub struct Book {
    pub policies: Vec<Policy>,
    pub calendars: HashMap<String, Calendar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State3 {
    Ok,
    Warning,
    Breached,
}

impl State3 {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warning => "WARNING",
            Self::Breached => "BREACHED",
        }
    }
}

impl Book {
    pub async fn load(c: &mut PgConnection) -> Result<Self> {
        let rows = sqlx::query(
            "SELECT id,work_type,scope_kind,scope_id,target_minutes,calendar_id,warn_ratio,idle_alert_minutes,escalate_to,
                    auto_raise_priority,enabled,updated_at
               FROM work.sla_policies WHERE archived_at IS NULL ORDER BY work_type, scope_id NULLS FIRST",
        )
        .fetch_all(&mut *c)
        .await?;
        let policies = rows
            .iter()
            .map(|r| Policy {
                id: r.get("id"),
                work_type: r.get("work_type"),
                scope_kind: r.get("scope_kind"),
                scope_id: r.get("scope_id"),
                target_minutes: r.get("target_minutes"),
                calendar_id: r.get("calendar_id"),
                warn_ratio: r.get("warn_ratio"),
                idle_alert_minutes: r.get("idle_alert_minutes"),
                escalate_to: r.get("escalate_to"),
                auto_raise_priority: r.get("auto_raise_priority"),
                enabled: r.get("enabled"),
                updated_at: r.get("updated_at"),
            })
            .collect();
        Ok(Self {
            policies,
            calendars: calendars(c).await?,
        })
    }

    /// The enabled policy for a work type, a DSP-scoped one first.
    pub fn policy(&self, work_type: &str, dsp: Option<&str>) -> Option<&Policy> {
        let mine = |p: &&Policy| p.enabled && p.work_type == work_type;
        dsp.and_then(|d| {
            self.policies.iter().filter(mine).find(|p| {
                p.scope_kind.as_deref() == Some("dsp") && p.scope_id.as_deref() == Some(d)
            })
        })
        .or_else(|| {
            self.policies
                .iter()
                .filter(mine)
                .find(|p| p.scope_kind.is_none())
        })
    }

    pub fn due(&self, p: &Policy, start: DateTime<Utc>) -> DateTime<Utc> {
        match p.calendar_id.as_ref().and_then(|c| self.calendars.get(c)) {
            Some(cal) => cal.add(start, p.target_minutes as i64),
            None => start + Duration::minutes(p.target_minutes as i64),
        }
    }

    pub fn due_for(
        &self,
        work_type: &str,
        dsp: Option<&str>,
        start: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        self.policy(work_type, dsp).map(|p| self.due(p, start))
    }

    /// OK / WARNING (less than `warn_ratio` of the window left) / BREACHED.
    pub fn state(
        &self,
        warn_ratio: f32,
        start: DateTime<Utc>,
        due: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> State3 {
        if now >= due {
            return State3::Breached;
        }
        let total = (due - start).num_seconds().max(1) as f64;
        let left = (due - now).num_seconds() as f64;
        if left / total <= warn_ratio as f64 {
            State3::Warning
        } else {
            State3::Ok
        }
    }
}

/// Deadline state of one task (the earlier of its own due date and its SLA).
pub fn task_deadline(
    due_at: Option<DateTime<Utc>>,
    sla_due_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    closed: bool,
    warn_ratio: f32,
) -> (Option<DateTime<Utc>>, Option<&'static str>) {
    let deadline = match (due_at, sla_due_at) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    let state = match deadline {
        Some(d) if !closed => {
            let now = Utc::now();
            Some(if now >= d {
                "BREACHED"
            } else {
                let total = (d - created_at).num_seconds().max(1) as f64;
                if ((d - now).num_seconds() as f64) / total <= warn_ratio as f64 {
                    "WARNING"
                } else {
                    "OK"
                }
            })
        }
        _ => None,
    };
    (deadline, state)
}

/// SLA due date for a new task (or one whose type changed).
pub(crate) async fn task_due(
    c: &mut PgConnection,
    work_type: &str,
    start: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>> {
    let book = Book::load(c).await?;
    Ok(book.due_for(work_type, None, start))
}

// ---------------------------------------------------------------------------
// Existing queues with deadlines (read from the sources, never stored)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct QueueItem {
    pub queue: &'static str,
    pub id: Uuid,
    pub title: String,
    pub org_name: Option<String>,
    pub start: DateTime<Utc>,
    pub due: DateTime<Utc>,
    pub state: State3,
    pub escalate_to: String,
    pub link: String,
    /// who is already on it (release review claim)
    pub owner: Option<Uuid>,
    /// who asked (second approval: the requester may not approve it)
    pub requester: Option<Uuid>,
}

impl QueueItem {
    pub fn json(&self) -> Value {
        json!({
            "queue": self.queue, "id": self.id, "title": self.title, "org_name": self.org_name,
            "start": self.start, "due_at": self.due, "state": self.state.as_str(), "escalate_to": self.escalate_to,
            "link": self.link, "owner_id": self.owner,
        })
    }
}

/// Open items of the existing queues with their SLA state. Release review
/// starts at submission, second approval at the request, document review at
/// upload, a customer inquiry at its last customer message.
pub async fn queue_items(c: &mut PgConnection, book: &Book) -> Result<Vec<QueueItem>> {
    let rows = sqlx::query(
        "SELECT 'RELEASE_REVIEW' AS q, r.id, r.title, o.name AS org_name, COALESCE(ar.created_at, r.created_at) AS start,
                (SELECT claimed_by FROM rights.review_claims c WHERE c.release_id=r.id) AS owner, NULL::uuid AS requester
           FROM catalog.releases r JOIN identity.orgs o ON o.id=r.org_id
           LEFT JOIN catalog.application_revisions ar ON ar.org_id=r.org_id AND ar.id=r.current_revision_id
          WHERE r.archived_at IS NULL AND (r.status='STAGE2_REVIEW' OR (r.status='READY_FOR_DELIVERY' AND EXISTS(
                SELECT 1 FROM portal.documents d WHERE d.org_id=r.org_id AND d.release_id=r.id
                 AND d.kind='AGREEMENT' AND d.status IN ('REVIEW','PREPARED'))))
         UNION ALL
         SELECT 'SECOND_APPROVAL', a.id, r.title, o.name, a.created_at, NULL, a.requested_by
           FROM rights.staff_approvals a JOIN catalog.releases r ON r.id=a.release_id JOIN identity.orgs o ON o.id=a.org_id
          WHERE a.status='PENDING' AND a.expires_at>now() AND a.revision_id=r.current_revision_id
            AND r.status='STAGE2_REVIEW' AND r.archived_at IS NULL
         UNION ALL
         SELECT 'DOCUMENT_REVIEW', d.id, d.title, o.name, d.updated_at, NULL, NULL
           FROM portal.documents d JOIN identity.orgs o ON o.id=d.org_id
          WHERE d.kind='RIGHTS_PROOF' AND d.status='REVIEW'
         UNION ALL
         SELECT 'SUPPORT_INQUIRY', i.id, i.subject, o.name,
                COALESCE((SELECT max(m.created_at) FROM portal.inquiry_messages m WHERE m.inquiry_id=i.id AND m.author_kind<>'STAFF'), i.created_at),
                NULL, NULL
           FROM portal.inquiries i JOIN identity.orgs o ON o.id=i.org_id
          WHERE i.status='OPEN'
         LIMIT 2000",
    )
    .fetch_all(&mut *c)
    .await?;
    let now = Utc::now();
    let mut out = Vec::new();
    for r in rows {
        let q: String = r.get("q");
        let queue = QUEUE_TYPES
            .iter()
            .copied()
            .find(|t| *t == q)
            .unwrap_or("EXCEPTION");
        let Some(p) = book.policy(queue, None) else {
            continue;
        };
        let start: DateTime<Utc> = r.get("start");
        let due = book.due(p, start);
        let id: Uuid = r.get("id");
        let link = match queue {
            "RELEASE_REVIEW" => format!("/admin/reviews/{id}"),
            "SECOND_APPROVAL" => "/admin/approvals".to_string(),
            "DOCUMENT_REVIEW" => "/admin/documents".to_string(),
            _ => format!("/admin/inquiries/{id}"),
        };
        out.push(QueueItem {
            queue,
            id,
            title: r.get("title"),
            org_name: r.get("org_name"),
            start,
            due,
            state: book.state(p.warn_ratio, start, due, now),
            escalate_to: p.escalate_to.clone(),
            link,
            owner: r.get("owner"),
            requester: r.get("requester"),
        });
    }
    out.sort_by_key(|i| i.due);
    Ok(out)
}

// ---------------------------------------------------------------------------
// Task escalation sweep
// ---------------------------------------------------------------------------

const RAISE: &[(&str, &str)] = &[("LOW", "NORMAL"), ("NORMAL", "HIGH"), ("HIGH", "URGENT")];

/// Escalate open tasks past their deadline: level 1 at the deadline (priority
/// raised one step when the policy says so), level 2 when it is overdue by
/// another full SLA window (escalated to the policy's role). Idempotent and
/// cheap when nothing is late; runs from staff reads (inbox, summary, SLA
/// board), so nothing depends on a scheduler.
pub async fn sweep(pool: &sqlx::PgPool) -> Result<()> {
    let due: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM work.tasks WHERE archived_at IS NULL AND status NOT IN ('DONE','CANCELLED')
           AND escalation_level < 2 AND LEAST(COALESCE(due_at,'infinity'), COALESCE(sla_due_at,'infinity')) <= now())",
    )
    .fetch_one(pool)
    .await?;
    if !due {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    let book = Book::load(&mut tx).await?;
    let rows = sqlx::query(
        "SELECT id, work_type, priority, escalation_level, created_at,
                LEAST(COALESCE(due_at,'infinity'), COALESCE(sla_due_at,'infinity')) AS deadline
           FROM work.tasks
          WHERE archived_at IS NULL AND status NOT IN ('DONE','CANCELLED') AND escalation_level < 2
            AND LEAST(COALESCE(due_at,'infinity'), COALESCE(sla_due_at,'infinity')) <= now()
          ORDER BY 6 LIMIT 500
          FOR UPDATE SKIP LOCKED",
    )
    .fetch_all(&mut *tx)
    .await?;
    let now = Utc::now();
    for r in rows {
        let id: Uuid = r.get("id");
        let work_type: String = r.get("work_type");
        let priority: String = r.get("priority");
        let level: i16 = r.get("escalation_level");
        let created: DateTime<Utc> = r.get("created_at");
        let deadline: DateTime<Utc> = r.get("deadline");
        let policy = book.policy(&work_type, None);
        let window = (deadline - created).max(Duration::minutes(
            policy.map(|p| p.target_minutes as i64).unwrap_or(60),
        ));
        let target = if now >= deadline + window { 2 } else { 1 };
        if level >= target {
            continue;
        }
        let raise = policy.is_none_or(|p| p.auto_raise_priority) && level == 0;
        let next_priority = if raise {
            RAISE
                .iter()
                .find(|(a, _)| *a == priority)
                .map(|(_, b)| *b)
                .unwrap_or(priority.as_str())
                .to_string()
        } else {
            priority.clone()
        };
        sqlx::query(
            "UPDATE work.tasks SET escalation_level=$2, priority=$3,
                    priority_raised=CASE WHEN $3<>priority AND priority_raised IS NULL THEN priority ELSE priority_raised END,
                    updated_at=now(), row_version=row_version+1
              WHERE id=$1",
        )
        .bind(id)
        .bind(target as i16)
        .bind(&next_priority)
        .execute(&mut *tx)
        .await?;
        let to = policy.map(|p| p.escalate_to.as_str()).unwrap_or("ADMIN");
        let detail = if next_priority != priority {
            format!("L{target} → {to} · {priority}→{next_priority}")
        } else {
            format!("L{target} → {to}")
        };
        super::tasks::event(&mut tx, id, None, "SLA_ESCALATED", Some(&detail)).await?;
        crate::operations::audit(
            &mut tx,
            None,
            None,
            Some(id),
            "work.task_escalated",
            if target == 2 { "SLA_L2" } else { "SLA_L1" },
            Uuid::new_v4(),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// Policies, calendars (with holidays) and the deadline board of the existing
/// queues and open tasks.
pub async fn overview(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff::staff(s, h, false).await?;
    sweep(&s.pool).await?;
    let mut c = s.pool.acquire().await?;
    let book = Book::load(&mut c).await?;
    let queue = queue_items(&mut c, &book).await?;
    let tasks = sqlx::query(
        "SELECT t.id, t.seq, t.title, t.work_type, t.priority, t.assignee_id, t.due_at, t.sla_due_at, t.created_at,
                t.escalation_level
           FROM work.tasks t
          WHERE t.archived_at IS NULL AND t.status NOT IN ('DONE','CANCELLED') AND (t.due_at IS NOT NULL OR t.sla_due_at IS NOT NULL)
          ORDER BY LEAST(COALESCE(t.due_at,'infinity'), COALESCE(t.sla_due_at,'infinity')) LIMIT 300",
    )
    .fetch_all(&mut *c)
    .await?;
    let tasks: Vec<Value> = tasks
        .iter()
        .map(|r| {
            let wt: String = r.get("work_type");
            let warn = book.policy(&wt, None).map(|p| p.warn_ratio).unwrap_or(0.25);
            let (deadline, state) = task_deadline(
                r.get("due_at"),
                r.get("sla_due_at"),
                r.get("created_at"),
                false,
                warn,
            );
            json!({
                "id": r.get::<Uuid,_>("id"), "key": format!("WRK-{}", r.get::<i64,_>("seq")), "title": r.get::<String,_>("title"),
                "type": wt, "priority": r.get::<String,_>("priority"), "assignee_id": r.get::<Option<Uuid>,_>("assignee_id"),
                "deadline": deadline, "state": state, "escalation_level": r.get::<i16,_>("escalation_level"),
            })
        })
        .collect();
    let mut board: Vec<Value> = Vec::new();
    for q in QUEUE_TYPES.iter().filter(|q| **q != "EXCEPTION") {
        let mine: Vec<&QueueItem> = queue.iter().filter(|i| i.queue == *q).collect();
        board.push(json!({
            "queue": q, "open": mine.len(),
            "warning": mine.iter().filter(|i| i.state == State3::Warning).count(),
            "breached": mine.iter().filter(|i| i.state == State3::Breached).count(),
        }));
    }
    let mut cals: Vec<&Calendar> = book.calendars.values().collect();
    cals.sort_by(|a, b| a.id.cmp(&b.id));
    let today = Utc::now().date_naive() - Duration::days(1);
    let calendars: Vec<Value> = cals
        .iter()
        .map(|c| {
            let mut hol: Vec<&NaiveDate> = c.holidays.iter().filter(|d| **d >= today).collect();
            hol.sort();
            json!({
                "id": c.id, "name": c.name, "utc_offset_minutes": c.offset,
                "work_start": c.start.format("%H:%M").to_string(), "work_end": c.end.format("%H:%M").to_string(),
                "workdays": c.workdays, "holidays": hol.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
            })
        })
        .collect();
    let names: HashMap<NaiveDate, String> =
        sqlx::query("SELECT day, name FROM work.calendar_holidays")
            .fetch_all(&mut *c)
            .await?
            .iter()
            .map(|r| (r.get("day"), r.get("name")))
            .collect();
    Ok(json!({
        "policies": book.policies.iter().map(Policy::json).collect::<Vec<_>>(),
        "calendars": calendars,
        "holiday_names": names.iter().map(|(d, n)| (d.to_string(), json!(n))).collect::<serde_json::Map<_,_>>(),
        "board": board,
        "queue": queue.iter().filter(|i| i.state != State3::Ok).take(200).map(QueueItem::json).collect::<Vec<_>>(),
        "tasks": tasks,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyInput {
    /// absent = new policy
    pub id: Option<Uuid>,
    #[serde(rename = "type")]
    pub work_type: String,
    #[serde(default)]
    pub scope_kind: Option<String>,
    #[serde(default)]
    pub scope_id: Option<String>,
    pub target_minutes: i32,
    #[serde(default)]
    pub calendar_id: Option<String>,
    pub warn_ratio: f32,
    pub idle_alert_minutes: i32,
    pub escalate_to: String,
    pub auto_raise_priority: bool,
    pub enabled: bool,
}

pub async fn save_policy(s: &AppState, h: &HeaderMap, i: PolicyInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    admin(&st)?;
    let mut types: Vec<&str> = super::tasks::WORK_TYPES.to_vec();
    types.extend(QUEUE_TYPES);
    one_of(&i.work_type, &types, "WORK_TYPE_UNKNOWN")?;
    one_of(&i.escalate_to, ROLES, "ROLE_UNKNOWN")?;
    if !(15..=200_000).contains(&i.target_minutes) {
        return Err(Error::InvalidCode("SLA_TARGET_RANGE"));
    }
    if !(10..=200_000).contains(&i.idle_alert_minutes) {
        return Err(Error::InvalidCode("SLA_IDLE_RANGE"));
    }
    if !(i.warn_ratio > 0.0 && i.warn_ratio < 1.0) {
        return Err(Error::InvalidCode("SLA_WARN_RANGE"));
    }
    let scope = match (i.scope_kind.as_deref(), i.scope_id.as_deref()) {
        (Some("dsp"), Some(d)) if crate::dsp_registry::Dsp::from_code(d).is_some() => {
            Some(d.to_string())
        }
        (None, None) => None,
        _ => return Err(Error::InvalidCode("SLA_SCOPE_INVALID")),
    };
    let mut tx = s.pool.begin().await?;
    if let Some(cal) = &i.calendar_id {
        let ok: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work.calendars WHERE id=$1)")
                .bind(cal)
                .fetch_one(&mut *tx)
                .await?;
        if !ok {
            return Err(Error::InvalidCode("CALENDAR_UNKNOWN"));
        }
    }
    let id = match i.id {
        Some(id) => {
            let n = sqlx::query(
                "UPDATE work.sla_policies SET work_type=$2, scope_kind=$3, scope_id=$4, target_minutes=$5, calendar_id=$6,
                        warn_ratio=$7, idle_alert_minutes=$8, escalate_to=$9, auto_raise_priority=$10, enabled=$11,
                        updated_by=$12, updated_at=now()
                  WHERE id=$1 AND archived_at IS NULL",
            )
            .bind(id).bind(&i.work_type).bind(scope.as_ref().map(|_| "dsp")).bind(&scope).bind(i.target_minutes)
            .bind(&i.calendar_id).bind(i.warn_ratio).bind(i.idle_alert_minutes).bind(&i.escalate_to)
            .bind(i.auto_raise_priority).bind(i.enabled).bind(st.actor.user)
            .execute(&mut *tx)
            .await
            .map_err(dup)?;
            if n.rows_affected() == 0 {
                return Err(Error::NotFound);
            }
            id
        }
        None => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO work.sla_policies(id,work_type,scope_kind,scope_id,target_minutes,calendar_id,warn_ratio,
                        idle_alert_minutes,escalate_to,auto_raise_priority,enabled,updated_by)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
            )
            .bind(id).bind(&i.work_type).bind(scope.as_ref().map(|_| "dsp")).bind(&scope).bind(i.target_minutes)
            .bind(&i.calendar_id).bind(i.warn_ratio).bind(i.idle_alert_minutes).bind(&i.escalate_to)
            .bind(i.auto_raise_priority).bind(i.enabled).bind(st.actor.user)
            .execute(&mut *tx)
            .await
            .map_err(dup)?;
            id
        }
    };
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.sla_policy_saved",
        &i.work_type,
    )
    .await?;
    tx.commit().await?;
    overview(s, h).await
}

fn dup(e: sqlx::Error) -> Error {
    if let sqlx::Error::Database(d) = &e
        && d.code().as_deref() == Some("23505")
    {
        return Error::InvalidCode("SLA_POLICY_EXISTS");
    }
    e.into()
}

pub async fn archive_policy(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    admin(&st)?;
    let mut tx = s.pool.begin().await?;
    let t: Option<String> = sqlx::query_scalar(
        "UPDATE work.sla_policies SET archived_at=now(), archived_by=$2 WHERE id=$1 AND archived_at IS NULL RETURNING work_type",
    )
    .bind(id)
    .bind(st.actor.user)
    .fetch_optional(&mut *tx)
    .await?;
    let t = t.ok_or(Error::NotFound)?;
    audit(&mut tx, &st, Some(id), "work.sla_policy_archived", &t).await?;
    tx.commit().await?;
    overview(s, h).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarInput {
    pub name: String,
    pub work_start: String,
    pub work_end: String,
    pub workdays: Vec<i16>,
}

pub async fn save_calendar(
    s: &AppState,
    h: &HeaderMap,
    id: String,
    i: CalendarInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    admin(&st)?;
    let name = line(&i.name, 80)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    let t = |v: &str| {
        NaiveTime::parse_from_str(v, "%H:%M").map_err(|_| Error::InvalidCode("TIME_INVALID"))
    };
    let (a, b) = (t(&i.work_start)?, t(&i.work_end)?);
    if b <= a {
        return Err(Error::InvalidCode("WORK_HOURS_INVALID"));
    }
    let mut days = i.workdays.clone();
    days.sort();
    days.dedup();
    if days.is_empty() || days.iter().any(|d| !(1..=7).contains(d)) {
        return Err(Error::InvalidCode("WORKDAYS_INVALID"));
    }
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query(
        "UPDATE work.calendars SET name=$2, work_start=$3, work_end=$4, workdays=$5, updated_by=$6, updated_at=now() WHERE id=$1",
    )
    .bind(&id)
    .bind(&name)
    .bind(a)
    .bind(b)
    .bind(&days)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    if n.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    audit(&mut tx, &st, None, "work.calendar_saved", &id).await?;
    tx.commit().await?;
    overview(s, h).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolidayInput {
    pub day: NaiveDate,
    /// absent or empty = remove the holiday
    #[serde(default)]
    pub name: Option<String>,
}

pub async fn set_holiday(
    s: &AppState,
    h: &HeaderMap,
    id: String,
    i: HolidayInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    admin(&st)?;
    let name = i.name.as_deref().map(|n| line(n, 80)).transpose()?;
    let mut tx = s.pool.begin().await?;
    let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM work.calendars WHERE id=$1)")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    if !ok {
        return Err(Error::NotFound);
    }
    match name.filter(|n| !n.is_empty()) {
        Some(n) => {
            sqlx::query(
                "INSERT INTO work.calendar_holidays(calendar_id,day,name,created_by) VALUES($1,$2,$3,$4)
                 ON CONFLICT (calendar_id, day) DO UPDATE SET name=EXCLUDED.name",
            )
            .bind(&id)
            .bind(i.day)
            .bind(&n)
            .bind(st.actor.user)
            .execute(&mut *tx)
            .await?;
            audit(&mut tx, &st, None, "work.holiday_set", &i.day.to_string()).await?;
        }
        None => {
            sqlx::query("DELETE FROM work.calendar_holidays WHERE calendar_id=$1 AND day=$2")
                .bind(&id)
                .bind(i.day)
                .execute(&mut *tx)
                .await?;
            audit(
                &mut tx,
                &st,
                None,
                "work.holiday_removed",
                &i.day.to_string(),
            )
            .await?;
        }
    }
    tx.commit().await?;
    overview(s, h).await
}

async fn h_overview(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(overview(&s, &h).await?))
}
async fn h_save_policy(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<PolicyInput>,
) -> Result<Json<Value>> {
    Ok(Json(save_policy(&s, &h, i).await?))
}
async fn h_archive_policy(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(archive_policy(&s, &h, id).await?))
}
async fn h_calendar(
    State(s): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(i): Json<CalendarInput>,
) -> Result<Json<Value>> {
    Ok(Json(save_calendar(&s, &h, id, i).await?))
}
async fn h_holiday(
    State(s): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(i): Json<HolidayInput>,
) -> Result<Json<Value>> {
    Ok(Json(set_holiday(&s, &h, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/sla", get(h_overview))
        .route("/api/staff/work/sla/policies", post(h_save_policy))
        .route(
            "/api/staff/work/sla/policies/{id}/archive",
            post(h_archive_policy),
        )
        .route("/api/staff/work/calendars/{id}", post(h_calendar))
        .route("/api/staff/work/calendars/{id}/holidays", post(h_holiday))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kr() -> Calendar {
        Calendar {
            id: "KR".into(),
            name: "kr".into(),
            offset: 540,
            start: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            workdays: vec![1, 2, 3, 4, 5],
            holidays: [NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()].into(),
        }
    }
    fn kst(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn business_minutes_skip_nights_weekends_and_holidays() {
        let c = kr();
        // Fri 17:00 + 2h → Mon is a holiday → Tue 10:00
        assert_eq!(
            c.add(kst("2026-10-02T17:00:00+09:00"), 120),
            kst("2026-10-06T10:00:00+09:00")
        );
        // before opening counts from 09:00
        assert_eq!(
            c.add(kst("2026-10-07T06:00:00+09:00"), 540),
            kst("2026-10-07T18:00:00+09:00")
        );
        // a full working day over the end of the day rolls to the next morning
        assert_eq!(
            c.add(kst("2026-10-07T12:00:00+09:00"), 540),
            kst("2026-10-08T12:00:00+09:00")
        );
    }

    #[test]
    fn deadline_state() {
        let start = Utc::now() - Duration::hours(9);
        let (_, s) = task_deadline(Some(start + Duration::hours(10)), None, start, false, 0.25);
        assert_eq!(s, Some("WARNING"));
        let (_, s) = task_deadline(None, Some(start + Duration::hours(1)), start, false, 0.25);
        assert_eq!(s, Some("BREACHED"));
        let (_, s) = task_deadline(None, Some(start + Duration::hours(100)), start, false, 0.25);
        assert_eq!(s, Some("OK"));
        let (_, s) = task_deadline(None, Some(start), start, true, 0.25);
        assert_eq!(s, None);
    }
}
