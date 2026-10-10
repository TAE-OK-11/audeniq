//! Kanban boards. A card is a task (`work.tasks`); a board adds columns
//! (each mapped to a task status, so inbox, SLA and review rules keep working)
//! and the card's place (column + position). Status rules stay in
//! `tasks::apply`, which the move endpoint calls inside the same transaction.
//!
//! Access (always checked here, never only in the UI):
//! - PERSONAL: its owner; a card on it is also visible to its creator,
//!   assignee and reviewer.
//! - TEAM: every staff member reads; its role and ADMIN edit.
//! - OPERATIONS: every staff member reads and edits.
//!
//! Managing a board (columns, rename, archive, automation) is ADMIN or the
//! board's creator / owner. Every change is audited (`work.board_*`).
use super::{audit, entities, line, one_of, tasks};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff::{self, Staff, StaffRole},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

const KINDS: &[&str] = &["PERSONAL", "TEAM", "OPERATIONS"];
const ROLES: &[&str] = &["ADMIN", "REVIEWER", "OPERATOR", "SUPPORT"];
const COLUMN_STATUSES: &[&str] = &[
    "TODO",
    "IN_PROGRESS",
    "IN_REVIEW",
    "BLOCKED",
    "DONE",
    "CANCELLED",
];
pub const TRIGGERS: &[&str] = &[
    "QC_FAILURE",
    "METADATA_CONFLICT",
    "MISSING_RIGHTS_DOCUMENT",
    "DSP_REJECTION",
    "DISTRIBUTION_FAILURE",
    "SETTLEMENT_MISMATCH",
    "AUTOMATION_FAILURE",
    "SUPPORT_INQUIRY",
    "APPROVAL_REQUIRED",
    "RELEASE_REVIEW_OVERDUE",
];
const DEFAULT_COLUMNS: &[(&str, &str)] = &[
    ("Inbox", "TODO"),
    ("To Do", "TODO"),
    ("In Progress", "IN_PROGRESS"),
    ("Review", "IN_REVIEW"),
    ("Waiting", "BLOCKED"),
    ("Blocked", "BLOCKED"),
    ("Done", "DONE"),
];
const GAP: f64 = 1024.0;

// ---------------------------------------------------------------------------
// Access
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Board {
    pub id: Uuid,
    pub key: Option<String>,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub owner_id: Option<Uuid>,
    pub team_role: Option<String>,
    pub created_by: Option<Uuid>,
    pub row_version: i32,
    pub archived: bool,
}

impl Board {
    pub fn can_read(&self, st: &Staff) -> bool {
        self.kind != "PERSONAL" || self.owner_id == Some(st.actor.user)
    }
    pub fn can_write(&self, st: &Staff) -> bool {
        !self.archived
            && match self.kind.as_str() {
                "PERSONAL" => self.owner_id == Some(st.actor.user),
                "TEAM" => {
                    st.role == StaffRole::Admin
                        || self.team_role.as_deref() == Some(st.role.as_str())
                }
                _ => true,
            }
    }
    pub fn can_manage(&self, st: &Staff) -> bool {
        match self.kind.as_str() {
            "PERSONAL" => self.owner_id == Some(st.actor.user),
            _ => {
                st.role == StaffRole::Admin
                    || (self.created_by.is_some() && self.created_by == Some(st.actor.user))
            }
        }
    }
    fn json(&self, st: &Staff) -> Value {
        json!({
            "id": self.id, "key": self.key, "name": self.name, "description": self.description, "kind": self.kind,
            "owner_id": self.owner_id, "team_role": self.team_role, "created_by": self.created_by,
            "row_version": self.row_version, "archived": self.archived,
            "access": {"write": self.can_write(st), "manage": self.can_manage(st)},
        })
    }
}

