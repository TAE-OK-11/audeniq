//! General task engine: tasks and one level of subtasks, assignee, reviewer,
//! priority, status, due date, recurrence and links to any AUDENIQ entity
//! (`work.links`). Status changes, assignment, review decisions, links and
//! archive are recorded as task events and audited (`work.*`).
use super::{Act, active_staff, audit, entities, line, one_of, permit, text};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff,
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

pub const WORK_TYPES: &[&str] = &[
    "GENERAL",
    "URGENT_DELIVERY",
    "RELEASE",
    "RIGHTS",
    "CONTRACT",
    "ARTIST",
    "DSP_INQUIRY",
    "SETTLEMENT",
    "SUPPORT",
    "LEGAL",
];
const STATUSES: &[&str] = &[
    "TODO",
    "IN_PROGRESS",
    "IN_REVIEW",
    "BLOCKED",
    "DONE",
    "CANCELLED",
];
pub const PRIORITIES: &[&str] = &["LOW", "NORMAL", "HIGH", "URGENT"];

macro_rules! task_cols {
    () => {
        "t.id,t.seq,t.parent_id,t.title,t.description,t.work_type,t.status,t.priority,t.assignee_id,t.reviewer_id,t.due_at,
         t.changes_requested,t.recurrence,t.recurred_from,t.tags,t.created_by,t.created_at,t.updated_at,t.assigned_at,t.started_at,
         t.completed_at,t.archived_at,t.row_version,t.sla_due_at,t.escalation_level,t.priority_raised,
         COALESCE((SELECT p.warn_ratio FROM work.sla_policies p WHERE p.work_type=t.work_type AND p.scope_kind IS NULL
                    AND p.archived_at IS NULL AND p.enabled),0.25)::real AS warn_ratio,
         (SELECT count(*) FROM work.tasks c WHERE c.parent_id=t.id AND c.archived_at IS NULL) AS subtasks_total,
         (SELECT count(*) FROM work.tasks c WHERE c.parent_id=t.id AND c.archived_at IS NULL AND c.status IN ('DONE','CANCELLED')) AS subtasks_done,
         (SELECT count(*) FROM work.notes n WHERE n.target_kind='task' AND n.target_id=t.id::text AND n.archived_at IS NULL) AS note_count"
    };
}

#[derive(Debug, Clone)]
pub(crate) struct Task {
    pub id: Uuid,
    pub seq: i64,
    pub parent_id: Option<Uuid>,
    pub title: String,
    pub description: String,
    pub work_type: String,
    pub status: String,
    pub priority: String,
    pub assignee_id: Option<Uuid>,
    pub reviewer_id: Option<Uuid>,
    pub due_at: Option<DateTime<Utc>>,
    pub changes_requested: bool,
    pub recurrence: Option<Value>,
    pub recurred_from: Option<Uuid>,
    pub tags: Vec<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub assigned_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub row_version: i32,
    pub sla_due_at: Option<DateTime<Utc>>,
    pub escalation_level: i16,
    pub priority_raised: Option<String>,
    pub warn_ratio: f32,
    pub subtasks_total: i64,
    pub subtasks_done: i64,
    pub note_count: i64,
}

