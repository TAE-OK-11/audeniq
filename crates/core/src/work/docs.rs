//! AUDENIQ Docs — internal block documents. Content is validated block JSON
//! (`doc_model`), never HTML. Access is `work.doc_level` (owner, ADMIN,
//! staff-wide access, per-role / per-user grants) and is checked on every
//! call; a document the caller cannot see is a 404, so ids cannot be probed.
//!
//! Saves use optimistic concurrency: a save names the version it edited and
//! a stale save is a 409, never a silent overwrite. Versions are snapshots
//! taken per editing session (another editor, or ten quiet minutes), on
//! status changes, restores and "save version" — not on every keystroke.
//! Embedded AUDENIQ entities are kept as `work.links` (origin = embed), so
//! releases and cards list the documents about them; the live data of an
//! embed is read from its source on display (`/embeds`).
use super::{audit, doc_model, entities, line, one_of};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff::{self, Staff},
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
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

pub const VIEWER: i32 = 1;
pub const EDITOR: i32 = 2;
pub const MANAGER: i32 = 3;
pub const OWNER: i32 = 4;
const MAX_DEPTH: i16 = 4;
const SESSION_MINUTES: i64 = 10;

fn level_name(l: i32) -> &'static str {
    match l {
        4 => "OWNER",
        3 => "MANAGER",
        2 => "EDITOR",
        1 => "VIEWER",
        _ => "NONE",
    }
}

macro_rules! doc_cols {
    () => {
        "d.id, d.parent_id, d.depth, d.title, d.icon, d.status, d.staff_access, d.version, d.position, d.template_id,
         d.created_by, d.updated_by, d.created_at, d.updated_at, d.archived_at,
         d.review_requested_by, d.review_requested_at, d.approved_by, d.approved_at, d.rejected_by, d.rejected_at, d.rejection_reason"
    };
}

fn summary(r: &PgRow, level: i32) -> Value {
    json!({
        "id": r.get::<Uuid,_>("id"), "parent_id": r.get::<Option<Uuid>,_>("parent_id"), "depth": r.get::<i16,_>("depth"),
        "title": r.get::<String,_>("title"), "icon": r.get::<Option<String>,_>("icon"), "status": r.get::<String,_>("status"),
        "staff_access": r.get::<Option<String>,_>("staff_access"), "version": r.get::<i32,_>("version"),
        "position": r.get::<f64,_>("position"), "template_id": r.get::<Option<Uuid>,_>("template_id"),
        "created_by": r.get::<Uuid,_>("created_by"), "updated_by": r.get::<Uuid,_>("updated_by"),
        "created_at": r.get::<DateTime<Utc>,_>("created_at"), "updated_at": r.get::<DateTime<Utc>,_>("updated_at"),
        "archived_at": r.get::<Option<DateTime<Utc>>,_>("archived_at"),
        "review": {
            "requested_by": r.get::<Option<Uuid>,_>("review_requested_by"), "requested_at": r.get::<Option<DateTime<Utc>>,_>("review_requested_at"),
            "approved_by": r.get::<Option<Uuid>,_>("approved_by"), "approved_at": r.get::<Option<DateTime<Utc>>,_>("approved_at"),
            "rejected_by": r.get::<Option<Uuid>,_>("rejected_by"), "rejected_at": r.get::<Option<DateTime<Utc>>,_>("rejected_at"),
            "rejection_reason": r.get::<Option<String>,_>("rejection_reason"),
        },
        "level": level_name(level),
    })
}

/// The caller's level on a document (0 = cannot see it).
pub(crate) async fn level(c: &mut PgConnection, st: &Staff, id: Uuid) -> Result<i32> {
    Ok(sqlx::query_scalar("SELECT work.doc_level($1,$2,$3)")
        .bind(id)
        .bind(st.actor.user)
        .bind(st.role.team())
        .fetch_one(&mut *c)
        .await?)
}

/// Load a document the caller may see at `need`; hidden documents are a 404.
async fn load(
    c: &mut PgConnection,
    st: &Staff,
    id: Uuid,
    need: i32,
    lock: bool,
) -> Result<(PgRow, i32)> {
    let lv = level(c, st, id).await?;
    if lv == 0 {
        return Err(Error::NotFound);
    }
    if lv < need {
        return Err(Error::Forbidden);
    }
    let r = if lock {
        sqlx::query(concat!(
            "SELECT ",
            doc_cols!(),
            ", d.content, d.content_text FROM work.docs d WHERE d.id=$1 FOR UPDATE"
        ))
    } else {
        sqlx::query(concat!(
            "SELECT ",
            doc_cols!(),
            ", d.content, d.content_text FROM work.docs d WHERE d.id=$1"
        ))
    }
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    Ok((r, lv))
}

fn title_of(v: &str) -> Result<String> {
    line(v, 200)
}

fn icon_of(v: Option<&str>) -> Result<Option<String>> {
    match v.map(str::trim).filter(|v| !v.is_empty()) {
        None => Ok(None),
        Some(v)
            if v.chars().count() <= 4
                && v.len() <= 16
                && !v.chars().any(|c| {
                    c.is_ascii_alphanumeric() || c.is_control() || "<>\"'&".contains(c)
                }) =>
        {
            Ok(Some(v.to_string()))
        }
        _ => Err(Error::InvalidCode("DOC_ICON_INVALID")),
    }
}

// ---------------------------------------------------------------------------
// Tree, search, recent
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct TreeQuery {
    /// children of this document (absent = top level)
    pub parent: Option<Uuid>,
    #[serde(default)]
    pub archived: bool,
}

/// One level of the tree (lazy). Documents the caller cannot see are left
/// out; a visible child of a hidden parent is listed at the top level.
pub async fn tree(s: &AppState, h: &HeaderMap, q: TreeQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    if let Some(p) = q.parent
        && level(&mut c, &st, p).await? == 0
    {
        return Err(Error::NotFound);
    }
    let rows = sqlx::query(concat!(
        "SELECT ", doc_cols!(), ", work.doc_level(d.id,$2,$3) AS lv,
                EXISTS(SELECT 1 FROM work.docs k WHERE k.parent_id=d.id AND k.deleted_at IS NULL AND (k.archived_at IS NULL) <> $4
                         AND work.doc_level(k.id,$2,$3) > 0) AS has_children
           FROM work.docs d
          WHERE d.deleted_at IS NULL AND (d.archived_at IS NULL) <> $4
            AND work.doc_level(d.id,$2,$3) > 0
            AND CASE WHEN $1::uuid IS NULL
                     THEN d.parent_id IS NULL OR work.doc_level(d.parent_id,$2,$3) = 0
                          OR ($4 AND (SELECT p.archived_at IS NULL FROM work.docs p WHERE p.id=d.parent_id))
                     ELSE d.parent_id = $1 END
          ORDER BY d.position, d.created_at
          LIMIT 500"
    ))
    .bind(q.parent)
    .bind(st.actor.user)
    .bind(st.role.team())
    .bind(q.archived)
    .fetch_all(&mut *c)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut v = summary(r, r.get("lv"));
            v["has_children"] = json!(r.get::<bool, _>("has_children"));
            v
        })
        .collect();
    Ok(json!({ "items": items }))
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    pub q: Option<String>,
    pub status: Option<String>,
    pub author: Option<Uuid>,
    pub entity_kind: Option<String>,
    pub entity_id: Option<String>,
    pub limit: Option<i64>,
}