pub(crate) async fn load_board(c: &mut PgConnection, id: Uuid) -> Result<Board> {
    let r = sqlx::query(
        "SELECT id,key,name,description,kind,owner_id,team_role,created_by,row_version,archived_at IS NOT NULL AS archived
           FROM work.boards WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    Ok(Board {
        id: r.get("id"),
        key: r.get("key"),
        name: r.get("name"),
        description: r.get("description"),
        kind: r.get("kind"),
        owner_id: r.get("owner_id"),
        team_role: r.get("team_role"),
        created_by: r.get("created_by"),
        row_version: r.get("row_version"),
        archived: r.get("archived"),
    })
}

/// A board the caller may read (someone else's personal board is a 404).
async fn readable(c: &mut PgConnection, st: &Staff, id: Uuid) -> Result<Board> {
    let b = load_board(c, id).await?;
    if !b.can_read(st) {
        return Err(Error::NotFound);
    }
    Ok(b)
}

pub(crate) async fn require_write(c: &mut PgConnection, st: &Staff, id: Uuid) -> Result<Board> {
    let b = readable(c, st, id).await?;
    if b.archived {
        return Err(Error::PolicyGate("BOARD_ARCHIVED"));
    }
    if !b.can_write(st) {
        return Err(Error::Forbidden);
    }
    Ok(b)
}

async fn require_manage(c: &mut PgConnection, st: &Staff, id: Uuid) -> Result<Board> {
    let b = readable(c, st, id).await?;
    if !b.can_manage(st) {
        return Err(Error::Forbidden);
    }
    Ok(b)
}

/// Can the caller see this card? (Personal-board cards: owner, creator, assignee, reviewer.)
pub(crate) async fn card_visible(
    c: &mut PgConnection,
    st: &Staff,
    t: &tasks::Task,
) -> Result<bool> {
    let Some(b) = t.board_id else { return Ok(true) };
    let b = load_board(c, b).await?;
    let me = Some(st.actor.user);
    Ok(b.can_read(st) || t.created_by == me || t.assignee_id == me || t.reviewer_id == me)
}

/// Can the caller change this card? Personal board: its owner and the people on the card.
pub(crate) async fn card_writable(c: &mut PgConnection, st: &Staff, t: &tasks::Task) -> Result<()> {
    let Some(b) = t.board_id else { return Ok(()) };
    let b = load_board(c, b).await?;
    let me = Some(st.actor.user);
    let ok = if b.kind == "PERSONAL" {
        b.owner_id == me || t.created_by == me || t.assignee_id == me || t.reviewer_id == me
    } else {
        b.can_write(st) || t.assignee_id == me || t.reviewer_id == me
    };
    if ok { Ok(()) } else { Err(Error::Forbidden) }
}

/// SQL predicate for task lists: hides other people's personal-board cards ($me = caller).
#[macro_export]
macro_rules! work_card_visible {
    ($me:literal) => {
        concat!(
            "(t.board_id IS NULL OR NOT EXISTS(SELECT 1 FROM work.boards vb WHERE vb.id=t.board_id AND vb.kind='PERSONAL'
               AND vb.owner_id<>", $me, " AND t.created_by IS DISTINCT FROM ", $me, " AND t.assignee_id IS DISTINCT FROM ", $me,
            " AND t.reviewer_id IS DISTINCT FROM ", $me, "))"
        )
    };
}

// ---------------------------------------------------------------------------
// Placement
// ---------------------------------------------------------------------------

pub(crate) struct Placement {
    pub board: Uuid,
    pub column: Uuid,
    pub status: String,
    pub position: f64,
}

async fn end_of(c: &mut PgConnection, column: Uuid) -> Result<f64> {
    let max: Option<f64> = sqlx::query_scalar(
        "SELECT max(position) FROM work.tasks WHERE column_id=$1 AND archived_at IS NULL",
    )
    .bind(column)
    .fetch_one(&mut *c)
    .await?;
    Ok(max.unwrap_or(0.0) + GAP)
}

/// First open column of `status` on a board, with a position at its end.
pub(crate) async fn column_for_status(
    c: &mut PgConnection,
    board: Uuid,
    status: &str,
) -> Result<Option<(Uuid, f64)>> {
    let col: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM work.board_columns WHERE board_id=$1 AND status=$2 AND archived_at IS NULL ORDER BY position LIMIT 1",
    )
    .bind(board)
    .bind(status)
    .fetch_optional(&mut *c)
    .await?;
    match col {
        Some(col) => Ok(Some((col, end_of(c, col).await?))),
        None => Ok(None),
    }
}

/// Where a new card goes: the given column, the board's first column, or the
/// general board's first to-do column.
pub(crate) async fn place(
    c: &mut PgConnection,
    st: &Staff,
    board: Option<Uuid>,
    column: Option<Uuid>,
) -> Result<Placement> {
    let board = match (board, column) {
        (Some(b), _) => b,
        (None, Some(col)) => {
            sqlx::query_scalar("SELECT board_id FROM work.board_columns WHERE id=$1")
                .bind(col)
                .fetch_optional(&mut *c)
                .await?
                .ok_or(Error::InvalidCode("COLUMN_NOT_FOUND"))?
        }
        (None, None) => sqlx::query_scalar(
            "SELECT id FROM work.boards WHERE key='general' AND archived_at IS NULL",
        )
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::InvalidCode("BOARD_NOT_FOUND"))?,
    };
    require_write(c, st, board).await?;
    let row = match column {
        Some(col) => sqlx::query(
            "SELECT id, status FROM work.board_columns WHERE id=$1 AND board_id=$2 AND archived_at IS NULL",
        )
        .bind(col)
        .bind(board),
        None => sqlx::query(
            "SELECT id, status FROM work.board_columns WHERE board_id=$1 AND archived_at IS NULL
              ORDER BY (status <> 'TODO'), position LIMIT 1",
        )
        .bind(board),
    }
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::InvalidCode("COLUMN_NOT_FOUND"))?;
    let column: Uuid = row.get("id");
    Ok(Placement {
        board,
        column,
        status: row.get("status"),
        position: end_of(c, column).await?,
    })
}