impl Task {
    fn from_row(r: &PgRow) -> Self {
        Self {
            id: r.get("id"),
            seq: r.get("seq"),
            parent_id: r.get("parent_id"),
            title: r.get("title"),
            description: r.get("description"),
            work_type: r.get("work_type"),
            status: r.get("status"),
            priority: r.get("priority"),
            assignee_id: r.get("assignee_id"),
            reviewer_id: r.get("reviewer_id"),
            due_at: r.get("due_at"),
            changes_requested: r.get("changes_requested"),
            recurrence: r.get("recurrence"),
            recurred_from: r.get("recurred_from"),
            tags: r.get("tags"),
            created_by: r.get("created_by"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
            assigned_at: r.get("assigned_at"),
            started_at: r.get("started_at"),
            completed_at: r.get("completed_at"),
            archived_at: r.get("archived_at"),
            row_version: r.get("row_version"),
            sla_due_at: r.get("sla_due_at"),
            escalation_level: r.get("escalation_level"),
            priority_raised: r.get("priority_raised"),
            warn_ratio: r.get("warn_ratio"),
            subtasks_total: r.get("subtasks_total"),
            subtasks_done: r.get("subtasks_done"),
            note_count: r.get("note_count"),
        }
    }
    fn closed(&self) -> bool {
        matches!(self.status.as_str(), "DONE" | "CANCELLED")
    }
    fn json(&self, links: &[Value]) -> Value {
        let (deadline, sla_state) = super::sla::task_deadline(
            self.due_at,
            self.sla_due_at,
            self.created_at,
            self.closed(),
            self.warn_ratio,
        );
        json!({
            "id": self.id, "key": format!("WRK-{}", self.seq), "parent_id": self.parent_id,
            "title": self.title, "description": self.description, "type": self.work_type,
            "status": self.status, "priority": self.priority,
            "assignee_id": self.assignee_id, "reviewer_id": self.reviewer_id, "due_at": self.due_at,
            "overdue": !self.closed() && self.due_at.is_some_and(|d| d < Utc::now()),
            "sla_due_at": self.sla_due_at, "deadline": deadline, "sla_state": sla_state,
            "escalation_level": self.escalation_level, "priority_raised": self.priority_raised,
            "changes_requested": self.changes_requested, "recurrence": self.recurrence, "recurred_from": self.recurred_from,
            "tags": self.tags, "created_by": self.created_by, "created_at": self.created_at, "updated_at": self.updated_at,
            "assigned_at": self.assigned_at, "started_at": self.started_at, "completed_at": self.completed_at,
            "archived_at": self.archived_at, "row_version": self.row_version,
            "subtasks": {"total": self.subtasks_total, "done": self.subtasks_done},
            "note_count": self.note_count, "links": links,
        })
    }
}

pub(crate) async fn event(
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

async fn load(c: &mut PgConnection, id: Uuid, lock: bool) -> Result<Task> {
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

/// Active links of the given tasks, labels resolved from the source tables.
async fn links_of(c: &mut PgConnection, tasks: &[Uuid]) -> Result<HashMap<Uuid, Vec<Value>>> {
    let rows = sqlx::query(
        "SELECT id, subject_id, entity_kind, entity_id FROM work.links
          WHERE subject_kind='task' AND subject_id = ANY($1) AND archived_at IS NULL ORDER BY created_at",
    )
    .bind(tasks)
    .fetch_all(&mut *c)
    .await?;
    let refs: Vec<entities::Ref> = rows
        .iter()
        .map(|r| entities::Ref {
            kind: r.get("entity_kind"),
            id: r.get("entity_id"),
        })
        .collect();
    let labels = entities::resolve(c, &refs).await?;
    let mut out: HashMap<Uuid, Vec<Value>> = HashMap::new();
    for (r, rf) in rows.iter().zip(refs.iter()) {
        let mut v = labels.get(rf).cloned().unwrap_or(Value::Null);
        v["link_id"] = json!(r.get::<Uuid, _>("id"));
        out.entry(r.get("subject_id")).or_default().push(v);
    }
    Ok(out)
}

async fn add_link(
    c: &mut PgConnection,
    st: &staff::Staff,
    task: Uuid,
    r: &entities::Ref,
) -> Result<Option<Uuid>> {
    entities::require(c, r).await?;
    if r.kind == "task" && r.id == task.to_string() {
        return Err(Error::InvalidCode("LINK_SELF"));
    }
    let id = Uuid::new_v4();
    let n = sqlx::query(
        "INSERT INTO work.links(id,subject_kind,subject_id,entity_kind,entity_id,created_by) VALUES($1,'task',$2,$3,$4,$5)
         ON CONFLICT (subject_kind, subject_id, entity_kind, entity_id) WHERE archived_at IS NULL DO NOTHING",
    )
    .bind(id)
    .bind(task)
    .bind(&r.kind)
    .bind(&r.id)
    .bind(st.actor.user)
    .execute(&mut *c)
    .await?;
    if n.rows_affected() == 0 {
        return Ok(None);
    }
    event(
        c,
        task,
        Some(st.actor.user),
        "LINKED",
        Some(&format!("{}:{}", r.kind, r.id)),
    )
    .await?;
    audit(c, st, Some(task), "work.link_added", &r.kind).await?;
    Ok(Some(id))
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    /// mine · review · created · all
    pub view: Option<String>,
    /// open · done · all · archived
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub work_type: Option<String>,
    pub priority: Option<String>,
    pub assignee: Option<Uuid>,
    /// overdue · today · week
    pub due: Option<String>,
    pub entity_kind: Option<String>,
    pub entity_id: Option<String>,
    pub q: Option<String>,
    /// include subtasks in the list (default: top-level only)
    pub subtasks: Option<bool>,
}

pub async fn list(s: &AppState, h: &HeaderMap, q: ListQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let view = q.view.as_deref().unwrap_or("mine");
    one_of(view, &["mine", "review", "created", "all"], "VIEW_UNKNOWN")?;
    let status = q.status.as_deref().unwrap_or("open");
    one_of(
        status,
        &["open", "done", "all", "archived"],
        "STATUS_UNKNOWN",
    )?;
    let work_type = q.work_type.as_deref().filter(|v| !v.is_empty());
    if let Some(t) = work_type {
        one_of(t, WORK_TYPES, "WORK_TYPE_UNKNOWN")?;
    }
    let priority = q.priority.as_deref().filter(|v| !v.is_empty());
    if let Some(p) = priority {
        one_of(p, PRIORITIES, "PRIORITY_UNKNOWN")?;
    }
    let due = q.due.as_deref().filter(|v| !v.is_empty());
    if let Some(d) = due {
        one_of(d, &["overdue", "today", "week"], "DUE_UNKNOWN")?;
    }
    let entity = match (q.entity_kind.as_deref(), q.entity_id.as_deref()) {
        (Some(k), Some(i)) if !k.is_empty() && !i.is_empty() => Some(entities::parse(k, i)?),
        _ => None,
    };
    let term =
        q.q.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| line(t, 80))
            .transpose()?;
    let mut c = s.pool.acquire().await?;
    let rows = sqlx::query(concat!(
        "SELECT ", task_cols!(), " FROM work.tasks t
          WHERE ($1='all' OR ($1='mine' AND t.assignee_id=$2) OR ($1='review' AND t.reviewer_id=$2) OR ($1='created' AND t.created_by=$2))
            AND (CASE $3 WHEN 'archived' THEN t.archived_at IS NOT NULL
                         WHEN 'open' THEN t.archived_at IS NULL AND t.status NOT IN ('DONE','CANCELLED')
                         WHEN 'done' THEN t.archived_at IS NULL AND t.status IN ('DONE','CANCELLED')
                         ELSE t.archived_at IS NULL END)
            AND ($4::text IS NULL OR t.work_type=$4)
            AND ($5::text IS NULL OR t.priority=$5)
            AND ($6::uuid IS NULL OR t.assignee_id=$6)
            AND ($7::text IS NULL
                 OR ($7='overdue' AND LEAST(COALESCE(t.due_at,'infinity'),COALESCE(t.sla_due_at,'infinity')) < now() AND t.status NOT IN ('DONE','CANCELLED'))
                 OR ($7='today' AND LEAST(COALESCE(t.due_at,'infinity'),COALESCE(t.sla_due_at,'infinity')) < date_trunc('day', now() AT TIME ZONE 'Asia/Seoul') AT TIME ZONE 'Asia/Seoul' + interval '1 day')
                 OR ($7='week' AND LEAST(COALESCE(t.due_at,'infinity'),COALESCE(t.sla_due_at,'infinity')) < now() + interval '7 days'))
            AND ($8::text IS NULL OR EXISTS(SELECT 1 FROM work.links l WHERE l.subject_kind='task' AND l.subject_id=t.id
                   AND l.archived_at IS NULL AND l.entity_kind=$8 AND l.entity_id=$9))
            AND ($10::text IS NULL OR t.title ILIKE '%'||$10||'%' OR t.description ILIKE '%'||$10||'%' OR ('WRK-'||t.seq)=upper($10) OR $10=ANY(t.tags))
            AND ($11 OR t.parent_id IS NULL)
          ORDER BY (t.status IN ('DONE','CANCELLED')), array_position($12::text[], t.priority) DESC, LEAST(COALESCE(t.due_at,'infinity'),COALESCE(t.sla_due_at,'infinity')), t.created_at DESC
          LIMIT 300"
    ))
    .bind(view)
    .bind(st.actor.user)
    .bind(status)
    .bind(work_type)
    .bind(priority)
    .bind(q.assignee)
    .bind(due)
    .bind(entity.as_ref().map(|e| e.kind.clone()))
    .bind(entity.as_ref().map(|e| e.id.clone()))
    .bind(term)
    .bind(q.subtasks.unwrap_or(false) || entity.is_some())
    .bind(PRIORITIES)
    .fetch_all(&mut *c)
    .await?;
    let tasks: Vec<Task> = rows.iter().map(Task::from_row).collect();
    let links = links_of(&mut c, &tasks.iter().map(|t| t.id).collect::<Vec<_>>()).await?;
    let items: Vec<Value> = tasks
        .iter()
        .map(|t| t.json(links.get(&t.id).map(Vec::as_slice).unwrap_or(&[])))
        .collect();
    Ok(json!({ "items": items }))
}

// ---------------------------------------------------------------------------
// Detail
// ---------------------------------------------------------------------------

pub async fn detail(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let t = load(&mut c, id, false).await?;
    let subs = sqlx::query(concat!(
        "SELECT ",
        task_cols!(),
        " FROM work.tasks t WHERE t.parent_id=$1 AND t.archived_at IS NULL ORDER BY t.seq"
    ))
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let subs: Vec<Task> = subs.iter().map(Task::from_row).collect();
    let mut ids = vec![t.id];
    ids.extend(subs.iter().map(|x| x.id));
    let links = links_of(&mut c, &ids).await?;
    let parent = match t.parent_id {
        Some(p) => {
            let p = load(&mut c, p, false).await?;
            Some(
                json!({"id": p.id, "key": format!("WRK-{}", p.seq), "title": p.title, "status": p.status}),
            )
        }
        None => None,
    };
    let events = sqlx::query("SELECT at,actor_id,action,detail FROM work.task_events WHERE task_id=$1 ORDER BY at DESC, id LIMIT 300")
        .bind(id)
        .fetch_all(&mut *c)
        .await?;
    let empty: Vec<Value> = Vec::new();
    Ok(json!({
        "task": t.json(links.get(&t.id).unwrap_or(&empty)),
        "parent": parent,
        "subtasks": subs.iter().map(|x| x.json(links.get(&x.id).unwrap_or(&empty))).collect::<Vec<_>>(),
        "events": events.iter().map(|r| json!({
            "at": r.get::<DateTime<Utc>,_>("at"), "actor_id": r.get::<Option<Uuid>,_>("actor_id"),
            "action": r.get::<String,_>("action"), "detail": r.get::<Option<String>,_>("detail"),
        })).collect::<Vec<_>>(),
    }))
}

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recurrence {
    pub freq: String,
    pub interval: i32,
    #[serde(default)]
    pub next_at: Option<DateTime<Utc>>,
}

pub(crate) fn next_occurrence(freq: &str, interval: i32, from: DateTime<Utc>) -> DateTime<Utc> {
    match freq {
        "DAILY" => from + Duration::days(interval as i64),
        "WEEKLY" => from + Duration::weeks(interval as i64),
        _ => from
            .checked_add_months(chrono::Months::new(interval as u32))
            .unwrap_or(from + Duration::days(30 * interval as i64)),
    }
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

fn tags(v: &[String]) -> Result<Vec<String>> {
    if v.len() > 8 {
        return Err(Error::InvalidCode("TOO_MANY_TAGS"));
    }
    let mut out = Vec::new();
    for t in v {
        let t = line(t, 30)?;
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkInput {
    pub entity_kind: String,
    pub entity_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "type")]
    pub work_type: String,
    pub priority: String,
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub assignee_id: Option<Uuid>,
    #[serde(default)]
    pub reviewer_id: Option<Uuid>,
    #[serde(default)]
    pub due_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
    #[serde(default)]
    pub links: Vec<LinkInput>,
    #[serde(default)]
    pub tags: Vec<String>,
}

pub async fn create(s: &AppState, h: &HeaderMap, i: CreateInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let title = line(&i.title, 200)?;
    if title.is_empty() {
        return Err(Error::InvalidCode("TITLE_REQUIRED"));
    }
    let description = text(&i.description, 8000)?;
    one_of(&i.work_type, WORK_TYPES, "WORK_TYPE_UNKNOWN")?;
    one_of(&i.priority, PRIORITIES, "PRIORITY_UNKNOWN")?;
    if i.links.len() > 20 {
        return Err(Error::InvalidCode("TOO_MANY_LINKS"));
    }
    let refs = i
        .links
        .iter()
        .map(|l| entities::parse(&l.entity_kind, &l.entity_id))
        .collect::<Result<Vec<_>>>()?;
    let tg = tags(&i.tags)?;
    let now = Utc::now();
    let mut tx = s.pool.begin().await?;
    active_staff(&mut tx, i.assignee_id).await?;
    active_staff(&mut tx, i.reviewer_id).await?;
    if let Some(p) = i.parent_id {
        let parent = load(&mut tx, p, true)
            .await
            .map_err(|_| Error::InvalidCode("PARENT_NOT_FOUND"))?;
        if parent.parent_id.is_some() || parent.archived_at.is_some() {
            return Err(Error::InvalidCode("PARENT_INVALID"));
        }
        if i.recurrence.is_some() {
            return Err(Error::InvalidCode("SUBTASK_RECURRENCE"));
        }
    }
    let rec = recurrence(i.recurrence.as_ref(), now)?;
    let sla_due = super::sla::task_due(&mut tx, &i.work_type, now).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.tasks(id,parent_id,title,description,work_type,priority,assignee_id,reviewer_id,due_at,recurrence,tags,created_by,created_at,updated_at,assigned_at,sla_due_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,CASE WHEN $7::uuid IS NULL THEN NULL ELSE $13 END,$14)",
    )
    .bind(id).bind(i.parent_id).bind(&title).bind(&description).bind(&i.work_type).bind(&i.priority)
    .bind(i.assignee_id).bind(i.reviewer_id).bind(i.due_at).bind(rec).bind(&tg).bind(st.actor.user).bind(now).bind(sla_due)
    .execute(&mut *tx)
    .await?;
    event(&mut tx, id, Some(st.actor.user), "CREATED", None).await?;
    audit(&mut tx, &st, Some(id), "work.task_created", &i.work_type).await?;
    if let Some(p) = i.parent_id {
        event(
            &mut tx,
            p,
            Some(st.actor.user),
            "SUBTASK_ADDED",
            Some(&title),
        )
        .await?;
    }
    for r in &refs {
        add_link(&mut tx, &st, id, r).await?;
    }
    tx.commit().await?;
    detail(s, h, id).await
}

// ---------------------------------------------------------------------------
// Update (fields, status, review decision)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateInput {
    pub row_version: i32,
    pub title: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "type")]
    pub work_type: Option<String>,
    pub priority: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub assignee_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub reviewer_id: Option<Option<Uuid>>,
    #[serde(default, deserialize_with = "double_option")]
    pub due_at: Option<Option<DateTime<Utc>>>,
    #[serde(default, deserialize_with = "double_option")]
    pub recurrence: Option<Option<Recurrence>>,
    pub tags: Option<Vec<String>>,
    pub status: Option<String>,
    /// APPROVE · REQUEST_CHANGES (task in review)
    pub review: Option<String>,
    pub review_note: Option<String>,
}

