//! Unified work inbox: everything waiting on the signed-in staff member,
//! computed from where it already lives (tasks, release review claims, second
//! approvals, document review, customer inquiries, note mentions, the
//! exception queue). Only "read" marks are stored (`work.inbox_reads`); an
//! item's id carries its version, so a changed item is unread again.
use super::{
    exceptions,
    sla::{self, Book, State3},
};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff::{self, Duty, Staff, StaffRole},
};
use axum::{
    Json, Router,
    extract::{Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

pub const CATEGORIES: &[&str] = &[
    "ASSIGNED",
    "REVIEW_REQUEST",
    "APPROVAL_REQUEST",
    "MENTION",
    "FAILED_AUTOMATION",
    "SLA_WARNING",
    "OVERDUE",
    "SUPPORT_ESCALATION",
    "RIGHTS_ISSUE",
    "DISTRIBUTION_FAILURE",
    "EXCEPTION",
    "CONTACT_DUE",
];

#[derive(Debug, Clone)]
struct Item {
    id: String,
    category: &'static str,
    title: String,
    subtitle: String,
    /// task work type, queue type or exception kind
    kind: String,
    priority: &'static str,
    due_at: Option<DateTime<Utc>>,
    sla_state: Option<&'static str>,
    at: DateTime<Utc>,
    link: String,
    entity_kind: Option<String>,
    entity_id: Option<String>,
    flags: Vec<&'static str>,
}

impl Item {
    fn json(&self, read: bool) -> Value {
        json!({
            "id": self.id, "category": self.category, "title": self.title, "subtitle": self.subtitle,
            "type": self.kind, "priority": self.priority, "due_at": self.due_at, "sla_state": self.sla_state,
            "at": self.at, "link": self.link,
            "entity": self.entity_kind.as_ref().map(|k| json!({"kind": k, "id": self.entity_id})),
            "flags": self.flags, "read": read,
        })
    }
}

fn prio(p: &str) -> &'static str {
    match p {
        "URGENT" => "URGENT",
        "HIGH" => "HIGH",
        "LOW" => "LOW",
        _ => "NORMAL",
    }
}

fn prio_of_state(s: State3) -> &'static str {
    match s {
        State3::Breached => "URGENT",
        State3::Warning => "HIGH",
        State3::Ok => "NORMAL",
    }
}

fn duty_of_queue(q: &str) -> Duty {
    match q {
        "DOCUMENT_REVIEW" => Duty::Documents,
        "SUPPORT_INQUIRY" => Duty::Inquiries,
        _ => Duty::Review,
    }
}