/// Full-text search (title + body), author, status and related entity.
pub async fn search(s: &AppState, h: &HeaderMap, q: SearchQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let term =
        q.q.as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(|t| line(t, 120))
            .transpose()?;
    let status = q.status.as_deref().filter(|v| !v.is_empty());
    if let Some(v) = status {
        one_of(v, &["DRAFT", "IN_REVIEW", "APPROVED"], "STATUS_UNKNOWN")?;
    }
    let entity = match (q.entity_kind.as_deref(), q.entity_id.as_deref()) {
        (Some(k), Some(i)) if !k.is_empty() && !i.is_empty() => Some(entities::parse(k, i)?),
        _ => None,
    };
    let limit = q.limit.unwrap_or(30).clamp(1, 100);
    let rows = sqlx::query(concat!(
        "SELECT ", doc_cols!(), ", work.doc_level(d.id,$2,$3) AS lv,
                CASE WHEN $1::text IS NULL THEN NULL ELSE
                  substr(d.content_text, greatest(1, strpos(lower(d.content_text), lower($1)) - 60), 220) END AS snippet,
                CASE WHEN $1::text IS NULL THEN 0 ELSE ts_rank(d.search, plainto_tsquery('simple', $1))
                     + CASE WHEN d.title ILIKE '%'||$1||'%' THEN 1 ELSE 0 END END AS rank
           FROM work.docs d
          WHERE d.deleted_at IS NULL AND d.archived_at IS NULL AND work.doc_level(d.id,$2,$3) > 0
            AND ($1::text IS NULL OR d.search @@ plainto_tsquery('simple', $1)
                 OR d.title ILIKE '%'||$1||'%' OR d.content_text ILIKE '%'||$1||'%'
                 OR EXISTS(SELECT 1 FROM identity.users u WHERE u.id=d.created_by
                           AND (u.email=lower($1) OR u.email_bidx=ANY($9))))
            AND ($4::text IS NULL OR d.status=$4)
            AND ($5::uuid IS NULL OR d.created_by=$5)
            AND ($6::text IS NULL OR EXISTS(SELECT 1 FROM work.links l WHERE l.subject_kind='doc' AND l.subject_id=d.id
                   AND l.archived_at IS NULL AND l.entity_kind=$6 AND l.entity_id=$7)
                 OR EXISTS(SELECT 1 FROM work.links l WHERE l.entity_kind='doc' AND l.entity_id=d.id::text AND l.archived_at IS NULL
                   AND l.subject_kind=$6 AND l.subject_id::text=$7))
          ORDER BY rank DESC, d.updated_at DESC
          LIMIT $8"
    ))
    .bind(&term)
    .bind(st.actor.user)
    .bind(st.role.team())
    .bind(status)
    .bind(q.author)
    .bind(entity.as_ref().map(|e| e.kind.clone()))
    .bind(entity.as_ref().map(|e| e.id.clone()))
    .bind(limit)
    // Authors match by their full email (stored sealed, so no prefix search).
    .bind(
        term.as_deref()
            .map(|t| crate::pii::email_indexes(&s.payout_keys, &t.trim().to_lowercase()))
            .unwrap_or_default(),
    )
    .fetch_all(&s.pool)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let mut v = summary(r, r.get("lv"));
            v["snippet"] = json!(r.get::<Option<String>, _>("snippet"));
            v
        })
        .collect();
    Ok(json!({ "items": items }))
}

// ---------------------------------------------------------------------------
// Create, read
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub title: String,
    pub icon: Option<String>,
    pub template_id: Option<Uuid>,
    /// start linked to a record (a card's "new document", a release, …)
    pub link: Option<super::tasks::LinkInput>,
}

async fn snapshot(
    c: &mut PgConnection,
    doc: Uuid,
    reason: &str,
    label: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO work.doc_versions(doc_id,version,title,icon,content,content_text,reason,label,created_by,created_at)
         SELECT id,version,title,icon,content,content_text,$2,$3,updated_by,updated_at FROM work.docs WHERE id=$1
         ON CONFLICT (doc_id, version) DO NOTHING",
    )
    .bind(doc)
    .bind(reason)
    .bind(label)
    .execute(&mut *c)
    .await?;
    Ok(())
}