/// Position between two neighbours; renumbers the column when the gap is used up.
async fn between(
    c: &mut PgConnection,
    column: Uuid,
    moving: Uuid,
    before: Option<Uuid>,
    after: Option<Uuid>,
) -> Result<f64> {
    async fn pos(c: &mut PgConnection, column: Uuid, id: Option<Uuid>) -> Result<Option<f64>> {
        let Some(id) = id else { return Ok(None) };
        let p: Option<f64> = sqlx::query_scalar(
            "SELECT position FROM work.tasks WHERE id=$1 AND column_id=$2 AND archived_at IS NULL",
        )
        .bind(id)
        .bind(column)
        .fetch_optional(&mut *c)
        .await?;
        p.map(Some)
            .ok_or(Error::InvalidCode("NEIGHBOUR_NOT_IN_COLUMN"))
    }
    for _ in 0..2 {
        let (a, b) = (pos(c, column, before).await?, pos(c, column, after).await?);
        let p = match (a, b) {
            (Some(a), Some(b)) if b > a && b - a > 1e-6 => (a + b) / 2.0,
            (Some(a), None) => a + GAP,
            (None, Some(b)) => b - GAP,
            (None, None) => end_of(c, column).await?,
            _ => {
                // no room between them: renumber the column in its current order, then retry
                sqlx::query(
                    "UPDATE work.tasks t SET position = o.n * 1024 FROM (
                       SELECT id, row_number() OVER (ORDER BY position, created_at) AS n
                         FROM work.tasks WHERE column_id=$1 AND archived_at IS NULL AND id<>$2) o
                     WHERE t.id=o.id",
                )
                .bind(column)
                .bind(moving)
                .execute(&mut *c)
                .await?;
                continue;
            }
        };
        return Ok(p);
    }
    Err(Error::Conflict)
}

// ---------------------------------------------------------------------------
// Boards
// ---------------------------------------------------------------------------

pub async fn list(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let rows = sqlx::query(
        "SELECT b.id, (SELECT count(*) FROM work.tasks t JOIN work.board_columns bc ON bc.id=t.column_id
                        WHERE t.board_id=b.id AND t.archived_at IS NULL AND bc.status NOT IN ('DONE','CANCELLED')) AS open_cards
           FROM work.boards b
          WHERE b.archived_at IS NULL AND (b.kind <> 'PERSONAL' OR b.owner_id=$1)
          ORDER BY (b.kind='PERSONAL') DESC, b.position, b.created_at",
    )
    .bind(st.actor.user)
    .fetch_all(&s.pool)
    .await?;
    let mut c = s.pool.acquire().await?;
    let mut items = Vec::new();
    for r in rows {
        let b = load_board(&mut c, r.get("id")).await?;
        let mut v = b.json(&st);
        v["open_cards"] = json!(r.get::<i64, _>("open_cards"));
        items.push(v);
    }
    Ok(json!({ "items": items }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// create only
    pub kind: Option<String>,
    #[serde(default)]
    pub team_role: Option<String>,
    /// update only
    pub row_version: Option<i32>,
}

pub async fn create(s: &AppState, h: &HeaderMap, i: BoardInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let name = line(&i.name, 80)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    let description = line(&i.description, 500)?;
    let kind = i.kind.as_deref().unwrap_or("PERSONAL");
    one_of(kind, KINDS, "BOARD_KIND_UNKNOWN")?;
    // personal boards for anyone; shared boards are an ADMIN decision
    if kind != "PERSONAL" && st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    let team_role = match (kind, i.team_role.as_deref()) {
        ("TEAM", Some(r)) => {
            one_of(r, ROLES, "ROLE_UNKNOWN")?;
            Some(r.to_string())
        }
        ("TEAM", None) => return Err(Error::InvalidCode("ROLE_REQUIRED")),
        _ => None,
    };
    let mut tx = s.pool.begin().await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.boards WHERE archived_at IS NULL AND (kind<>'PERSONAL' OR owner_id=$1)",
    )
    .bind(st.actor.user)
    .fetch_one(&mut *tx)
    .await?;
    if n >= 100 {
        return Err(Error::InvalidCode("TOO_MANY_BOARDS"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.boards(id,name,description,kind,owner_id,team_role,created_by,position)
         VALUES($1,$2,$3,$4,$5,$6,$7,(SELECT COALESCE(max(position),0)+1 FROM work.boards))",
    )
    .bind(id)
    .bind(&name)
    .bind(&description)
    .bind(kind)
    .bind((kind == "PERSONAL").then_some(st.actor.user))
    .bind(&team_role)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    for (n, (col, status)) in DEFAULT_COLUMNS.iter().enumerate() {
        sqlx::query(
            "INSERT INTO work.board_columns(board_id,name,status,position) VALUES($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(col)
        .bind(status)
        .bind(n as f64)
        .execute(&mut *tx)
        .await?;
    }
    audit(&mut tx, &st, Some(id), "work.board_created", kind).await?;
    tx.commit().await?;
    view(s, h, id, ViewQuery::default()).await
}

pub async fn update(s: &AppState, h: &HeaderMap, id: Uuid, i: BoardInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let name = line(&i.name, 80)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    let description = line(&i.description, 500)?;
    let mut tx = s.pool.begin().await?;
    let b = require_manage(&mut tx, &st, id).await?;
    if i.row_version.is_some_and(|v| v != b.row_version) {
        return Err(Error::Conflict);
    }
    let team_role = match (b.kind.as_str(), i.team_role.as_deref()) {
        ("TEAM", Some(r)) => {
            one_of(r, ROLES, "ROLE_UNKNOWN")?;
            Some(r.to_string())
        }
        _ => b.team_role.clone(),
    };
    sqlx::query(
        "UPDATE work.boards SET name=$2, description=$3, team_role=$4, updated_at=now(), row_version=row_version+1 WHERE id=$1",
    )
    .bind(id)
    .bind(&name)
    .bind(&description)
    .bind(&team_role)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &st, Some(id), "work.board_updated", "BOARD").await?;
    tx.commit().await?;
    view(s, h, id, ViewQuery::default()).await
}

pub async fn archive(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let b = require_manage(&mut tx, &st, id).await?;
    if b.archived {
        return Err(Error::Conflict);
    }
    if b.key.as_deref() == Some("general") {
        return Err(Error::PolicyGate("BOARD_DEFAULT"));
    }
    sqlx::query("UPDATE work.boards SET archived_at=now(), archived_by=$2, row_version=row_version+1 WHERE id=$1")
        .bind(id)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE work.board_rules SET enabled=false, updated_at=now() WHERE board_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &st, Some(id), "work.board_archived", "BOARD").await?;
    tx.commit().await?;
    Ok(json!({"archived": id}))
}

// ---------------------------------------------------------------------------
// Board view (columns + filtered cards)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ViewQuery {
    /// uuid · "me" · "none"
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub column: Option<Uuid>,
    pub tag: Option<String>,
    #[serde(rename = "type")]
    pub work_type: Option<String>,
    /// overdue · today · week · none
    pub due: Option<String>,
    pub creator: Option<Uuid>,
    /// manual · auto · or one source (qc, distribution, …)
    pub source: Option<String>,
    pub entity_kind: Option<String>,
    pub q: Option<String>,
    /// days of finished cards to show (default 30)
    pub done_days: Option<i32>,
}