/// `null` clears the field, an absent key leaves it as is.
pub(crate) fn double_option<'de, T, D>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

pub async fn update(s: &AppState, h: &HeaderMap, id: Uuid, p: UpdateInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let me = st.actor.user;
    let now = Utc::now();
    let mut tx = s.pool.begin().await?;
    let mut t = load(&mut tx, id, true).await?;
    if t.archived_at.is_some() {
        return Err(Error::PolicyGate("TASK_ARCHIVED"));
    }
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
            events.push(("EDITED", Some("title".into())));
        }
    }
    if let Some(v) = &p.description {
        let v = text(v, 8000)?;
        if v != t.description {
            t.description = v;
            events.push(("EDITED", Some("description".into())));
        }
    }
    if let Some(v) = &p.work_type {
        one_of(v, WORK_TYPES, "WORK_TYPE_UNKNOWN")?;
        if *v != t.work_type {
            t.work_type = v.clone();
            t.sla_due_at = super::sla::task_due(&mut tx, v, t.created_at).await?;
            t.escalation_level = 0;
            events.push(("TYPE", Some(v.clone())));
        }
    }
    if let Some(v) = &p.priority {
        one_of(v, PRIORITIES, "PRIORITY_UNKNOWN")?;
        if *v != t.priority {
            t.priority = v.clone();
            events.push(("PRIORITY", Some(v.clone())));
        }
    }
    if let Some(v) = p.assignee_id {
        active_staff(&mut tx, v).await?;
        if v != t.assignee_id {
            t.assignee_id = v;
            t.assigned_at = v.map(|_| now);
            events.push(("ASSIGNED", v.map(|u| u.to_string())));
        }
    }
    if let Some(v) = p.reviewer_id {
        active_staff(&mut tx, v).await?;
        if v != t.reviewer_id {
            t.reviewer_id = v;
            events.push(("REVIEWER", v.map(|u| u.to_string())));
        }
    }
    if let Some(v) = p.due_at
        && v != t.due_at
    {
        t.due_at = v;
        t.escalation_level = 0;
        events.push(("DUE", v.map(|d| d.to_rfc3339())));
    }
    if let Some(v) = &p.recurrence {
        if v.is_some() && t.parent_id.is_some() {
            return Err(Error::InvalidCode("SUBTASK_RECURRENCE"));
        }
        let r = recurrence(v.as_ref(), now)?;
        if r.as_ref().map(|x| (&x["freq"], &x["interval"]))
            != t.recurrence.as_ref().map(|x| (&x["freq"], &x["interval"]))
        {
            t.recurrence = r;
            events.push(("RECURRENCE", None));
        }
    }
    if let Some(v) = &p.tags {
        t.tags = tags(v)?;
    }

    let mut done_now = false;
    if let Some(r) = p.review.as_deref() {
        if t.status != "IN_REVIEW" {
            return Err(Error::PolicyGate("TASK_NOT_IN_REVIEW"));
        }
        permit(
            &st,
            Act::DecideReview {
                reviewer: t.reviewer_id,
            },
        )?;
        let note = p
            .review_note
            .as_deref()
            .map(|n| text(n, 2000))
            .transpose()?
            .filter(|n| !n.is_empty());
        match r {
            "APPROVE" => {
                if t.subtasks_total > t.subtasks_done {
                    return Err(Error::PolicyGate("TASK_SUBTASKS_OPEN"));
                }
                t.status = "DONE".into();
                t.completed_at = Some(now);
                t.changes_requested = false;
                done_now = true;
                events.push(("APPROVED", note));
            }
            "REQUEST_CHANGES" => {
                let note = note.ok_or(Error::InvalidCode("REVIEW_NOTE_REQUIRED"))?;
                t.status = "IN_PROGRESS".into();
                t.changes_requested = true;
                events.push(("CHANGES_REQUESTED", Some(note)));
            }
            _ => return Err(Error::InvalidCode("REVIEW_UNKNOWN")),
        }
    } else if let Some(next) = p.status.as_deref() {
        one_of(next, STATUSES, "STATUS_UNKNOWN")?;
        if next != t.status {
            let mut next = next.to_string();
            // With a reviewer other than me, "done" asks for review first.
            if next == "DONE" && t.status != "IN_REVIEW" && t.reviewer_id.is_some_and(|r| r != me) {
                next = "IN_REVIEW".into();
            }
            if next == "IN_REVIEW" && t.reviewer_id.is_none() {
                return Err(Error::PolicyGate("TASK_NO_REVIEWER"));
            }
            if t.status == "IN_REVIEW" && next == "DONE" {
                // finishing a task in review is the reviewer's decision
                permit(
                    &st,
                    Act::DecideReview {
                        reviewer: t.reviewer_id,
                    },
                )?;
            }
            if next == "DONE" && t.subtasks_total > t.subtasks_done {
                return Err(Error::PolicyGate("TASK_SUBTASKS_OPEN"));
            }
            match next.as_str() {
                "IN_PROGRESS" => {
                    t.started_at.get_or_insert(now);
                    t.completed_at = None;
                }
                "TODO" | "BLOCKED" => t.completed_at = None,
                "IN_REVIEW" => {
                    t.changes_requested = false;
                    t.completed_at = None;
                }
                "DONE" => {
                    t.completed_at = Some(now);
                    t.changes_requested = false;
                    done_now = true;
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

    if events.is_empty() && p.tags.is_none() {
        tx.commit().await?;
        return detail(s, h, id).await;
    }
    sqlx::query(
        "UPDATE work.tasks SET title=$2,description=$3,work_type=$4,status=$5,priority=$6,assignee_id=$7,reviewer_id=$8,due_at=$9,
            changes_requested=$10,recurrence=$11,tags=$12,assigned_at=$13,started_at=$14,completed_at=$15,updated_at=$16,
            sla_due_at=$17,escalation_level=$18,row_version=row_version+1
          WHERE id=$1",
    )
    .bind(t.id).bind(&t.title).bind(&t.description).bind(&t.work_type).bind(&t.status).bind(&t.priority)
    .bind(t.assignee_id).bind(t.reviewer_id).bind(t.due_at).bind(t.changes_requested).bind(&t.recurrence).bind(&t.tags)
    .bind(t.assigned_at).bind(t.started_at).bind(t.completed_at).bind(now)
    .bind(t.sla_due_at).bind(t.escalation_level)
    .execute(&mut *tx)
    .await?;
    for (a, d) in &events {
        event(&mut tx, t.id, Some(me), a, d.as_deref()).await?;
    }
    // A recurring task that is done makes the next one: same people, type and links.
    if done_now && let Some(r) = t.recurrence.clone() {
        let freq = r["freq"].as_str().unwrap_or("WEEKLY").to_string();
        let interval = r["interval"].as_i64().unwrap_or(1) as i32;
        let next = Uuid::new_v4();
        let next_sla = super::sla::task_due(&mut tx, &t.work_type, now).await?;
        let rec = json!({"freq": freq, "interval": interval, "next_at": next_occurrence(&freq, interval, now)});
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO work.tasks(id,title,description,work_type,priority,assignee_id,reviewer_id,due_at,recurrence,recurred_from,tags,created_by,created_at,updated_at,assigned_at,sla_due_at)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$13,CASE WHEN $6::uuid IS NULL THEN NULL ELSE $13 END,$14) RETURNING seq",
        )
        .bind(next).bind(&t.title).bind(&t.description).bind(&t.work_type).bind(t.priority_raised.as_ref().unwrap_or(&t.priority)).bind(t.assignee_id).bind(t.reviewer_id)
        .bind(t.due_at.map(|d| next_occurrence(&freq, interval, d))).bind(rec).bind(t.id).bind(&t.tags).bind(me).bind(now).bind(next_sla)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO work.links(id,subject_kind,subject_id,entity_kind,entity_id,created_by)
             SELECT gen_random_uuid(),'task',$1,entity_kind,entity_id,$3 FROM work.links
              WHERE subject_kind='task' AND subject_id=$2 AND archived_at IS NULL",
        )
        .bind(next)
        .bind(t.id)
        .bind(me)
        .execute(&mut *tx)
        .await?;
        event(
            &mut tx,
            next,
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
        audit(&mut tx, &st, Some(next), "work.task_created", "RECURRENCE").await?;
    }
    let reason = events
        .iter()
        .map(|e| e.0)
        .find(|a| *a != "EDITED")
        .or(events.first().map(|e| e.0))
        .unwrap_or("TAGS");
    audit(&mut tx, &st, Some(t.id), "work.task_updated", reason).await?;
    tx.commit().await?;
    detail(s, h, id).await
}

// ---------------------------------------------------------------------------
// Archive, links
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveInput {
    #[serde(default)]
    pub reason: String,
}

/// Archive a task and its subtasks (never delete). Recorded and audited.
pub async fn archive(s: &AppState, h: &HeaderMap, id: Uuid, i: ArchiveInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let reason = line(&i.reason, 200)?;
    let mut tx = s.pool.begin().await?;
    let t = load(&mut tx, id, true).await?;
    permit(
        &st,
        Act::ArchiveTask {
            created_by: t.created_by,
            assignee: t.assignee_id,
        },
    )?;
    if t.archived_at.is_some() {
        return Err(Error::Conflict);
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE work.tasks SET archived_at=now(), archived_by=$2, row_version=row_version+1
          WHERE (id=$1 OR parent_id=$1) AND archived_at IS NULL RETURNING id",
    )
    .bind(id)
    .bind(st.actor.user)
    .fetch_all(&mut *tx)
    .await?;
    for tid in &ids {
        event(
            &mut tx,
            *tid,
            Some(st.actor.user),
            "ARCHIVED",
            (!reason.is_empty()).then_some(reason.as_str()),
        )
        .await?;
        audit(
            &mut tx,
            &st,
            Some(*tid),
            "work.task_archived",
            if *tid == id {
                "ARCHIVE"
            } else {
                "PARENT_ARCHIVED"
            },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(json!({ "archived": ids }))
}

pub async fn link(s: &AppState, h: &HeaderMap, id: Uuid, i: LinkInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let r = entities::parse(&i.entity_kind, &i.entity_id)?;
    let mut tx = s.pool.begin().await?;
    let t = load(&mut tx, id, true).await?;
    if t.archived_at.is_some() {
        return Err(Error::PolicyGate("TASK_ARCHIVED"));
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM work.links WHERE subject_kind='task' AND subject_id=$1 AND archived_at IS NULL")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if n >= 20 {
        return Err(Error::InvalidCode("TOO_MANY_LINKS"));
    }
    add_link(&mut tx, &st, id, &r).await?;
    tx.commit().await?;
    detail(s, h, id).await
}

pub async fn unlink(s: &AppState, h: &HeaderMap, link: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let r = sqlx::query(
        "UPDATE work.links SET archived_at=now(), archived_by=$2 WHERE id=$1 AND archived_at IS NULL
         RETURNING subject_id, entity_kind, entity_id",
    )
    .bind(link)
    .bind(st.actor.user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let task: Uuid = r.get("subject_id");
    let kind: String = r.get("entity_kind");
    event(
        &mut tx,
        task,
        Some(st.actor.user),
        "UNLINKED",
        Some(&format!("{}:{}", kind, r.get::<String, _>("entity_id"))),
    )
    .await?;
    audit(&mut tx, &st, Some(task), "work.link_archived", &kind).await?;
    tx.commit().await?;
    detail(s, h, task).await
}

async fn h_list(
    State(s): State<AppState>,
    Query(q): Query<ListQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(list(&s, &h, q).await?))
}
async fn h_create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CreateInput>,
) -> Result<Json<Value>> {
    Ok(Json(create(&s, &h, i).await?))
}
async fn h_detail(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(detail(&s, &h, id).await?))
}
async fn h_update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<UpdateInput>,
) -> Result<Json<Value>> {
    Ok(Json(update(&s, &h, id, i).await?))
}
async fn h_archive(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ArchiveInput>,
) -> Result<Json<Value>> {
    Ok(Json(archive(&s, &h, id, i).await?))
}
async fn h_link(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<LinkInput>,
) -> Result<Json<Value>> {
    Ok(Json(link(&s, &h, id, i).await?))
}
async fn h_unlink(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(unlink(&s, &h, id).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/tasks", get(h_list).post(h_create))
        .route("/api/staff/work/tasks/{id}", get(h_detail).post(h_update))
        .route("/api/staff/work/tasks/{id}/archive", post(h_archive))
        .route("/api/staff/work/tasks/{id}/links", post(h_link))
        .route("/api/staff/work/links/{id}/archive", post(h_unlink))
}