pub async fn create(s: &AppState, h: &HeaderMap, i: CreateInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut title = title_of(&i.title)?;
    let mut icon = icon_of(i.icon.as_deref())?;
    let link = i
        .link
        .as_ref()
        .map(|l| entities::parse(&l.entity_kind, &l.entity_id))
        .transpose()?;
    let mut tx = s.pool.begin().await?;
    let (depth, staff_access): (i16, Option<String>) = match i.parent_id {
        Some(p) => {
            let (r, _) = load(&mut tx, &st, p, EDITOR, false).await?;
            if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
                return Err(Error::PolicyGate("DOC_ARCHIVED"));
            }
            let d: i16 = r.get("depth");
            if d >= MAX_DEPTH {
                return Err(Error::PolicyGate("DOC_TOO_DEEP"));
            }
            (d + 1, r.get("staff_access"))
        }
        None => (0, Some("EDITOR".into())),
    };
    let content = match i.template_id {
        Some(t) => {
            let r = sqlx::query("SELECT name, icon, content FROM work.doc_templates WHERE id=$1 AND archived_at IS NULL")
                .bind(t)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(Error::InvalidCode("TEMPLATE_NOT_FOUND"))?;
            if title.is_empty() {
                title = r.get("name");
            }
            if icon.is_none() {
                icon = r.get("icon");
            }
            r.get::<Value, _>("content")
        }
        None => doc_model::empty(),
    };
    let derived = doc_model::validate(&content)?;
    if let Some(r) = &link {
        entities::require(&mut tx, r).await?;
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.docs(id,parent_id,depth,title,icon,content,content_text,staff_access,template_id,created_by,updated_by,position)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10,
                (SELECT COALESCE(max(position),0)+1024 FROM work.docs WHERE parent_id IS NOT DISTINCT FROM $2 AND deleted_at IS NULL))",
    )
    .bind(id)
    .bind(i.parent_id)
    .bind(depth)
    .bind(&title)
    .bind(&icon)
    .bind(&content)
    .bind(&derived.text)
    .bind(&staff_access)
    .bind(i.template_id)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    // a child starts with its parent's sharing
    if let Some(p) = i.parent_id {
        sqlx::query(
            "INSERT INTO work.doc_permissions(doc_id,principal_kind,principal_id,level,granted_by)
             SELECT $1,principal_kind,principal_id,level,$3 FROM work.doc_permissions WHERE doc_id=$2",
        )
        .bind(id)
        .bind(p)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await?;
    }
    sync_links(&mut tx, &st, id, &derived.embeds).await?;
    if let Some(r) = &link {
        // a card's documents are links from the card; anything else: the document holds the link
        if r.kind == "task" {
            let task =
                Uuid::parse_str(&r.id).map_err(|_| Error::InvalidCode("ENTITY_ID_INVALID"))?;
            let t = super::tasks::load(&mut tx, task, false).await?;
            if !super::kanban::card_visible(&mut tx, &st, &t).await? {
                return Err(Error::InvalidCode("ENTITY_NOT_FOUND"));
            }
            link_row(
                &mut tx,
                "task",
                task,
                "doc",
                &id.to_string(),
                st.actor.user,
                "manual",
            )
            .await?;
            super::tasks::event(
                &mut tx,
                task,
                Some(st.actor.user),
                "LINKED",
                Some(&format!("doc:{id}")),
            )
            .await?;
        } else {
            link_row(&mut tx, "doc", id, &r.kind, &r.id, st.actor.user, "manual").await?;
        }
    }
    snapshot(&mut tx, id, "CREATE", None).await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.doc_created",
        if i.template_id.is_some() {
            "TEMPLATE"
        } else {
            "BLANK"
        },
    )
    .await?;
    tx.commit().await?;
    detail(s, h, id).await
}

async fn link_row(
    c: &mut PgConnection,
    subject_kind: &str,
    subject: Uuid,
    kind: &str,
    id: &str,
    by: Uuid,
    origin: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO work.links(id,subject_kind,subject_id,entity_kind,entity_id,created_by,origin) VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT (subject_kind, subject_id, entity_kind, entity_id) WHERE archived_at IS NULL DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(subject_kind)
    .bind(subject)
    .bind(kind)
    .bind(id)
    .bind(by)
    .bind(origin)
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// Keep the document's embed links equal to what its content embeds
/// (records that no longer exist are skipped, not an error).
async fn sync_links(
    c: &mut PgConnection,
    st: &Staff,
    doc: Uuid,
    embeds: &BTreeSet<(String, String)>,
) -> Result<()> {
    let wanted: Vec<entities::Ref> = embeds
        .iter()
        // audit trails are shown, not linked; a document never links itself
        .filter(|(k, i)| {
            entities::KINDS.contains(&k.as_str()) && !(k == "doc" && *i == doc.to_string())
        })
        .map(|(k, i)| entities::Ref {
            kind: k.clone(),
            id: i.clone(),
        })
        .collect();
    let labels = entities::resolve(c, &wanted).await?;
    let keep: Vec<String> = wanted
        .iter()
        .filter(|r| labels.get(*r).is_some_and(|v| v.get("missing").is_none()))
        .map(|r| format!("{}:{}", r.kind, r.id))
        .collect();
    sqlx::query(
        "UPDATE work.links SET archived_at=now(), archived_by=$2
          WHERE subject_kind='doc' AND subject_id=$1 AND origin='embed' AND archived_at IS NULL
            AND NOT (entity_kind || ':' || entity_id = ANY($3))",
    )
    .bind(doc)
    .bind(st.actor.user)
    .bind(&keep)
    .execute(&mut *c)
    .await?;
    for k in &keep {
        let (kind, id) = k.split_once(':').unwrap_or(("", ""));
        link_row(c, "doc", doc, kind, id, st.actor.user, "embed").await?;
    }
    Ok(())
}

pub async fn detail(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let (r, lv) = load(&mut c, &st, id, VIEWER, false).await?;
    let mut doc = summary(&r, lv);
    doc["content"] = r.get::<Value, _>("content");
    // breadcrumb: visible ancestors only
    let crumbs: Vec<Value> = sqlx::query_scalar(
        "WITH RECURSIVE up AS (
           SELECT p.id, p.parent_id, p.title, p.icon, 1 AS n FROM work.docs p WHERE p.id=(SELECT parent_id FROM work.docs WHERE id=$1)
           UNION ALL SELECT p.id, p.parent_id, p.title, p.icon, up.n+1 FROM work.docs p JOIN up ON p.id=up.parent_id WHERE up.n < 10)
         SELECT jsonb_build_object('id',id,'title',title,'icon',icon) FROM up WHERE work.doc_level(id,$2,$3) > 0 ORDER BY n DESC",
    )
    .bind(id)
    .bind(st.actor.user)
    .bind(st.role.team())
    .fetch_all(&mut *c)
    .await?;
    // records this document is about (embeds + manual links), labels from the sources
    let rows = sqlx::query(
        "SELECT id, entity_kind, entity_id, origin FROM work.links WHERE subject_kind='doc' AND subject_id=$1 AND archived_at IS NULL ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let refs: Vec<entities::Ref> = rows
        .iter()
        .map(|r| entities::Ref {
            kind: r.get("entity_kind"),
            id: r.get("entity_id"),
        })
        .collect();
    let labels = entities::resolve(&mut c, &refs).await?;
    let related: Vec<Value> = rows
        .iter()
        .zip(refs.iter())
        .filter(|(_, rf)| rf.kind != "task" && rf.kind != "doc")
        .map(|(r, rf)| {
            let mut v = labels.get(rf).cloned().unwrap_or(Value::Null);
            v["link_id"] = json!(r.get::<Uuid, _>("id"));
            v["origin"] = json!(r.get::<String, _>("origin"));
            v
        })
        .collect();
    // cards: linked from the card, or embedded here — only cards the caller may see
    let card_ids: Vec<Uuid> = sqlx::query_scalar(concat!(
        "SELECT t.id FROM work.tasks t WHERE t.archived_at IS NULL AND (
            EXISTS(SELECT 1 FROM work.links l WHERE l.subject_kind='task' AND l.subject_id=t.id AND l.entity_kind='doc' AND l.entity_id=$1::text AND l.archived_at IS NULL)
            OR EXISTS(SELECT 1 FROM work.links l WHERE l.subject_kind='doc' AND l.subject_id=$1 AND l.entity_kind='task' AND l.entity_id=t.id::text AND l.archived_at IS NULL))
           AND ", crate::work_card_visible!("$2"), " ORDER BY t.updated_at DESC LIMIT 50"
    ))
    .bind(id)
    .bind(st.actor.user)
    .fetch_all(&mut *c)
    .await?;
    let cards: Vec<Value> = if card_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_scalar(
            "SELECT jsonb_build_object('id',t.id,'key','WRK-'||t.seq,'title',t.title,'status',t.status,'priority',t.priority,
                    'assignee_id',t.assignee_id,'due_at',t.due_at,'column_name',bc.name,'board_id',t.board_id)
               FROM work.tasks t LEFT JOIN work.board_columns bc ON bc.id=t.column_id WHERE t.id = ANY($1) ORDER BY t.updated_at DESC",
        )
        .bind(&card_ids)
        .fetch_all(&mut *c)
        .await?
    };
    // other documents that embed or mention this one
    let backlinks: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',d.id,'title',d.title,'icon',d.icon) FROM work.links l JOIN work.docs d ON d.id=l.subject_id
          WHERE l.subject_kind='doc' AND l.entity_kind='doc' AND l.entity_id=$1::text AND l.archived_at IS NULL
            AND d.deleted_at IS NULL AND work.doc_level(d.id,$2,$3) > 0 ORDER BY d.updated_at DESC LIMIT 50",
    )
    .bind(id)
    .bind(st.actor.user)
    .bind(st.role.team())
    .fetch_all(&mut *c)
    .await?;
    let comments: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.notes WHERE target_kind='doc' AND target_id=$1::text AND archived_at IS NULL AND resolved_at IS NULL AND parent_id IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *c)
    .await?;
    Ok(json!({
        "doc": doc, "breadcrumb": crumbs, "related": related, "cards": cards, "backlinks": backlinks,
        "open_comments": comments,
    }))
}