/// Everything waiting on `st`, newest problems first.
async fn build(s: &AppState, st: &Staff) -> Result<Vec<Item>> {
    sla::sweep(&s.pool).await?;
    let me = st.actor.user;
    let role = st.role.as_str();
    let now = Utc::now();
    let mut tx = s.pool.begin().await?;
    let book = Book::load(&mut tx).await?;
    let mut out: Vec<Item> = Vec::new();

    // Tasks: mine, waiting on my review, or escalated to my role.
    let rows = sqlx::query(
        "SELECT t.id, t.seq, t.title, t.work_type, t.status, t.priority, t.assignee_id, t.reviewer_id, t.due_at, t.sla_due_at,
                t.created_at, t.updated_at, t.escalation_level, t.row_version,
                (SELECT max(e.at) FROM work.task_events e WHERE e.task_id=t.id) AS last_activity
           FROM work.tasks t
          WHERE t.archived_at IS NULL AND t.status NOT IN ('DONE','CANCELLED')
            AND (t.assignee_id=$1 OR (t.reviewer_id=$1 AND t.status='IN_REVIEW') OR t.escalation_level>0)
          ORDER BY t.updated_at DESC LIMIT 500",
    )
    .bind(me)
    .fetch_all(&mut *tx)
    .await?;
    for r in rows {
        let id: Uuid = r.get("id");
        let work_type: String = r.get("work_type");
        let status: String = r.get("status");
        let assignee: Option<Uuid> = r.get("assignee_id");
        let reviewer: Option<Uuid> = r.get("reviewer_id");
        let level: i16 = r.get("escalation_level");
        let created: DateTime<Utc> = r.get("created_at");
        let policy = book.policy(&work_type, None);
        let warn = policy.map(|p| p.warn_ratio).unwrap_or(0.25);
        let (deadline, state) =
            sla::task_deadline(r.get("due_at"), r.get("sla_due_at"), created, false, warn);
        let mine = assignee == Some(me);
        let review = reviewer == Some(me) && status == "IN_REVIEW";
        let escalated_to_me = level > 0
            && !mine
            && (st.role == StaffRole::Admin || policy.is_some_and(|p| p.escalate_to == role));
        if !mine && !review && !escalated_to_me {
            continue;
        }
        let mut flags = Vec::new();
        if level > 0 {
            flags.push("ESCALATED");
        }
        let last: DateTime<Utc> = r
            .get::<Option<DateTime<Utc>>, _>("last_activity")
            .unwrap_or(created);
        let idle = mine
            && status != "BLOCKED"
            && policy.is_some_and(|p| now - last > Duration::minutes(p.idle_alert_minutes as i64));
        if idle {
            flags.push("IDLE");
        }
        let category = if review {
            "REVIEW_REQUEST"
        } else if state == Some("BREACHED") || escalated_to_me {
            "OVERDUE"
        } else if state == Some("WARNING") || idle {
            "SLA_WARNING"
        } else {
            "ASSIGNED"
        };
        out.push(Item {
            id: format!("task:{id}@{}", r.get::<i32, _>("row_version")),
            category,
            title: r.get("title"),
            subtitle: format!("WRK-{} · {}", r.get::<i64, _>("seq"), status),
            kind: work_type,
            priority: prio(&r.get::<String, _>("priority")),
            due_at: deadline,
            sla_state: state,
            at: r.get("updated_at"),
            link: format!("/admin/tasks/{id}"),
            entity_kind: Some("task".into()),
            entity_id: Some(id.to_string()),
            flags,
        });
    }

    // Existing queues (release review, second approval, documents, inquiries).
    for q in sla::queue_items(&mut tx, &book).await? {
        if !st.role.may(duty_of_queue(q.queue)) {
            continue;
        }
        let (category, why) = match q.queue {
            "RELEASE_REVIEW" if q.owner == Some(me) => ("REVIEW_REQUEST", None),
            "SECOND_APPROVAL" if q.requester != Some(me) => ("APPROVAL_REQUEST", None),
            "SUPPORT_INQUIRY" if q.state == State3::Breached => ("SUPPORT_ESCALATION", None),
            _ if q.state == State3::Breached && q.owner.is_none_or(|o| o == me) => {
                ("OVERDUE", Some("SLA"))
            }
            _ if q.state == State3::Warning && q.owner.is_none_or(|o| o == me) => {
                ("SLA_WARNING", Some("SLA"))
            }
            _ => continue,
        };
        let (entity_kind, entity_id) = match q.queue {
            "RELEASE_REVIEW" => (Some("release"), Some(q.id.to_string())),
            "DOCUMENT_REVIEW" => (Some("document"), Some(q.id.to_string())),
            "SUPPORT_INQUIRY" => (Some("inquiry"), Some(q.id.to_string())),
            _ => (None, None),
        };
        out.push(Item {
            id: format!(
                "{}:{}@{}",
                q.queue.to_lowercase(),
                q.id,
                q.start.timestamp()
            ),
            category,
            title: q.title.clone(),
            subtitle: q.org_name.clone().unwrap_or_default(),
            kind: q.queue.into(),
            priority: prio_of_state(q.state),
            due_at: Some(q.due),
            sla_state: Some(q.state.as_str()),
            at: q.start,
            link: q.link.clone(),
            entity_kind: entity_kind.map(String::from),
            entity_id,
            flags: why.into_iter().collect(),
        });
    }

    // Mentions in unresolved notes.
    let notes = sqlx::query(
        "SELECT n.id, n.target_kind, n.target_id, left(n.body, 160) AS body, n.created_at, n.edited_at, u.email
           FROM work.notes n JOIN identity.users u ON u.id=n.author_id
          WHERE $1 = ANY(n.mentions) AND n.author_id<>$1 AND n.archived_at IS NULL AND n.resolved_at IS NULL
            AND (n.target_kind<>'doc' OR work.doc_level(n.target_id::uuid,$1,$2) > 0)
          ORDER BY n.created_at DESC LIMIT 200",
    )
    .bind(me)
    .bind(st.role.as_str())
    .fetch_all(&mut *tx)
    .await?;
    for n in notes {
        let id: Uuid = n.get("id");
        let kind: String = n.get("target_kind");
        let target: String = n.get("target_id");
        let at: DateTime<Utc> = n.get("created_at");
        let edited: Option<DateTime<Utc>> = n.get("edited_at");
        let email: String = n.get("email");
        let link = match kind.as_str() {
            "task" => format!("/admin/tasks/{target}"),
            "release" => format!("/admin/reviews/{target}"),
            "inquiry" => format!("/admin/inquiries/{target}"),
            "document" => "/admin/documents".into(),
            "channel" => format!("/admin/chat/{target}"),
            "doc" => format!("/admin/docs/{target}"),
            _ => "/admin".into(),
        };
        out.push(Item {
            id: format!("note:{id}@{}", edited.unwrap_or(at).timestamp()),
            category: "MENTION",
            title: n.get("body"),
            subtitle: email.split('@').next().unwrap_or("").to_string(),
            kind: "NOTE".into(),
            priority: "NORMAL",
            due_at: None,
            sla_state: None,
            at,
            link,
            entity_kind: Some(kind),
            entity_id: Some(target),
            flags: vec![],
        });
    }

    // CRM: relationships I own whose next contact date has come.
    let due = sqlx::query(concat!(
        "SELECT r.id, r.kind, r.next_contact_at, r.row_version, ",
        super::crm::source_name!(),
        " AS name FROM work.relationships r
          WHERE r.owner_id=$1 AND r.archived_at IS NULL AND r.next_contact_at <= now()
          ORDER BY r.next_contact_at LIMIT 100"
    ))
    .bind(me)
    .fetch_all(&mut *tx)
    .await?;
    for r in due {
        let id: Uuid = r.get("id");
        let at: DateTime<Utc> = r.get("next_contact_at");
        out.push(Item {
            id: format!("relationship:{id}@{}", r.get::<i32, _>("row_version")),
            category: "CONTACT_DUE",
            title: r.get::<Option<String>, _>("name").unwrap_or_default(),
            subtitle: r.get("kind"),
            kind: "RELATIONSHIP".into(),
            priority: if now - at > Duration::days(3) {
                "HIGH"
            } else {
                "NORMAL"
            },
            due_at: Some(at),
            sla_state: None,
            at,
            link: format!("/admin/crm/{id}"),
            entity_kind: Some("relationship".into()),
            entity_id: Some(id.to_string()),
            flags: vec![],
        });
    }

    // Exceptions: assigned to me, or unassigned and urgent within my duty.
    let excs = exceptions::collect(&mut tx, None, None).await?;
    tx.commit().await?;
    for e in excs.iter().filter(|e| !e.closed()) {
        let sla = exceptions::sla_of(&book, e, now);
        let mine = e.assignee() == Some(me);
        let urgent = matches!(e.severity.as_str(), "CRITICAL" | "HIGH")
            || sla.is_some_and(|(_, s)| s != "OK");
        if !(mine || (e.assignee().is_none() && urgent && exceptions::may(st, &e.kind))) {
            continue;
        }
        let category = match e.kind.as_str() {
            "AUTOMATION_FAILURE" => "FAILED_AUTOMATION",
            "MISSING_RIGHTS_DOCUMENT" => "RIGHTS_ISSUE",
            "DSP_REJECTION" | "DISTRIBUTION_FAILURE" | "METADATA_CONFLICT" => {
                "DISTRIBUTION_FAILURE"
            }
            _ => "EXCEPTION",
        };
        let mut flags = Vec::new();
        if mine {
            flags.push("ASSIGNED");
        }
        out.push(Item {
            id: format!("exception:{}@{}", e.id(), e.detected_at.timestamp()),
            category,
            title: e.title.clone(),
            subtitle: [e.org_name.clone(), e.dsp.clone(), Some(e.detail.clone())]
                .into_iter()
                .flatten()
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(" · ")
                .chars()
                .take(200)
                .collect(),
            kind: e.kind.clone(),
            priority: match e.severity.as_str() {
                "CRITICAL" => "URGENT",
                "HIGH" => "HIGH",
                "LOW" => "LOW",
                _ => "NORMAL",
            },
            due_at: sla.map(|(d, _)| d),
            sla_state: sla.map(|(_, s)| s),
            at: e.detected_at,
            link: format!("/admin/exceptions?focus={}", urlencoding_lite(&e.id())),
            entity_kind: e.entity_kind.clone(),
            entity_id: e.entity_id.clone(),
            flags,
        });
    }

    let rank = |p: &str| match p {
        "URGENT" => 0,
        "HIGH" => 1,
        "NORMAL" => 2,
        _ => 3,
    };
    out.sort_by(|a, b| {
        rank(a.priority)
            .cmp(&rank(b.priority))
            .then(
                a.due_at
                    .unwrap_or(DateTime::<Utc>::MAX_UTC)
                    .cmp(&b.due_at.unwrap_or(DateTime::<Utc>::MAX_UTC)),
            )
            .then(b.at.cmp(&a.at))
    });
    Ok(out)
}

