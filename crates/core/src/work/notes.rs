//! Internal notes on any AUDENIQ entity (release, artist, DSP, contract
//! document, task …): @mentions, attachments, edit history, resolve and
//! archive. Internal only: no tenant route reads `work.notes`, and mentions
//! never go to `portal.notifications` (which artists can see).
//!
//! Attachments reuse the studio upload path: a presigned PUT into a
//! quarantine key, then completion checks size, type and nonce, freezes the
//! object, hashes it and runs the antivirus scan when it is enabled. Only
//! metadata is kept in `work.attachments`; downloads are `attachment` only.
use super::{Act, active_staff, audit, docs, entities, kanban, line, one_of, permit, tasks, text};
use crate::{
    api::AppState,
    error::{Error, Result},
    staff::{self, Staff},
    upload_safety,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, header},
    response::Response,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

const MAX_ATTACHMENTS: usize = 5;
const MAX_ATTACHMENT_BYTES: i64 = 20 * 1024 * 1024;
/// Files staff attach to notes: documents, spreadsheets, images, archives.
const ATTACHMENT_TYPES: &[&str] = &[
    "application/pdf",
    "image/png",
    "image/jpeg",
    "image/webp",
    "text/plain",
    "text/csv",
    "application/zip",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/vnd.ms-excel",
    "application/msword",
];

async fn notes_json(c: &mut PgConnection, ids: &[Uuid]) -> Result<Vec<Value>> {
    let rows = sqlx::query(
        "SELECT n.id,n.target_kind,n.target_id,n.parent_id,n.anchor,n.author_id,n.body,n.mentions,n.resolved_at,n.resolved_by,n.edited_at,n.created_at,n.archived_at,
                (SELECT count(*) FROM work.note_revisions r WHERE r.note_id=n.id) AS revisions,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('id',a.id,'name',a.name,'size',a.size_bytes,'content_type',a.content_type) ORDER BY a.created_at)
                            FROM work.attachments a WHERE a.note_id=n.id AND a.archived_at IS NULL),'[]') AS attachments
           FROM work.notes n WHERE n.id = ANY($1) ORDER BY n.created_at",
    )
    .bind(ids)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let resolved_at: Option<DateTime<Utc>> = r.get("resolved_at");
            json!({
                "id": r.get::<Uuid,_>("id"), "target_kind": r.get::<String,_>("target_kind"), "target_id": r.get::<String,_>("target_id"),
                "parent_id": r.get::<Option<Uuid>,_>("parent_id"), "anchor": r.get::<Option<Value>,_>("anchor"),
                "author_id": r.get::<Uuid,_>("author_id"), "body": r.get::<String,_>("body"), "mentions": r.get::<Vec<Uuid>,_>("mentions"),
                "attachments": r.get::<Value,_>("attachments"), "resolved": resolved_at.is_some(), "resolved_at": resolved_at,
                "resolved_by": r.get::<Option<Uuid>,_>("resolved_by"), "edited_at": r.get::<Option<DateTime<Utc>>,_>("edited_at"),
                "revisions": r.get::<i64,_>("revisions"), "created_at": r.get::<DateTime<Utc>,_>("created_at"),
                "archived_at": r.get::<Option<DateTime<Utc>>,_>("archived_at"),
            })
        })
        .collect())
}

async fn mentions_ok(c: &mut PgConnection, m: &[Uuid]) -> Result<Vec<Uuid>> {
    if m.len() > 20 {
        return Err(Error::InvalidCode("TOO_MANY_MENTIONS"));
    }
    let mut v = m.to_vec();
    v.sort();
    v.dedup();
    for u in &v {
        active_staff(c, Some(*u)).await?;
    }
    Ok(v)
}

/// Notes inherit their target's access: a document's comments need the
/// document (hidden ones are a 404), a card's notes need the card.
pub(crate) async fn target_gate(
    c: &mut PgConnection,
    st: &Staff,
    kind: &str,
    id: &str,
) -> Result<()> {
    match kind {
        "doc" => {
            let id = Uuid::parse_str(id).map_err(|_| Error::NotFound)?;
            if docs::level(c, st, id).await? < docs::VIEWER {
                return Err(Error::NotFound);
            }
        }
        "task" => {
            let id = Uuid::parse_str(id).map_err(|_| Error::NotFound)?;
            let t = tasks::load(c, id, false).await?;
            if !kanban::card_visible(c, st, &t).await? {
                return Err(Error::NotFound);
            }
        }
        _ => {}
    }
    Ok(())
}