// ---------------------------------------------------------------------------
// Save (autosave) with optimistic concurrency
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveInput {
    pub base_version: i32,
    pub title: Option<String>,
    #[serde(default, deserialize_with = "super::tasks::double_option")]
    pub icon: Option<Option<String>>,
    pub content: Option<Value>,
}

/// Attachments the content shows must be ready uploads the caller may use:
/// new ones (not yet on a document) uploaded by them, or ones already on a
/// document they can read. New ones become this document's.
async fn claim_attachments(
    c: &mut PgConnection,
    st: &Staff,
    doc: Uuid,
    ids: &BTreeSet<Uuid>,
) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let list: Vec<Uuid> = ids.iter().copied().collect();
    let rows = sqlx::query(
        "SELECT id, doc_id, uploaded_by, note_id FROM work.attachments WHERE id = ANY($1) AND state='READY' AND archived_at IS NULL",
    )
    .bind(&list)
    .fetch_all(&mut *c)
    .await?;
    if rows.len() != list.len() {
        return Err(Error::InvalidCode("ATTACHMENT_NOT_READY"));
    }
    for r in rows {
        let att: Uuid = r.get("id");
        match r.get::<Option<Uuid>, _>("doc_id") {
            Some(d) if d == doc => {}
            Some(d) => {
                if level(c, st, d).await? == 0 {
                    return Err(Error::InvalidCode("ATTACHMENT_NOT_READY"));
                }
            }
            None => {
                if r.get::<Uuid, _>("uploaded_by") != st.actor.user
                    || r.get::<Option<Uuid>, _>("note_id").is_some()
                {
                    return Err(Error::InvalidCode("ATTACHMENT_NOT_READY"));
                }
                sqlx::query("UPDATE work.attachments SET doc_id=$2 WHERE id=$1 AND doc_id IS NULL")
                    .bind(att)
                    .bind(doc)
                    .execute(&mut *c)
                    .await?;
            }
        }
    }
    Ok(())
}

pub async fn save(s: &AppState, h: &HeaderMap, id: Uuid, i: SaveInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let me = st.actor.user;
    let mut tx = s.pool.begin().await?;
    let (r, _) = load(&mut tx, &st, id, EDITOR, true).await?;
    if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
        return Err(Error::PolicyGate("DOC_ARCHIVED"));
    }
    let current: i32 = r.get("version");
    if current != i.base_version {
        // someone else saved in between: never overwrite, the editor reloads / merges
        return Err(Error::Conflict);
    }
    let title = match &i.title {
        Some(t) => title_of(t)?,
        None => r.get("title"),
    };
    let icon = match &i.icon {
        Some(v) => icon_of(v.as_deref())?,
        None => r.get("icon"),
    };
    let (content, text) = match &i.content {
        Some(cv) => {
            let d = doc_model::validate(cv)?;
            claim_attachments(&mut tx, &st, id, &d.attachments).await?;
            sync_links(&mut tx, &st, id, &d.embeds).await?;
            (cv.clone(), d.text)
        }
        None => (
            r.get::<Value, _>("content"),
            r.get::<String, _>("content_text"),
        ),
    };
    // a new editing session (another editor, or a pause) keeps the previous state as a version
    let updated_by: Uuid = r.get("updated_by");
    let updated_at: DateTime<Utc> = r.get("updated_at");
    let new_session =
        updated_by != me || updated_at < Utc::now() - Duration::minutes(SESSION_MINUTES);
    if new_session {
        snapshot(&mut tx, id, "AUTOSAVE", None).await?;
    }
    // editing reviewed / approved content sends it back to draft
    let status: String = r.get("status");
    let content_changed = i.content.is_some() || i.title.is_some();
    let reopen = content_changed && status != "DRAFT";
    let version: i32 = sqlx::query_scalar(
        "UPDATE work.docs SET title=$3, icon=$4, content=$5, content_text=$6, updated_by=$7, updated_at=now(), version=version+1,
                status=CASE WHEN $8 THEN 'DRAFT' ELSE status END
          WHERE id=$1 AND version=$2 RETURNING version",
    )
    .bind(id)
    .bind(current)
    .bind(&title)
    .bind(&icon)
    .bind(&content)
    .bind(&text)
    .bind(me)
    .bind(reopen)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    if new_session || reopen {
        audit(
            &mut tx,
            &st,
            Some(id),
            "work.doc_edited",
            if reopen { "REOPENED" } else { "SESSION" },
        )
        .await?;
    }
    tx.commit().await?;
    Ok(
        json!({"id": id, "version": version, "status": if reopen { "DRAFT".to_string() } else { status }, "saved_at": Utc::now()}),
    )
}