/// Item ids are `source:id@version`; only `:` needs escaping in a query value.
fn urlencoding_lite(s: &str) -> String {
    s.replace(':', "%3A")
}

async fn reads(s: &AppState, me: Uuid, ids: &[String]) -> Result<HashSet<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT item_id FROM work.inbox_reads WHERE user_id=$1 AND item_id = ANY($2)",
    )
    .bind(me)
    .bind(ids)
    .fetch_all(&s.pool)
    .await?;
    Ok(rows.into_iter().collect())
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct InboxQuery {
    pub category: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub priority: Option<String>,
    pub unread: Option<bool>,
    /// overdue · today · week
    pub due: Option<String>,
    pub entity_kind: Option<String>,
}

pub async fn inbox(s: &AppState, h: &HeaderMap, q: InboxQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let category = q.category.as_deref().filter(|v| !v.is_empty());
    if let Some(c) = category {
        super::one_of(c, CATEGORIES, "CATEGORY_UNKNOWN")?;
    }
    let priority = q.priority.as_deref().filter(|v| !v.is_empty());
    if let Some(p) = priority {
        super::one_of(p, super::tasks::PRIORITIES, "PRIORITY_UNKNOWN")?;
    }
    let due = q.due.as_deref().filter(|v| !v.is_empty());
    if let Some(d) = due {
        super::one_of(d, &["overdue", "today", "week"], "DUE_UNKNOWN")?;
    }
    let kind = q.kind.as_deref().filter(|v| !v.is_empty());
    let entity_kind = q.entity_kind.as_deref().filter(|v| !v.is_empty());
    let all = build(s, &st).await?;
    let ids: Vec<String> = all.iter().map(|i| i.id.clone()).collect();
    let read = reads(s, st.actor.user, &ids).await?;
    let now = Utc::now();
    let mut counts: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for i in &all {
        let c = counts.entry(i.category).or_default();
        c.0 += 1;
        if !read.contains(&i.id) {
            c.1 += 1;
        }
    }
    let end_of_today = {
        let kst = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
        let d = now.with_timezone(&kst).date_naive() + Duration::days(1);
        d.and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(kst)
            .single()
            .map(|t| t.with_timezone(&Utc))
            .unwrap_or(now)
    };
    let items: Vec<Value> = all
        .iter()
        .filter(|i| category.is_none_or(|c| i.category == c))
        .filter(|i| kind.is_none_or(|k| i.kind == k))
        .filter(|i| priority.is_none_or(|p| i.priority == p))
        .filter(|i| !q.unread.unwrap_or(false) || !read.contains(&i.id))
        .filter(|i| entity_kind.is_none_or(|k| i.entity_kind.as_deref() == Some(k)))
        .filter(|i| match due {
            Some("overdue") => i.due_at.is_some_and(|d| d < now),
            Some("today") => i.due_at.is_some_and(|d| d < end_of_today),
            Some("week") => i.due_at.is_some_and(|d| d < now + Duration::days(7)),
            _ => true,
        })
        .take(300)
        .map(|i| i.json(read.contains(&i.id)))
        .collect();
    Ok(json!({
        "items": items,
        "counts": counts.iter().map(|(k, (t, u))| (k.to_string(), json!({"total": t, "unread": u}))).collect::<serde_json::Map<_,_>>(),
        "total": all.len(),
        "unread": all.iter().filter(|i| !read.contains(&i.id)).count(),
    }))
}