macro_rules! entity_name_match {
    () => {
        "EXISTS(SELECT 1 FROM work.links l WHERE l.subject_kind='task' AND l.subject_id=t.id AND l.archived_at IS NULL AND (
            l.entity_id = $9
            OR (l.entity_kind='release' AND EXISTS(SELECT 1 FROM catalog.releases r WHERE r.id::text=l.entity_id AND (r.title ILIKE '%'||$9||'%' OR r.upc=$9)))
            OR (l.entity_kind='artist' AND EXISTS(SELECT 1 FROM catalog.artists a WHERE a.id::text=l.entity_id AND a.name ILIKE '%'||$9||'%'))
            OR (l.entity_kind='track' AND EXISTS(SELECT 1 FROM catalog.tracks tr WHERE tr.id::text=l.entity_id AND (tr.title ILIKE '%'||$9||'%' OR tr.isrc=upper($9))))
            OR (l.entity_kind='dsp' AND EXISTS(SELECT 1 FROM distribution.dsp_registry d WHERE d.code=l.entity_id AND d.name ILIKE '%'||$9||'%'))
            OR (l.entity_kind='inquiry' AND EXISTS(SELECT 1 FROM portal.inquiries i WHERE i.id::text=l.entity_id AND i.subject ILIKE '%'||$9||'%'))
            OR (l.entity_kind='org' AND EXISTS(SELECT 1 FROM identity.orgs o WHERE o.id::text=l.entity_id AND o.name ILIKE '%'||$9||'%'))))"
    };
}