// ---------------------------------------------------------------------------
// Tree changes: move, duplicate, archive, restore, delete
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveInput {
    pub parent_id: Option<Uuid>,
    /// sibling that ends up directly before / after it
    pub before_id: Option<Uuid>,
    pub after_id: Option<Uuid>,
}

pub async fn move_doc(s: &AppState, h: &HeaderMap, id: Uuid, i: MoveInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    load(&mut tx, &st, id, MANAGER, true).await?;
    // subtree height and cycle check
    let sub = sqlx::query(
        "WITH RECURSIVE down AS (SELECT id, 0 AS h FROM work.docs WHERE id=$1
           UNION ALL SELECT d.id, down.h+1 FROM work.docs d JOIN down ON d.parent_id=down.id WHERE down.h < 10)
         SELECT max(h) AS height, bool_or(id = $2) AS cycle FROM down",
    )
    .bind(id)
    .bind(i.parent_id)
    .fetch_one(&mut *tx)
    .await?;
    if sub.get::<Option<bool>, _>("cycle").unwrap_or(false) {
        return Err(Error::InvalidCode("DOC_MOVE_CYCLE"));
    }
    let height: i32 = sub.get::<Option<i32>, _>("height").unwrap_or(0);
    let depth: i16 = match i.parent_id {
        Some(p) => {
            let (pr, _) = load(&mut tx, &st, p, EDITOR, false).await?;
            if pr.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
                return Err(Error::PolicyGate("DOC_ARCHIVED"));
            }
            pr.get::<i16, _>("depth") + 1
        }
        None => 0,
    };
    if depth as i32 + height > MAX_DEPTH as i32 {
        return Err(Error::PolicyGate("DOC_TOO_DEEP"));
    }
    let (before, after) = (i.before_id, i.after_id);
    let mut positions: HashMap<Uuid, f64> = HashMap::new();
    for x in [before, after].into_iter().flatten() {
        let p: Option<f64> = sqlx::query_scalar(
            "SELECT position FROM work.docs WHERE id=$1 AND parent_id IS NOT DISTINCT FROM $2 AND deleted_at IS NULL",
        )
        .bind(x)
        .bind(i.parent_id)
        .fetch_optional(&mut *tx)
        .await?;
        positions.insert(x, p.ok_or(Error::InvalidCode("NEIGHBOUR_INVALID"))?);
    }
    let a = before.and_then(|x| positions.get(&x).copied());
    let b = after.and_then(|x| positions.get(&x).copied());
    let position = match (a, b) {
        (Some(a), Some(b)) if b - a > 1e-6 => (a + b) / 2.0,
        (Some(a), None) => a + 1024.0,
        (None, Some(b)) => b - 1024.0,
        _ => {
            // renumber siblings, then append
            sqlx::query(
                "UPDATE work.docs d SET position = o.n * 1024 FROM (
                   SELECT id, row_number() OVER (ORDER BY position, created_at) AS n FROM work.docs
                    WHERE parent_id IS NOT DISTINCT FROM $1 AND deleted_at IS NULL AND id<>$2) o WHERE d.id=o.id",
            )
            .bind(i.parent_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            let max: Option<f64> = sqlx::query_scalar(
                "SELECT max(position) FROM work.docs WHERE parent_id IS NOT DISTINCT FROM $1 AND deleted_at IS NULL AND id<>$2",
            )
            .bind(i.parent_id)
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            match (a, b) {
                (Some(_), Some(_)) => {
                    // place right after `before` in the new numbering
                    let ap: f64 = sqlx::query_scalar("SELECT position FROM work.docs WHERE id=$1")
                        .bind(before)
                        .fetch_one(&mut *tx)
                        .await?;
                    ap + 512.0
                }
                _ => max.unwrap_or(0.0) + 1024.0,
            }
        }
    };
    sqlx::query("UPDATE work.docs SET parent_id=$2, depth=$3, position=$4 WHERE id=$1")
        .bind(id)
        .bind(i.parent_id)
        .bind(depth)
        .bind(position)
        .execute(&mut *tx)
        .await?;
    // children follow their parent's depth
    sqlx::query(
        "WITH RECURSIVE down AS (SELECT id, $2::smallint AS depth FROM work.docs WHERE id=$1
           UNION ALL SELECT d.id, (down.depth+1)::smallint FROM work.docs d JOIN down ON d.parent_id=down.id)
         UPDATE work.docs d SET depth=down.depth FROM down WHERE d.id=down.id AND d.depth<>down.depth",
    )
    .bind(id)
    .bind(depth)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &st, Some(id), "work.doc_moved", "MOVE").await?;
    tx.commit().await?;
    detail(s, h, id).await
}

pub async fn duplicate(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (r, _) = load(&mut tx, &st, id, VIEWER, false).await?;
    let parent: Option<Uuid> = r.get("parent_id");
    if let Some(p) = parent
        && level(&mut tx, &st, p).await? < EDITOR
    {
        return Err(Error::Forbidden);
    }
    let copy = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.docs(id,parent_id,depth,title,icon,content,content_text,staff_access,template_id,created_by,updated_by,position)
         SELECT $2,parent_id,depth,left(title || ' (사본)', 200),icon,content,content_text,staff_access,template_id,$3,$3,position+0.5
           FROM work.docs WHERE id=$1",
    )
    .bind(id)
    .bind(copy)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO work.doc_permissions(doc_id,principal_kind,principal_id,level,granted_by)
         SELECT $2,principal_kind,principal_id,level,$3 FROM work.doc_permissions WHERE doc_id=$1",
    )
    .bind(id)
    .bind(copy)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO work.links(id,subject_kind,subject_id,entity_kind,entity_id,created_by,origin)
         SELECT gen_random_uuid(),'doc',$2,entity_kind,entity_id,$3,origin FROM work.links
          WHERE subject_kind='doc' AND subject_id=$1 AND archived_at IS NULL",
    )
    .bind(id)
    .bind(copy)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    snapshot(&mut tx, copy, "CREATE", None).await?;
    audit(&mut tx, &st, Some(copy), "work.doc_created", "DUPLICATE").await?;
    tx.commit().await?;
    detail(s, h, copy).await
}

pub async fn archive(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (r, _) = load(&mut tx, &st, id, MANAGER, true).await?;
    if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
        return Err(Error::Conflict);
    }
    // the whole subtree goes to the archive together
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE down AS (SELECT id FROM work.docs WHERE id=$1 UNION ALL SELECT d.id FROM work.docs d JOIN down ON d.parent_id=down.id)
         UPDATE work.docs d SET archived_at=now(), archived_by=$2 FROM down WHERE d.id=down.id AND d.archived_at IS NULL AND d.deleted_at IS NULL
         RETURNING d.id",
    )
    .bind(id)
    .bind(st.actor.user)
    .fetch_all(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.doc_archived",
        &format!("{} docs", ids.len()),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"archived": ids}))
}