/// The target of an existing note, checked with `target_gate`.
async fn note_gate(c: &mut PgConnection, st: &Staff, note: Uuid) -> Result<()> {
    let r = sqlx::query("SELECT target_kind, target_id FROM work.notes WHERE id=$1")
        .bind(note)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    target_gate(
        c,
        st,
        &r.get::<String, _>("target_kind"),
        &r.get::<String, _>("target_id"),
    )
    .await
}

/// A comment anchor: which block / text range of a document it is about.
/// Only a small, flat object of ids and offsets is kept.
fn anchor_of(v: Option<Value>) -> Result<Option<Value>> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let o = v
        .as_object()
        .ok_or(Error::InvalidCode("NOTE_ANCHOR_INVALID"))?;
    let mut out = serde_json::Map::new();
    for (k, val) in o {
        match (k.as_str(), val) {
            ("block", Value::String(b))
                if b.len() <= 64
                    && b.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
            {
                out.insert(k.clone(), val.clone());
            }
            ("from" | "to", Value::Number(n)) if n.as_u64().is_some_and(|n| n <= 10_000_000) => {
                out.insert(k.clone(), val.clone());
            }
            ("quote", Value::String(q)) => {
                out.insert(k.clone(), Value::String(q.chars().take(200).collect()));
            }
            _ => return Err(Error::InvalidCode("NOTE_ANCHOR_INVALID")),
        }
    }
    Ok(if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub target_kind: String,
    pub target_id: String,
    #[serde(default)]
    pub archived: bool,
    /// only the newest `latest` notes (chat), optionally older than `before`
    pub latest: Option<i64>,
    pub before: Option<DateTime<Utc>>,
}

pub async fn list(s: &AppState, h: &HeaderMap, q: ListQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let target = entities::parse(&q.target_kind, &q.target_id)?;
    let mut c = s.pool.acquire().await?;
    target_gate(&mut c, &st, &target.kind, &target.id).await?;
    if let Some(n) = q.latest {
        // newest first, then shown oldest-first; `has_more` pages further back
        let n = n.clamp(1, 200);
        let mut ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM work.notes WHERE target_kind=$1 AND target_id=$2 AND ($3 OR archived_at IS NULL)
               AND ($4::timestamptz IS NULL OR created_at < $4) ORDER BY created_at DESC, id DESC LIMIT $5",
        )
        .bind(&target.kind)
        .bind(&target.id)
        .bind(q.archived)
        .bind(q.before)
        .bind(n + 1)
        .fetch_all(&mut *c)
        .await?;
        let has_more = ids.len() as i64 > n;
        ids.truncate(n as usize);
        return Ok(json!({ "items": notes_json(&mut c, &ids).await?, "has_more": has_more }));
    }
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM work.notes WHERE target_kind=$1 AND target_id=$2 AND ($3 OR archived_at IS NULL) ORDER BY created_at LIMIT 500",
    )
    .bind(&target.kind)
    .bind(&target.id)
    .bind(q.archived)
    .fetch_all(&mut *c)
    .await?;
    Ok(json!({ "items": notes_json(&mut c, &ids).await? }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub target_kind: String,
    pub target_id: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub mentions: Vec<Uuid>,
    #[serde(default)]
    pub attachments: Vec<Uuid>,
    /// reply to this note (same target; one level of threads)
    pub parent_id: Option<Uuid>,
    /// part of a document the comment is about
    pub anchor: Option<Value>,
}

pub async fn create(s: &AppState, h: &HeaderMap, i: CreateInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let target = entities::parse(&i.target_kind, &i.target_id)?;
    let body = text(&i.body, 4000)?;
    if body.is_empty() && i.attachments.is_empty() {
        return Err(Error::InvalidCode("NOTE_EMPTY"));
    }
    if i.attachments.len() > MAX_ATTACHMENTS {
        return Err(Error::InvalidCode("TOO_MANY_ATTACHMENTS"));
    }
    let mut tx = s.pool.begin().await?;
    entities::require(&mut tx, &target).await?;
    target_gate(&mut tx, &st, &target.kind, &target.id).await?;
    let anchor = anchor_of(i.anchor)?;
    let parent = match i.parent_id {
        None => None,
        Some(p) => {
            // replies hang off a top-level note of the same target
            let ok: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM work.notes WHERE id=$1 AND target_kind=$2 AND target_id=$3 AND parent_id IS NULL AND archived_at IS NULL)",
            )
            .bind(p)
            .bind(&target.kind)
            .bind(&target.id)
            .fetch_one(&mut *tx)
            .await?;
            if !ok {
                return Err(Error::InvalidCode("NOTE_PARENT_INVALID"));
            }
            Some(p)
        }
    };
    if target.kind == "channel" {
        super::channels::require_open(&mut tx, &target.id).await?;
    }
    let mentions = mentions_ok(&mut tx, &i.mentions).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO work.notes(id,target_kind,target_id,author_id,body,mentions,parent_id,anchor) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
        .bind(id)
        .bind(&target.kind)
        .bind(&target.id)
        .bind(st.actor.user)
        .bind(&body)
        .bind(&mentions)
        .bind(parent)
        .bind(anchor)
        .execute(&mut *tx)
        .await?;
    for a in &i.attachments {
        // only my own finished, unattached upload
        let n = sqlx::query(
            "UPDATE work.attachments SET note_id=$1 WHERE id=$2 AND uploaded_by=$3 AND state='READY' AND note_id IS NULL AND archived_at IS NULL",
        )
        .bind(id)
        .bind(a)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await?;
        if n.rows_affected() != 1 {
            return Err(Error::InvalidCode("ATTACHMENT_NOT_READY"));
        }
    }
    if target.kind == "task" {
        let task = Uuid::parse_str(&target.id).map_err(|_| Error::Invalid)?;
        tasks::event(&mut tx, task, Some(st.actor.user), "NOTE", None).await?;
    }
    audit(&mut tx, &st, Some(id), "work.note_added", &target.kind).await?;
    let out = notes_json(&mut tx, &[id]).await?;
    tx.commit().await?;
    out.into_iter().next().ok_or(Error::Internal)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    pub body: String,
    #[serde(default)]
    pub mentions: Vec<Uuid>,
}