pub async fn view(s: &AppState, h: &HeaderMap, id: Uuid, q: ViewQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let me = st.actor.user;
    let (assignee, unassigned) = match q.assignee.as_deref().filter(|v| !v.is_empty()) {
        None => (None, false),
        Some("me") => (Some(me), false),
        Some("none") => (None, true),
        Some(v) => (
            Some(Uuid::parse_str(v).map_err(|_| Error::InvalidCode("ASSIGNEE_INVALID"))?),
            false,
        ),
    };
    let priority = q.priority.as_deref().filter(|v| !v.is_empty());
    if let Some(p) = priority {
        one_of(p, tasks::PRIORITIES, "PRIORITY_UNKNOWN")?;
    }
    let work_type = q.work_type.as_deref().filter(|v| !v.is_empty());
    if let Some(t) = work_type {
        one_of(t, tasks::WORK_TYPES, "WORK_TYPE_UNKNOWN")?;
    }
    let due = q.due.as_deref().filter(|v| !v.is_empty());
    if let Some(d) = due {
        one_of(d, &["overdue", "today", "week", "none"], "DUE_UNKNOWN")?;
    }
    let source = q.source.as_deref().filter(|v| !v.is_empty());
    if let Some(v) = source {
        one_of(
            v,
            &[
                "manual",
                "auto",
                "system",
                "qc",
                "distribution",
                "support",
                "rights",
                "finance",
                "sla",
            ],
            "SOURCE_UNKNOWN",
        )?;
    }
    let entity_kind = q.entity_kind.as_deref().filter(|v| !v.is_empty());
    if let Some(k) = entity_kind {
        one_of(k, entities::KINDS, "ENTITY_KIND_UNKNOWN")?;
    }
    let tag = q
        .tag
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| line(v, 30))
        .transpose()?;
    let term =
        q.q.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| line(v, 80))
            .transpose()?;
    let done_days = q.done_days.unwrap_or(30).clamp(1, 3650);

    let mut c = s.pool.acquire().await?;
    let b = readable(&mut c, &st, id).await?;
    let columns: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',bc.id,'name',bc.name,'status',bc.status,'position',bc.position,'wip_limit',bc.wip_limit,
                'count',(SELECT count(*) FROM work.tasks t WHERE t.column_id=bc.id AND t.archived_at IS NULL))
           FROM work.board_columns bc WHERE bc.board_id=$1 AND bc.archived_at IS NULL ORDER BY bc.position",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    const SQL: &str = concat!(
        "SELECT ", super::tasks::task_cols!(), " FROM work.tasks t
          JOIN work.board_columns bc ON bc.id=t.column_id
          WHERE t.board_id=$1 AND t.archived_at IS NULL AND bc.archived_at IS NULL
            AND (bc.status NOT IN ('DONE','CANCELLED') OR COALESCE(t.completed_at, t.updated_at) > now() - make_interval(days => $2))
            AND ($3::uuid IS NULL OR t.assignee_id=$3) AND (NOT $4 OR t.assignee_id IS NULL)
            AND ($5::text IS NULL OR t.priority=$5)
            AND ($6::uuid IS NULL OR t.column_id=$6)
            AND ($7::text IS NULL OR $7 = ANY(t.tags))
            AND ($8::text IS NULL OR t.work_type=$8)
            AND ($9::text IS NULL OR t.title ILIKE '%'||$9||'%' OR t.description ILIKE '%'||$9||'%' OR ('WRK-'||t.seq)=upper($9)
                 OR ", entity_name_match!(), ")
            AND ($10::text IS NULL
                 OR ($10='overdue' AND LEAST(COALESCE(t.due_at,'infinity'),COALESCE(t.sla_due_at,'infinity')) < now() AND bc.status NOT IN ('DONE','CANCELLED'))
                 OR ($10='today' AND t.due_at < date_trunc('day', now() AT TIME ZONE 'Asia/Seoul') AT TIME ZONE 'Asia/Seoul' + interval '1 day')
                 OR ($10='week' AND t.due_at < now() + interval '7 days')
                 OR ($10='none' AND t.due_at IS NULL))
            AND ($11::uuid IS NULL OR t.created_by=$11)
            AND ($12::text IS NULL OR ($12='auto' AND t.source<>'manual') OR t.source=$12)
            AND ($13::text IS NULL OR EXISTS(SELECT 1 FROM work.links l2 WHERE l2.subject_kind='task' AND l2.subject_id=t.id
                   AND l2.archived_at IS NULL AND l2.entity_kind=$13))
            AND ", crate::work_card_visible!("$14"), "
          ORDER BY bc.position, t.position, t.created_at
          LIMIT 1501"
    );
    let rows = sqlx::query(SQL)
        .bind(id)
        .bind(done_days)
        .bind(assignee)
        .bind(unassigned)
        .bind(priority)
        .bind(q.column)
        .bind(&tag)
        .bind(work_type)
        .bind(&term)
        .bind(due)
        .bind(q.creator)
        .bind(source)
        .bind(entity_kind)
        .bind(me)
        .fetch_all(&mut *c)
        .await?;
    let truncated = rows.len() > 1500;
    let cards: Vec<tasks::Task> = rows.iter().take(1500).map(tasks::Task::from_row).collect();
    let links = tasks::links_of(&mut c, &cards.iter().map(|t| t.id).collect::<Vec<_>>()).await?;
    let empty = Vec::new();
    let tags: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT unnest(tags) AS tag FROM work.tasks WHERE board_id=$1 AND archived_at IS NULL ORDER BY 1 LIMIT 200",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    Ok(json!({
        "board": b.json(&st),
        "columns": columns,
        "cards": cards.iter().map(|t| t.json(links.get(&t.id).unwrap_or(&empty))).collect::<Vec<_>>(),
        "tags": tags,
        "truncated": truncated,
    }))
}

// ---------------------------------------------------------------------------
// Columns
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColumnInput {
    pub name: Option<String>,
    pub status: Option<String>,
    #[serde(default, deserialize_with = "super::tasks::double_option")]
    pub wip_limit: Option<Option<i32>>,
}

async fn column_board(c: &mut PgConnection, col: Uuid) -> Result<(Uuid, String, String)> {
    let r = sqlx::query("SELECT board_id, name, status FROM work.board_columns WHERE id=$1 AND archived_at IS NULL FOR UPDATE")
        .bind(col)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    Ok((r.get("board_id"), r.get("name"), r.get("status")))
}

fn dup_column(e: sqlx::Error) -> Error {
    if let sqlx::Error::Database(d) = &e
        && d.code().as_deref() == Some("23505")
    {
        return Error::InvalidCode("COLUMN_EXISTS");
    }
    e.into()
}