pub async fn restore(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (r, _) = load(&mut tx, &st, id, MANAGER, true).await?;
    let at: Option<DateTime<Utc>> = r.get("archived_at");
    let at = at.ok_or(Error::Conflict)?;
    // a document whose parent is still archived comes back at the top level
    let parent: Option<Uuid> = r.get("parent_id");
    if let Some(p) = parent {
        let parent_archived: bool =
            sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM work.docs WHERE id=$1")
                .bind(p)
                .fetch_one(&mut *tx)
                .await?;
        if parent_archived {
            sqlx::query(
                "WITH RECURSIVE down AS (SELECT id, 0::smallint AS depth FROM work.docs WHERE id=$1
                   UNION ALL SELECT d.id, (down.depth+1)::smallint FROM work.docs d JOIN down ON d.parent_id=down.id)
                 UPDATE work.docs d SET depth=down.depth, parent_id=CASE WHEN d.id=$1 THEN NULL ELSE d.parent_id END FROM down WHERE d.id=down.id",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE down AS (SELECT id FROM work.docs WHERE id=$1 UNION ALL SELECT d.id FROM work.docs d JOIN down ON d.parent_id=down.id)
         UPDATE work.docs d SET archived_at=NULL, archived_by=NULL FROM down
          WHERE d.id=down.id AND d.deleted_at IS NULL AND d.archived_at >= $2 - interval '1 second'
         RETURNING d.id",
    )
    .bind(id)
    .bind(at)
    .fetch_all(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.doc_restored",
        &format!("{} docs", ids.len()),
    )
    .await?;
    tx.commit().await?;
    detail(s, h, id).await
}

/// Permanently remove an archived document (owner or ADMIN). History stays for audit.
pub async fn delete(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (r, lv) = load(&mut tx, &st, id, MANAGER, true).await?;
    if lv < OWNER && !st.role.is_admin() {
        return Err(Error::Forbidden);
    }
    if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_none() {
        return Err(Error::PolicyGate("DOC_NOT_ARCHIVED"));
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE down AS (SELECT id FROM work.docs WHERE id=$1 UNION ALL SELECT d.id FROM work.docs d JOIN down ON d.parent_id=down.id)
         UPDATE work.docs d SET deleted_at=now(), deleted_by=$2,
                archived_at=COALESCE(d.archived_at, now()), archived_by=COALESCE(d.archived_by, $2)
           FROM down WHERE d.id=down.id AND d.deleted_at IS NULL RETURNING d.id",
    )
    .bind(id)
    .bind(st.actor.user)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query("UPDATE work.links SET archived_at=now(), archived_by=$2 WHERE archived_at IS NULL AND ((subject_kind='doc' AND subject_id = ANY($1)) OR (entity_kind='doc' AND entity_id = ANY($3)))")
        .bind(&ids)
        .bind(st.actor.user)
        .bind(ids.iter().map(Uuid::to_string).collect::<Vec<_>>())
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.doc_deleted",
        &format!("{} docs", ids.len()),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"deleted": ids}))
}

// ---------------------------------------------------------------------------
// Status (review / approval)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusInput {
    /// REQUEST_REVIEW · APPROVE · REJECT · REOPEN
    pub action: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub async fn set_status(s: &AppState, h: &HeaderMap, id: Uuid, i: StatusInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let me = st.actor.user;
    let mut tx = s.pool.begin().await?;
    let (r, lv) = load(&mut tx, &st, id, EDITOR, true).await?;
    if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
        return Err(Error::PolicyGate("DOC_ARCHIVED"));
    }
    let status: String = r.get("status");
    let sql = match (i.action.as_str(), status.as_str()) {
        ("REQUEST_REVIEW", "DRAFT") => {
            "UPDATE work.docs SET status='IN_REVIEW', review_requested_by=$2, review_requested_at=now(),
                    approved_by=NULL, approved_at=NULL, rejected_by=NULL, rejected_at=NULL, rejection_reason=NULL WHERE id=$1"
        }
        ("APPROVE", "IN_REVIEW") => {
            if lv < MANAGER {
                return Err(Error::Forbidden);
            }
            // the requester does not approve their own request (ADMIN may;
            // anyone may while two-person approval is off)
            if crate::config::dual_approval_enabled()
                && r.get::<Option<Uuid>, _>("review_requested_by") == Some(me)
                && !st.role.is_admin()
            {
                return Err(Error::PolicyGate("DOC_SELF_APPROVAL"));
            }
            "UPDATE work.docs SET status='APPROVED', approved_by=$2, approved_at=now() WHERE id=$1"
        }
        ("REJECT", "IN_REVIEW") => {
            if lv < MANAGER {
                return Err(Error::Forbidden);
            }
            "UPDATE work.docs SET status='DRAFT', rejected_by=$2, rejected_at=now(), rejection_reason=$3 WHERE id=$1"
        }
        ("REOPEN", "APPROVED" | "IN_REVIEW") => "UPDATE work.docs SET status='DRAFT' WHERE id=$1",
        ("REQUEST_REVIEW" | "APPROVE" | "REJECT" | "REOPEN", _) => return Err(Error::PolicyGate("DOC_STATUS_INVALID")),
        _ => return Err(Error::InvalidCode("ACTION_UNKNOWN")),
    };
    let reason = i
        .reason
        .as_deref()
        .map(|v| super::text(v, 1000))
        .transpose()?
        .filter(|v| !v.is_empty());
    if i.action == "REJECT" && reason.is_none() {
        return Err(Error::InvalidCode("REASON_REQUIRED"));
    }
    sqlx::query(sql)
        .bind(id)
        .bind(me)
        .bind(&reason)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE work.docs SET version=version+1, updated_at=now(), updated_by=$2 WHERE id=$1",
    )
    .bind(id)
    .bind(me)
    .execute(&mut *tx)
    .await?;
    snapshot(&mut tx, id, "STATUS", Some(&i.action)).await?;
    audit(&mut tx, &st, Some(id), "work.doc_status", &i.action).await?;
    tx.commit().await?;
    detail(s, h, id).await
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

pub async fn versions(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let (r, _) = load(&mut c, &st, id, VIEWER, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('version',version,'title',title,'reason',reason,'label',label,'created_by',created_by,
                'created_at',created_at,'chars',length(content_text))
           FROM work.doc_versions WHERE doc_id=$1 ORDER BY version DESC LIMIT 200",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    Ok(json!({
        "current": {"version": r.get::<i32,_>("version"), "updated_by": r.get::<Uuid,_>("updated_by"), "updated_at": r.get::<DateTime<Utc>,_>("updated_at")},
        "items": items,
    }))
}