/// Author-only edit; the previous text is kept in `work.note_revisions`.
pub async fn edit(s: &AppState, h: &HeaderMap, id: Uuid, i: EditInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let body = text(&i.body, 4000)?;
    let mut tx = s.pool.begin().await?;
    note_gate(&mut tx, &st, id).await?;
    let r = sqlx::query(
        "SELECT author_id, body, mentions, archived_at FROM work.notes WHERE id=$1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    permit(
        &st,
        Act::EditNote {
            author: r.get("author_id"),
        },
    )?;
    if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
        return Err(Error::PolicyGate("NOTE_ARCHIVED"));
    }
    let attached: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.attachments WHERE note_id=$1 AND archived_at IS NULL",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if body.is_empty() && attached == 0 {
        return Err(Error::InvalidCode("NOTE_EMPTY"));
    }
    let mentions = mentions_ok(&mut tx, &i.mentions).await?;
    let old: String = r.get("body");
    let old_mentions: Vec<Uuid> = r.get("mentions");
    if old != body || old_mentions != mentions {
        sqlx::query("INSERT INTO work.note_revisions(id,note_id,body,mentions,edited_by) VALUES($1,$2,$3,$4,$5)")
            .bind(Uuid::new_v4())
            .bind(id)
            .bind(&old)
            .bind(&old_mentions)
            .bind(st.actor.user)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE work.notes SET body=$2, mentions=$3, edited_at=now() WHERE id=$1")
            .bind(id)
            .bind(&body)
            .bind(&mentions)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, &st, Some(id), "work.note_edited", "EDIT").await?;
    }
    let out = notes_json(&mut tx, &[id]).await?;
    tx.commit().await?;
    out.into_iter().next().ok_or(Error::Internal)
}