pub async fn add_column(s: &AppState, h: &HeaderMap, board: Uuid, i: ColumnInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let name = line(i.name.as_deref().unwrap_or(""), 40)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    let status = i.status.as_deref().unwrap_or("TODO");
    one_of(status, COLUMN_STATUSES, "STATUS_UNKNOWN")?;
    let wip = i.wip_limit.flatten();
    if wip.is_some_and(|w| !(1..=999).contains(&w)) {
        return Err(Error::InvalidCode("WIP_LIMIT_RANGE"));
    }
    let mut tx = s.pool.begin().await?;
    require_manage(&mut tx, &st, board).await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.board_columns WHERE board_id=$1 AND archived_at IS NULL",
    )
    .bind(board)
    .fetch_one(&mut *tx)
    .await?;
    if n >= 20 {
        return Err(Error::InvalidCode("TOO_MANY_COLUMNS"));
    }
    sqlx::query(
        "INSERT INTO work.board_columns(board_id,name,status,position,wip_limit)
         VALUES($1,$2,$3,(SELECT COALESCE(max(position),0)+1 FROM work.board_columns WHERE board_id=$1),$4)",
    )
    .bind(board)
    .bind(&name)
    .bind(status)
    .bind(wip)
    .execute(&mut *tx)
    .await
    .map_err(dup_column)?;
    audit(&mut tx, &st, Some(board), "work.board_column_added", status).await?;
    tx.commit().await?;
    view(s, h, board, ViewQuery::default()).await
}

pub async fn update_column(
    s: &AppState,
    h: &HeaderMap,
    col: Uuid,
    i: ColumnInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (board, old_name, old_status) = column_board(&mut tx, col).await?;
    require_manage(&mut tx, &st, board).await?;
    let name = match &i.name {
        Some(n) => {
            let n = line(n, 40)?;
            if n.is_empty() {
                return Err(Error::InvalidCode("NAME_REQUIRED"));
            }
            n
        }
        None => old_name,
    };
    let status = i.status.clone().unwrap_or(old_status.clone());
    one_of(&status, COLUMN_STATUSES, "STATUS_UNKNOWN")?;
    if let Some(Some(w)) = i.wip_limit
        && !(1..=999).contains(&w)
    {
        return Err(Error::InvalidCode("WIP_LIMIT_RANGE"));
    }
    sqlx::query(
        "UPDATE work.board_columns SET name=$2, status=$3,
                wip_limit=CASE WHEN $4 THEN $5 ELSE wip_limit END WHERE id=$1",
    )
    .bind(col)
    .bind(&name)
    .bind(&status)
    .bind(i.wip_limit.is_some())
    .bind(i.wip_limit.flatten())
    .execute(&mut *tx)
    .await
    .map_err(dup_column)?;
    // the cards follow the column's new status (recorded on each card)
    if status != old_status {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE work.tasks SET status=$2, updated_at=now(), row_version=row_version+1,
                    started_at=CASE WHEN $2 IN ('IN_PROGRESS','IN_REVIEW') THEN COALESCE(started_at, now()) ELSE started_at END,
                    completed_at=CASE WHEN $2 IN ('DONE','CANCELLED') THEN COALESCE(completed_at, now()) ELSE NULL END
              WHERE column_id=$1 AND archived_at IS NULL RETURNING id",
        )
        .bind(col)
        .bind(&status)
        .fetch_all(&mut *tx)
        .await?;
        for t in ids {
            tasks::event(&mut tx, t, Some(st.actor.user), "STATUS", Some(&status)).await?;
        }
    }
    audit(
        &mut tx,
        &st,
        Some(board),
        "work.board_column_updated",
        &status,
    )
    .await?;
    tx.commit().await?;
    view(s, h, board, ViewQuery::default()).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveColumnInput {
    /// where the cards in the column go (required when it is not empty)
    pub move_to: Option<Uuid>,
}

pub async fn archive_column(
    s: &AppState,
    h: &HeaderMap,
    col: Uuid,
    i: ArchiveColumnInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (board, _, _) = column_board(&mut tx, col).await?;
    require_manage(&mut tx, &st, board).await?;
    let left: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.board_columns WHERE board_id=$1 AND archived_at IS NULL",
    )
    .bind(board)
    .fetch_one(&mut *tx)
    .await?;
    if left <= 1 {
        return Err(Error::PolicyGate("BOARD_NEEDS_COLUMN"));
    }
    let cards: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM work.tasks WHERE column_id=$1 AND archived_at IS NULL ORDER BY position",
    )
    .bind(col)
    .fetch_all(&mut *tx)
    .await?;
    if !cards.is_empty() {
        let target = i.move_to.ok_or(Error::InvalidCode("MOVE_TO_REQUIRED"))?;
        if target == col {
            return Err(Error::InvalidCode("MOVE_TO_INVALID"));
        }
        let (status, target_name): (String, String) = sqlx::query_as(
            "SELECT status, name FROM work.board_columns WHERE id=$1 AND board_id=$2 AND archived_at IS NULL",
        )
        .bind(target)
        .bind(board)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::InvalidCode("MOVE_TO_INVALID"))?;
        let start = end_of(&mut tx, target).await?;
        for (n, t) in cards.iter().enumerate() {
            sqlx::query(
                "UPDATE work.tasks SET column_id=$2, position=$3, status=$4, updated_at=now(), row_version=row_version+1,
                        completed_at=CASE WHEN $4 IN ('DONE','CANCELLED') THEN COALESCE(completed_at, now()) ELSE NULL END
                  WHERE id=$1",
            )
            .bind(t)
            .bind(target)
            .bind(start + n as f64 * GAP)
            .bind(&status)
            .execute(&mut *tx)
            .await?;
            tasks::event(
                &mut tx,
                *t,
                Some(st.actor.user),
                "MOVED",
                Some(&target_name),
            )
            .await?;
        }
    }
    sqlx::query("UPDATE work.board_columns SET archived_at=now(), archived_by=$2 WHERE id=$1")
        .bind(col)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE work.board_rules SET enabled=false, updated_at=now() WHERE column_id=$1")
        .bind(col)
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &st,
        Some(board),
        "work.board_column_archived",
        &format!("{} cards", cards.len()),
    )
    .await?;
    tx.commit().await?;
    view(s, h, board, ViewQuery::default()).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrderInput {
    pub ids: Vec<Uuid>,
}