pub async fn version(s: &AppState, h: &HeaderMap, id: Uuid, v: i32) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let (r, _) = load(&mut c, &st, id, VIEWER, false).await?;
    if v == r.get::<i32, _>("version") {
        return Ok(
            json!({"version": v, "title": r.get::<String,_>("title"), "content": r.get::<Value,_>("content"),
                         "content_text": r.get::<String,_>("content_text"), "current": true}),
        );
    }
    let row = sqlx::query("SELECT version,title,content,content_text,created_by,created_at,reason FROM work.doc_versions WHERE doc_id=$1 AND version=$2")
        .bind(id)
        .bind(v)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(json!({
        "version": v, "title": row.get::<String,_>("title"), "content": row.get::<Value,_>("content"),
        "content_text": row.get::<String,_>("content_text"), "created_by": row.get::<Uuid,_>("created_by"),
        "created_at": row.get::<DateTime<Utc>,_>("created_at"), "reason": row.get::<String,_>("reason"), "current": false,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionInput {
    #[serde(default)]
    pub label: Option<String>,
}

/// "Save version": keep the current state as a named version.
pub async fn save_version(s: &AppState, h: &HeaderMap, id: Uuid, i: VersionInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let label = i
        .label
        .as_deref()
        .map(|l| line(l, 120))
        .transpose()?
        .filter(|l| !l.is_empty());
    let mut tx = s.pool.begin().await?;
    load(&mut tx, &st, id, EDITOR, true).await?;
    snapshot(&mut tx, id, "MANUAL", label.as_deref()).await?;
    audit(&mut tx, &st, Some(id), "work.doc_version_saved", "MANUAL").await?;
    tx.commit().await?;
    versions(s, h, id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreInput {
    pub version: i32,
    pub base_version: i32,
}

/// Bring back an earlier version as a new version (nothing is lost).
pub async fn restore_version(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    i: RestoreInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (r, _) = load(&mut tx, &st, id, EDITOR, true).await?;
    if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
        return Err(Error::PolicyGate("DOC_ARCHIVED"));
    }
    if r.get::<i32, _>("version") != i.base_version {
        return Err(Error::Conflict);
    }
    let old = sqlx::query(
        "SELECT title, icon, content FROM work.doc_versions WHERE doc_id=$1 AND version=$2",
    )
    .bind(id)
    .bind(i.version)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let content: Value = old.get("content");
    let d = doc_model::validate(&content)?;
    snapshot(&mut tx, id, "AUTOSAVE", None).await?;
    sqlx::query(
        "UPDATE work.docs SET title=$2, icon=$3, content=$4, content_text=$5, updated_by=$6, updated_at=now(), version=version+1,
                status='DRAFT' WHERE id=$1",
    )
    .bind(id)
    .bind(old.get::<String, _>("title"))
    .bind(old.get::<Option<String>, _>("icon"))
    .bind(&content)
    .bind(&d.text)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    sync_links(&mut tx, &st, id, &d.embeds).await?;
    snapshot(&mut tx, id, "RESTORE", Some(&format!("v{}", i.version))).await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.doc_version_restored",
        &format!("v{}", i.version),
    )
    .await?;
    tx.commit().await?;
    detail(s, h, id).await
}

// ---------------------------------------------------------------------------
// Sharing
// ---------------------------------------------------------------------------

pub async fn permissions(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let (r, _) = load(&mut c, &st, id, MANAGER, false).await?;
    let mut grants: Value = Value::Array(
        sqlx::query_scalar(concat!(
            "SELECT jsonb_build_object('principal_kind',p.principal_kind,'principal_id',p.principal_id,'level',p.level,
                'email',(SELECT ",
            crate::email_ref!("u"),
            " FROM identity.users u WHERE p.principal_kind='USER' AND u.id::text=p.principal_id),
                'granted_by',p.granted_by,'granted_at',p.granted_at)
           FROM work.doc_permissions p WHERE p.doc_id=$1 ORDER BY p.principal_kind, p.granted_at"
        ))
        .bind(id)
        .fetch_all(&mut *c)
        .await?,
    );
    crate::pii::reveal(&s.payout_keys, &mut grants)?;
    Ok(
        json!({"owner_id": r.get::<Uuid,_>("created_by"), "staff_access": r.get::<Option<String>,_>("staff_access"), "grants": grants}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub principal_kind: String,
    pub principal_id: String,
    pub level: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionsInput {
    /// null = restricted (owner, ADMIN and the grants only)
    pub staff_access: Option<String>,
    pub grants: Vec<Grant>,
    #[serde(default)]
    pub apply_to_children: bool,
}

pub async fn set_permissions(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
    i: PermissionsInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if let Some(a) = &i.staff_access {
        one_of(a, &["VIEWER", "EDITOR"], "LEVEL_UNKNOWN")?;
    }
    if i.grants.len() > 100 {
        return Err(Error::InvalidCode("TOO_MANY_GRANTS"));
    }
    let mut tx = s.pool.begin().await?;
    load(&mut tx, &st, id, MANAGER, true).await?;
    let mut seen = BTreeSet::new();
    for g in &i.grants {
        one_of(&g.level, &["VIEWER", "EDITOR", "MANAGER"], "LEVEL_UNKNOWN")?;
        match g.principal_kind.as_str() {
            "ROLE" => one_of(
                &g.principal_id,
                &["ADMIN", "REVIEWER", "OPERATOR", "SUPPORT"],
                "ROLE_UNKNOWN",
            )?,
            "USER" => {
                let u = Uuid::parse_str(&g.principal_id)
                    .map_err(|_| Error::InvalidCode("USER_INVALID"))?;
                super::active_staff(&mut tx, Some(u)).await?;
            }
            _ => return Err(Error::InvalidCode("PRINCIPAL_UNKNOWN")),
        }
        if !seen.insert((g.principal_kind.clone(), g.principal_id.clone())) {
            return Err(Error::InvalidCode("GRANT_DUPLICATE"));
        }
    }
    let targets: Vec<Uuid> = if i.apply_to_children {
        sqlx::query_scalar(
            "WITH RECURSIVE down AS (SELECT id FROM work.docs WHERE id=$1 UNION ALL SELECT d.id FROM work.docs d JOIN down ON d.parent_id=down.id)
             SELECT id FROM down",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?
    } else {
        vec![id]
    };
    for t in &targets {
        // children: only those the caller manages
        if *t != id && level(&mut tx, &st, *t).await? < MANAGER {
            continue;
        }
        sqlx::query("UPDATE work.docs SET staff_access=$2 WHERE id=$1")
            .bind(t)
            .bind(&i.staff_access)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM work.doc_permissions WHERE doc_id=$1")
            .bind(t)
            .execute(&mut *tx)
            .await?;
        for g in &i.grants {
            sqlx::query(
                "INSERT INTO work.doc_permissions(doc_id,principal_kind,principal_id,level,granted_by) VALUES($1,$2,$3,$4,$5)",
            )
            .bind(t)
            .bind(&g.principal_kind)
            .bind(&g.principal_id)
            .bind(&g.level)
            .bind(st.actor.user)
            .execute(&mut *tx)
            .await?;
        }
        audit(
            &mut tx,
            &st,
            Some(*t),
            "work.doc_permissions",
            i.staff_access.as_deref().unwrap_or("RESTRICTED"),
        )
        .await?;
    }
    tx.commit().await?;
    permissions(s, h, id).await
}

// ---------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------

pub async fn templates(s: &AppState, h: &HeaderMap) -> Result<Value> {
    staff::staff(s, h, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'key',key,'name',name,'description',description,'icon',icon,'system',created_by IS NULL)
           FROM work.doc_templates WHERE archived_at IS NULL ORDER BY position, created_at",
    )
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({ "items": items }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateInput {
    pub from_doc: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// ADMIN: save a document as a template.
pub async fn save_template(s: &AppState, h: &HeaderMap, i: TemplateInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if !st.role.is_admin() {
        return Err(Error::Forbidden);
    }
    let name = line(&i.name, 80)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    let description = line(&i.description, 300)?;
    let mut tx = s.pool.begin().await?;
    let (r, _) = load(&mut tx, &st, i.from_doc, VIEWER, false).await?;
    let content: Value = r.get("content");
    // images and files stay with their document; a template carries text and structure
    let d = doc_model::validate(&content)?;
    if !d.attachments.is_empty() {
        return Err(Error::InvalidCode("TEMPLATE_HAS_FILES"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.doc_templates(id,name,description,icon,content,created_by,position)
         VALUES($1,$2,$3,$4,$5,$6,(SELECT COALESCE(max(position),0)+1 FROM work.doc_templates))",
    )
    .bind(id)
    .bind(&name)
    .bind(&description)
    .bind(r.get::<Option<String>, _>("icon"))
    .bind(&content)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.doc_template_saved",
        "TEMPLATE",
    )
    .await?;
    tx.commit().await?;
    templates(s, h).await
}

pub async fn archive_template(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    if !st.role.is_admin() {
        return Err(Error::Forbidden);
    }
    let mut tx = s.pool.begin().await?;
    let n = sqlx::query("UPDATE work.doc_templates SET archived_at=now(), archived_by=$2 WHERE id=$1 AND archived_at IS NULL")
        .bind(id)
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
        "work.doc_template_archived",
        "TEMPLATE",
    )
    .await?;
    tx.commit().await?;
    templates(s, h).await
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

async fn h_tree(
    State(s): State<AppState>,
    Query(q): Query<TreeQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(tree(&s, &h, q).await?))
}
async fn h_search(
    State(s): State<AppState>,
    Query(q): Query<SearchQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(search(&s, &h, q).await?))
}
async fn h_create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<CreateInput>,
) -> Result<Json<Value>> {
    Ok(Json(create(&s, &h, i).await?))
}
async fn h_get(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(detail(&s, &h, id).await?))
}
async fn h_save(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<SaveInput>,
) -> Result<Json<Value>> {
    Ok(Json(save(&s, &h, id, i).await?))
}
async fn h_move(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<MoveInput>,
) -> Result<Json<Value>> {
    Ok(Json(move_doc(&s, &h, id, i).await?))
}
async fn h_duplicate(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(duplicate(&s, &h, id).await?))
}
async fn h_archive(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(archive(&s, &h, id).await?))
}
async fn h_restore(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(restore(&s, &h, id).await?))
}
async fn h_delete(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(delete(&s, &h, id).await?))
}
async fn h_status(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<StatusInput>,
) -> Result<Json<Value>> {
    Ok(Json(set_status(&s, &h, id, i).await?))
}
async fn h_versions(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(versions(&s, &h, id).await?))
}
async fn h_save_version(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<VersionInput>,
) -> Result<Json<Value>> {
    Ok(Json(save_version(&s, &h, id, i).await?))
}
async fn h_version(
    State(s): State<AppState>,
    Path((id, v)): Path<(Uuid, i32)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(version(&s, &h, id, v).await?))
}
async fn h_restore_version(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<RestoreInput>,
) -> Result<Json<Value>> {
    Ok(Json(restore_version(&s, &h, id, i).await?))
}
async fn h_permissions(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(permissions(&s, &h, id).await?))
}
async fn h_set_permissions(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<PermissionsInput>,
) -> Result<Json<Value>> {
    Ok(Json(set_permissions(&s, &h, id, i).await?))
}
async fn h_templates(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(templates(&s, &h).await?))
}
async fn h_save_template(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<TemplateInput>,
) -> Result<Json<Value>> {
    Ok(Json(save_template(&s, &h, i).await?))
}
async fn h_archive_template(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(archive_template(&s, &h, id).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/docs", get(h_tree).post(h_create))
        .route("/api/staff/work/docs/search", get(h_search))
        .route("/api/staff/work/docs/{id}", get(h_get).post(h_save))
        .route("/api/staff/work/docs/{id}/move", post(h_move))
        .route("/api/staff/work/docs/{id}/duplicate", post(h_duplicate))
        .route("/api/staff/work/docs/{id}/archive", post(h_archive))
        .route("/api/staff/work/docs/{id}/restore", post(h_restore))
        .route("/api/staff/work/docs/{id}/delete", post(h_delete))
        .route("/api/staff/work/docs/{id}/status", post(h_status))
        .route(
            "/api/staff/work/docs/{id}/versions",
            get(h_versions).post(h_save_version),
        )
        .route("/api/staff/work/docs/{id}/versions/{v}", get(h_version))
        .route(
            "/api/staff/work/docs/{id}/restore-version",
            post(h_restore_version),
        )
        .route(
            "/api/staff/work/docs/{id}/permissions",
            get(h_permissions).post(h_set_permissions),
        )
        .route(
            "/api/staff/work/doc-templates",
            get(h_templates).post(h_save_template),
        )
        .route(
            "/api/staff/work/doc-templates/{id}/archive",
            post(h_archive_template),
        )
}