pub async fn revisions(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    note_gate(&mut *s.pool.acquire().await?, &st, id).await?;
    let rows = sqlx::query("SELECT body, mentions, edited_by, edited_at FROM work.note_revisions WHERE note_id=$1 ORDER BY edited_at DESC")
        .bind(id)
        .fetch_all(&s.pool)
        .await?;
    Ok(json!({ "items": rows.iter().map(|r| json!({
        "body": r.get::<String,_>("body"), "mentions": r.get::<Vec<Uuid>,_>("mentions"),
        "edited_by": r.get::<Uuid,_>("edited_by"), "edited_at": r.get::<DateTime<Utc>,_>("edited_at"),
    })).collect::<Vec<_>>() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveInput {
    pub resolved: bool,
}

pub async fn resolve(s: &AppState, h: &HeaderMap, id: Uuid, i: ResolveInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    note_gate(&mut tx, &st, id).await?;
    let n = sqlx::query(
        "UPDATE work.notes SET resolved_at=CASE WHEN $2 THEN now() END, resolved_by=CASE WHEN $2 THEN $3 END
          WHERE id=$1 AND archived_at IS NULL AND (resolved_at IS NULL) = $2",
    )
    .bind(id)
    .bind(i.resolved)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    if n.rows_affected() == 1 {
        audit(
            &mut tx,
            &st,
            Some(id),
            if i.resolved {
                "work.note_resolved"
            } else {
                "work.note_reopened"
            },
            "RESOLVE",
        )
        .await?;
    } else {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM work.notes WHERE id=$1 AND archived_at IS NULL)",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(Error::NotFound);
        }
    }
    let out = notes_json(&mut tx, &[id]).await?;
    tx.commit().await?;
    out.into_iter().next().ok_or(Error::Internal)
}

/// Archive a note (and its attachments). Author or ADMIN. Never deleted.
pub async fn archive(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    note_gate(&mut tx, &st, id).await?;
    let r = sqlx::query("SELECT author_id, archived_at FROM work.notes WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    permit(
        &st,
        Act::ArchiveNote {
            author: r.get("author_id"),
        },
    )?;
    if r.get::<Option<DateTime<Utc>>, _>("archived_at").is_some() {
        return Err(Error::Conflict);
    }
    sqlx::query("UPDATE work.notes SET archived_at=now(), archived_by=$2 WHERE id=$1")
        .bind(id)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE work.attachments SET archived_at=now(), archived_by=$2 WHERE note_id=$1 AND archived_at IS NULL")
        .bind(id)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, &st, Some(id), "work.note_archived", "ARCHIVE").await?;
    tx.commit().await?;
    Ok(json!({ "id": id, "archived": true }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MentionQuery {
    #[serde(default)]
    pub resolved: bool,
}

/// Notes that mention me (open ones by default), with what they are about.
pub async fn mentions(s: &AppState, h: &HeaderMap, q: MentionQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let mut c = s.pool.acquire().await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM work.notes WHERE $1 = ANY(mentions) AND archived_at IS NULL AND ($2 OR resolved_at IS NULL)
            AND (target_kind<>'doc' OR work.doc_level(target_id::uuid,$1,$3) > 0)
          ORDER BY created_at DESC LIMIT 200",
    )
    .bind(st.actor.user)
    .bind(q.resolved)
    .bind(st.role.team())
    .fetch_all(&mut *c)
    .await?;
    let mut items = notes_json(&mut c, &ids).await?;
    items.reverse();
    let refs: Vec<entities::Ref> = items
        .iter()
        .map(|n| entities::Ref {
            kind: n["target_kind"].as_str().unwrap_or("").into(),
            id: n["target_id"].as_str().unwrap_or("").into(),
        })
        .collect();
    let labels = entities::resolve(&mut c, &refs).await?;
    for (n, r) in items.iter_mut().zip(refs.iter()) {
        n["target"] = labels.get(r).cloned().unwrap_or(Value::Null);
    }
    Ok(json!({ "items": items }))
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachmentInput {
    pub name: String,
    pub content_type: String,
    pub size_bytes: i64,
}

pub async fn issue_attachment(s: &AppState, h: &HeaderMap, i: AttachmentInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let name = line(&i.name, 200)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("ATTACHMENT_NAME_REQUIRED"));
    }
    one_of(
        &i.content_type,
        ATTACHMENT_TYPES,
        "ATTACHMENT_TYPE_UNSUPPORTED",
    )?;
    if !(1..=MAX_ATTACHMENT_BYTES).contains(&i.size_bytes) {
        return Err(Error::InvalidCode("ATTACHMENT_TOO_LARGE"));
    }
    crate::auth::rate(&s.pool, &format!("work-attachments:{}", st.actor.user), 60).await?;
    let id = Uuid::new_v4();
    let nonce = Uuid::new_v4();
    let upload_key = format!("quarantine/work/{id}/{nonce}");
    let object_key = format!("work/attachments/{id}/{}", Uuid::new_v4());
    let mut tx = s.pool.begin().await?;
    let expires: DateTime<Utc> = sqlx::query_scalar("SELECT now()+interval '10 minutes'")
        .fetch_one(&mut *tx)
        .await?;
    let grant = s
        .storage
        .presign_put(
            &upload_key,
            i.size_bytes,
            &i.content_type,
            &nonce.to_string(),
            expires,
        )
        .await?;
    sqlx::query(
        "INSERT INTO work.attachments(id,name,content_type,size_bytes,upload_key,nonce,object_key,uploaded_by,expires_at)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id).bind(&name).bind(&i.content_type).bind(i.size_bytes).bind(&upload_key).bind(nonce).bind(&object_key)
    .bind(st.actor.user).bind(expires)
    .execute(&mut *tx)
    .await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.attachment_issued",
        "PRIVATE_QUARANTINE",
    )
    .await?;
    tx.commit().await?;
    Ok(json!({ "id": id, "grant": grant }))
}

/// Verify the uploaded object, freeze it out of quarantine, hash and scan it.
pub async fn complete_attachment(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let r = sqlx::query(
        "SELECT name, content_type, size_bytes, upload_key, nonce, object_key, state, (expires_at > now()) AS valid
           FROM work.attachments WHERE id=$1 AND uploaded_by=$2 AND archived_at IS NULL",
    )
    .bind(id)
    .bind(st.actor.user)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    if r.get::<String, _>("state") == "READY" {
        return Ok(json!({ "id": id, "state": "READY" }));
    }
    if !r.get::<bool, _>("valid") {
        return Err(Error::PolicyGate("ATTACHMENT_EXPIRED"));
    }
    let size: i64 = r.get("size_bytes");
    let mime: String = r.get("content_type");
    let upload_key: String = r.get("upload_key");
    let object_key: String = r.get("object_key");
    let nonce: Uuid = r.get("nonce");
    let meta = s
        .storage
        .head(&upload_key)
        .await?
        .ok_or(Error::PolicyGate("ATTACHMENT_NOT_UPLOADED"))?;
    if meta.size != size || meta.content_type != mime || meta.nonce != nonce.to_string() {
        return Err(Error::Conflict);
    }
    let _slot = s
        .upload_slots
        .try_acquire()
        .map_err(|_| Error::UploadBusy)?;
    s.storage
        .freeze(&upload_key, &object_key, &meta.etag)
        .await?;
    let path = std::env::temp_dir().join(format!("audeniq-work-{id}"));
    let digest = s.storage.download_to(&object_key, &path, size as u64).await;
    let scanned = match digest {
        Ok(d) if d.size == size as u64 => {
            upload_safety::scan(&path, size as u64, s.antivirus_enabled)
                .await
                .map(|_| d)
        }
        Ok(_) => Err(Error::Conflict),
        Err(e) => Err(e),
    };
    let _ = tokio::fs::remove_file(&path).await;
    let d = match scanned {
        Ok(d) => d,
        Err(e) => {
            let _ = s.storage.delete(&object_key).await;
            return Err(e);
        }
    };
    let _ = s.storage.delete(&upload_key).await;
    let mut tx = s.pool.begin().await?;
    sqlx::query("UPDATE work.attachments SET state='READY', sha256=$2, antivirus_status=$3 WHERE id=$1 AND state='ISSUED'")
        .bind(id)
        .bind(&d.sha256)
        .bind(if s.antivirus_enabled { "SCANNED" } else { "NOT_SCANNED" })
        .execute(&mut *tx)
        .await?;
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.attachment_ready",
        if s.antivirus_enabled {
            "SCANNED"
        } else {
            "NOT_SCANNED"
        },
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({ "id": id, "state": "READY", "name": r.get::<String,_>("name"), "size": size, "content_type": mime }),
    )
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DownloadQuery {
    /// show a PNG / JPEG / WebP image in the page (document image blocks)
    #[serde(default)]
    pub inline: bool,
}

const INLINE_IMAGES: &[&str] = &["image/png", "image/jpeg", "image/webp"];

/// Download, gated by what the file belongs to: a document's files follow the
/// document's access, a note's files its target's; an unattached upload only
/// its uploader. Only raster images are ever shown inline (sandboxed, no
/// sniffing); everything else is `attachment`. Audited; archived files are
/// not served.
pub async fn download(s: &AppState, h: &HeaderMap, id: Uuid, q: DownloadQuery) -> Result<Response> {
    let st = staff::staff(s, h, false).await?;
    let mut tx = s.pool.begin().await?;
    let r = sqlx::query(
        "SELECT a.name, a.object_key, a.content_type, a.doc_id, a.uploaded_by, n.target_kind, n.target_id
           FROM work.attachments a LEFT JOIN work.notes n ON n.id=a.note_id
          WHERE a.id=$1 AND a.state='READY' AND a.archived_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let doc: Option<Uuid> = r.get("doc_id");
    let note_target: Option<(String, String)> = r
        .get::<Option<String>, _>("target_kind")
        .zip(r.get::<Option<String>, _>("target_id"));
    match (doc, note_target) {
        (Some(d), _) => target_gate(&mut tx, &st, "doc", &d.to_string()).await?,
        (None, Some((k, t))) => target_gate(&mut tx, &st, &k, &t).await?,
        (None, None) => {
            if r.get::<Uuid, _>("uploaded_by") != st.actor.user {
                return Err(Error::NotFound);
            }
        }
    }
    let mime: String = r.get("content_type");
    let inline = q.inline && INLINE_IMAGES.contains(&mime.as_str());
    audit(
        &mut tx,
        &st,
        Some(id),
        "work.attachment_viewed",
        if inline { "VIEW" } else { "DOWNLOAD" },
    )
    .await?;
    tx.commit().await?;
    let bytes = s.storage.get(&r.get::<String, _>("object_key")).await?;
    let name: String = r.get("name");
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
    let mut res = Response::new(Body::from(bytes));
    let hs = res.headers_mut();
    if inline {
        if let Ok(v) = HeaderValue::from_str(&mime) {
            hs.insert(header::CONTENT_TYPE, v);
        }
        hs.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; sandbox"),
        );
        hs.insert(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("inline"),
        );
        hs.insert(
            "x-content-type-options",
            HeaderValue::from_static("nosniff"),
        );
        hs.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, max-age=300"),
        );
        return Ok(res);
    }
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
async fn h_edit(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<EditInput>,
) -> Result<Json<Value>> {
    Ok(Json(edit(&s, &h, id, i).await?))
}
async fn h_revisions(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(revisions(&s, &h, id).await?))
}
async fn h_resolve(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ResolveInput>,
) -> Result<Json<Value>> {
    Ok(Json(resolve(&s, &h, id, i).await?))
}
async fn h_archive(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(archive(&s, &h, id).await?))
}
async fn h_mentions(
    State(s): State<AppState>,
    Query(q): Query<MentionQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(mentions(&s, &h, q).await?))
}
async fn h_issue(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<AttachmentInput>,
) -> Result<Json<Value>> {
    Ok(Json(issue_attachment(&s, &h, i).await?))
}
async fn h_complete(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(complete_attachment(&s, &h, id).await?))
}
async fn h_download(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(q): Query<DownloadQuery>,
    h: HeaderMap,
) -> Result<Response> {
    download(&s, &h, id, q).await
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/notes", get(h_list).post(h_create))
        .route("/api/staff/work/notes/{id}", post(h_edit))
        .route("/api/staff/work/notes/{id}/revisions", get(h_revisions))
        .route("/api/staff/work/notes/{id}/resolve", post(h_resolve))
        .route("/api/staff/work/notes/{id}/archive", post(h_archive))
        .route("/api/staff/work/mentions", get(h_mentions))
        .route("/api/staff/work/attachments", post(h_issue))
        .route(
            "/api/staff/work/attachments/{id}/complete",
            post(h_complete),
        )
        .route("/api/staff/work/attachments/{id}", get(h_download))
}