/// Badges for the admin navigation.
pub async fn summary(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    // automatic Kanban cards from AUDENIQ events (throttled, idempotent)
    if let Err(e) = super::automation::sweep(&s.pool, false).await {
        tracing::warn!(error = %e, "work automation sweep failed");
    }
    let all = build(s, &st).await?;
    let ids: Vec<String> = all.iter().map(|i| i.id.clone()).collect();
    let read = reads(s, st.actor.user, &ids).await?;
    let mut tx = s.pool.begin().await?;
    let excs = exceptions::collect(&mut tx, None, None).await?;
    let chat = super::channels::unread_total(&mut tx, st.actor.user).await?;
    tx.commit().await?;
    let open: Vec<&exceptions::Item> = excs.iter().filter(|e| !e.closed()).collect();
    Ok(json!({
        "chat": {"unread": chat},
        "inbox": {"total": all.len(), "unread": all.iter().filter(|i| !read.contains(&i.id)).count()},
        "overdue": all.iter().filter(|i| i.category == "OVERDUE").count(),
        "sla_warning": all.iter().filter(|i| i.category == "SLA_WARNING").count(),
        "exceptions": {
            "open": open.len(),
            "mine": open.iter().filter(|e| e.assignee() == Some(st.actor.user)).count(),
            "critical": open.iter().filter(|e| e.severity == "CRITICAL").count(),
        },
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    pub ids: Vec<String>,
}

pub async fn mark_read(s: &AppState, h: &HeaderMap, i: ReadInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if i.ids.len() > 500 {
        return Err(Error::InvalidCode("TOO_MANY_ITEMS"));
    }
    if i.ids.iter().any(|v| v.is_empty() || v.len() > 160) {
        return Err(Error::InvalidCode("ITEM_ID_INVALID"));
    }
    sqlx::query(
        "INSERT INTO work.inbox_reads(user_id,item_id) SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING",
    )
    .bind(st.actor.user)
    .bind(&i.ids)
    .execute(&s.pool)
    .await?;
    Ok(json!({"read": i.ids.len()}))
}

async fn h_inbox(
    State(s): State<AppState>,
    Query(q): Query<InboxQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(inbox(&s, &h, q).await?))
}
async fn h_summary(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(summary(&s, &h).await?))
}
async fn h_read(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ReadInput>,
) -> Result<Json<Value>> {
    Ok(Json(mark_read(&s, &h, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/inbox", get(h_inbox))
        .route("/api/staff/work/inbox/read", post(h_read))
        .route("/api/staff/work/summary", get(h_summary))
}