pub async fn order_columns(
    s: &AppState,
    h: &HeaderMap,
    board: Uuid,
    i: OrderInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    require_manage(&mut tx, &st, board).await?;
    let mut current: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM work.board_columns WHERE board_id=$1 AND archived_at IS NULL FOR UPDATE",
    )
    .bind(board)
    .fetch_all(&mut *tx)
    .await?;
    let mut wanted = i.ids.clone();
    current.sort();
    wanted.sort();
    if current != wanted {
        return Err(Error::InvalidCode("COLUMN_ORDER_INVALID"));
    }
    for (n, id) in i.ids.iter().enumerate() {
        sqlx::query("UPDATE work.board_columns SET position=$2 WHERE id=$1")
            .bind(id)
            .bind(n as f64)
            .execute(&mut *tx)
            .await?;
    }
    audit(
        &mut tx,
        &st,
        Some(board),
        "work.board_columns_reordered",
        "ORDER",
    )
    .await?;
    tx.commit().await?;
    view(s, h, board, ViewQuery::default()).await
}

// ---------------------------------------------------------------------------
// Move a card (column and/or order) — status rules from tasks::apply
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveInput {
    pub row_version: i32,
    pub column_id: Uuid,
    /// the card that will be directly above / below it (None = end / start)
    pub before_id: Option<Uuid>,
    pub after_id: Option<Uuid>,
}

pub async fn move_card(s: &AppState, h: &HeaderMap, id: Uuid, i: MoveInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let t = tasks::load(&mut tx, id, true).await?;
    if t.archived_at.is_some() {
        return Err(Error::PolicyGate("TASK_ARCHIVED"));
    }
    if t.row_version != i.row_version {
        return Err(Error::Conflict);
    }
    let board = t.board_id.ok_or(Error::InvalidCode("TASK_NOT_ON_BOARD"))?;
    if !card_visible(&mut tx, &st, &t).await? {
        return Err(Error::NotFound);
    }
    card_writable(&mut tx, &st, &t).await?;
    let (col_board, col_name, col_status) = column_board(&mut tx, i.column_id).await?;
    if col_board != board {
        return Err(Error::InvalidCode("COLUMN_NOT_ON_BOARD"));
    }
    if [i.before_id, i.after_id].contains(&Some(id)) {
        return Err(Error::InvalidCode("NEIGHBOUR_INVALID"));
    }
    let from = t.column_id;
    // status first (review / subtask rules); a review redirect lands it in the review column
    if col_status != t.status {
        tasks::apply(
            &mut tx,
            &st,
            id,
            tasks::UpdateInput {
                row_version: t.row_version,
                status: Some(col_status.clone()),
                ..Default::default()
            },
        )
        .await?;
    }
    let after = tasks::load(&mut tx, id, true).await?;
    let target = if after.status == col_status {
        i.column_id
    } else {
        after.column_id.unwrap_or(i.column_id)
    };
    let position = if target == i.column_id {
        between(&mut tx, target, id, i.before_id, i.after_id).await?
    } else {
        after.position.unwrap_or(0.0)
    };
    sqlx::query("UPDATE work.tasks SET column_id=$2, position=$3, updated_at=now(), row_version=row_version+1 WHERE id=$1")
        .bind(id)
        .bind(target)
        .bind(position)
        .execute(&mut *tx)
        .await?;
    if from != Some(target) {
        // a status change already recorded the move to its status column
        if col_status == t.status || after.column_id != Some(target) {
            tasks::event(&mut tx, id, Some(st.actor.user), "MOVED", Some(&col_name)).await?;
        }
        audit(&mut tx, &st, Some(id), "work.task_moved", &col_status).await?;
    } else {
        audit(&mut tx, &st, Some(id), "work.task_reordered", "ORDER").await?;
    }
    let t = tasks::load(&mut tx, id, false).await?;
    let links = tasks::links_of(&mut tx, &[id]).await?;
    tx.commit().await?;
    Ok(
        json!({"card": t.json(links.get(&id).map(Vec::as_slice).unwrap_or(&[])), "redirected": after.status != col_status}),
    )
}

// ---------------------------------------------------------------------------
// Automation rules (which board receives cards for an AUDENIQ event)
// ---------------------------------------------------------------------------

pub async fn rules(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',r.id,'trigger',r.trigger,'board_id',r.board_id,'board_name',b.name,'column_id',r.column_id,
                'column_name',bc.name,'enabled',r.enabled AND b.archived_at IS NULL,'active_since',r.active_since,
                'cards',(SELECT count(*) FROM work.tasks t WHERE t.board_id=r.board_id AND t.source<>'manual'))
           FROM work.board_rules r JOIN work.boards b ON b.id=r.board_id JOIN work.board_columns bc ON bc.id=r.column_id
          ORDER BY r.trigger",
    )
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"items": items, "triggers": TRIGGERS}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleInput {
    pub trigger: String,
    pub board_id: Uuid,
    pub column_id: Uuid,
    pub enabled: bool,
}

pub async fn save_rule(s: &AppState, h: &HeaderMap, i: RuleInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    one_of(&i.trigger, TRIGGERS, "TRIGGER_UNKNOWN")?;
    let mut tx = s.pool.begin().await?;
    let b = load_board(&mut tx, i.board_id).await?;
    if b.archived || b.kind == "PERSONAL" {
        return Err(Error::InvalidCode("BOARD_INVALID"));
    }
    let (col_board, _, _) = column_board(&mut tx, i.column_id).await?;
    if col_board != i.board_id {
        return Err(Error::InvalidCode("COLUMN_NOT_ON_BOARD"));
    }
    sqlx::query(
        "INSERT INTO work.board_rules(board_id,column_id,trigger,enabled,created_by) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT (trigger) DO UPDATE SET board_id=EXCLUDED.board_id, column_id=EXCLUDED.column_id, enabled=EXCLUDED.enabled,
           updated_at=now(),
           -- turning a rule on starts from now (no flood of old events)
           active_since=CASE WHEN EXCLUDED.enabled AND NOT work.board_rules.enabled THEN now() ELSE work.board_rules.active_since END",
    )
    .bind(i.board_id)
    .bind(i.column_id)
    .bind(&i.trigger)
    .bind(i.enabled)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &st,
        Some(i.board_id),
        "work.board_rule_saved",
        &i.trigger,
    )
    .await?;
    tx.commit().await?;
    rules(s, h).await
}

/// Run the automation now (ADMIN; it also runs with the staff summary).
pub async fn run_automation(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    let created = super::automation::sweep(&s.pool, true).await?;
    Ok(json!({"created": created, "at": Utc::now()}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn h_list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(list(&s, &h).await?))
}
async fn h_create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<BoardInput>,
) -> Result<Json<Value>> {
    Ok(Json(create(&s, &h, i).await?))
}
async fn h_view(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<ViewQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(view(&s, &h, id, q).await?))
}
async fn h_update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<BoardInput>,
) -> Result<Json<Value>> {
    Ok(Json(update(&s, &h, id, i).await?))
}
async fn h_archive(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(archive(&s, &h, id).await?))
}
async fn h_add_column(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ColumnInput>,
) -> Result<Json<Value>> {
    Ok(Json(add_column(&s, &h, id, i).await?))
}
async fn h_order(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<OrderInput>,
) -> Result<Json<Value>> {
    Ok(Json(order_columns(&s, &h, id, i).await?))
}
async fn h_update_column(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ColumnInput>,
) -> Result<Json<Value>> {
    Ok(Json(update_column(&s, &h, id, i).await?))
}
async fn h_archive_column(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ArchiveColumnInput>,
) -> Result<Json<Value>> {
    Ok(Json(archive_column(&s, &h, id, i).await?))
}
async fn h_move(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<MoveInput>,
) -> Result<Json<Value>> {
    Ok(Json(move_card(&s, &h, id, i).await?))
}
async fn h_rules(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(rules(&s, &h).await?))
}
async fn h_save_rule(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<RuleInput>,
) -> Result<Json<Value>> {
    Ok(Json(save_rule(&s, &h, i).await?))
}
async fn h_run(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(run_automation(&s, &h).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/boards", get(h_list).post(h_create))
        .route("/api/staff/work/boards/{id}", get(h_view).post(h_update))
        .route("/api/staff/work/boards/{id}/archive", post(h_archive))
        .route("/api/staff/work/boards/{id}/columns", post(h_add_column))
        .route("/api/staff/work/boards/{id}/columns/order", post(h_order))
        .route("/api/staff/work/board-columns/{id}", post(h_update_column))
        .route(
            "/api/staff/work/board-columns/{id}/archive",
            post(h_archive_column),
        )
        .route("/api/staff/work/tasks/{id}/move", post(h_move))
        .route(
            "/api/staff/work/board-rules",
            get(h_rules).post(h_save_rule),
        )
        .route("/api/staff/work/automation/run", post(h_run))
}
