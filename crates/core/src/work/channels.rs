//! Staff channels (team conversation). A channel is only a name and a topic;
//! its messages are internal notes (`work.notes`, `target_kind = 'channel'`),
//! so @mentions reach the inbox, attachments go through the object store and
//! antivirus scan, edits keep their history and nothing is deleted. Each
//! person's read position (`work.channel_reads`) gives the unread counts.
//!
//! Every staff role reads and writes every channel. Creating is open to all
//! staff; renaming and archiving to the creator or ADMIN. Audited as
//! `work.channel_*`.
use super::{Act, audit, line, permit};
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
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default)]
    pub archived: bool,
}

/// Channels with the caller's unread and mention counts and the last message.
pub async fn list(s: &AppState, h: &HeaderMap, q: ListQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object(
            'id', c.id, 'name', c.name, 'topic', c.topic, 'created_by', c.created_by, 'created_at', c.created_at,
            'archived_at', c.archived_at,
            'last_read_at', r.last_read_at,
            'last_message_at', (SELECT max(n.created_at) FROM work.notes n WHERE n.target_kind='channel' AND n.target_id=c.id::text AND n.archived_at IS NULL),
            'unread', (SELECT count(*) FROM work.notes n WHERE n.target_kind='channel' AND n.target_id=c.id::text AND n.archived_at IS NULL
                         AND n.author_id<>$1 AND n.created_at > COALESCE(r.last_read_at, '-infinity')),
            'mentions', (SELECT count(*) FROM work.notes n WHERE n.target_kind='channel' AND n.target_id=c.id::text AND n.archived_at IS NULL
                         AND n.author_id<>$1 AND $1 = ANY(n.mentions) AND n.created_at > COALESCE(r.last_read_at, '-infinity')))
           FROM work.channels c
           LEFT JOIN work.channel_reads r ON r.channel_id=c.id AND r.user_id=$1
          WHERE ($2 OR c.archived_at IS NULL) AND (NOT $2 OR c.archived_at IS NOT NULL)
          ORDER BY c.created_by NULLS FIRST, c.created_at",
    )
    .bind(st.actor.user)
    .bind(q.archived)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({ "items": items }))
}

/// Total unread messages across channels (navigation badge).
pub(crate) async fn unread_total(c: &mut PgConnection, me: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM work.notes n
           JOIN work.channels c ON c.id::text=n.target_id AND c.archived_at IS NULL
           LEFT JOIN work.channel_reads r ON r.channel_id=c.id AND r.user_id=$1
          WHERE n.target_kind='channel' AND n.archived_at IS NULL AND n.author_id<>$1
            AND n.created_at > COALESCE(r.last_read_at, '-infinity')",
    )
    .bind(me)
    .fetch_one(&mut *c)
    .await?)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelInput {
    pub name: String,
    #[serde(default)]
    pub topic: String,
}

fn fields(i: &ChannelInput) -> Result<(String, String)> {
    let name = line(i.name.trim_start_matches('#'), 40)?;
    if name.is_empty() {
        return Err(Error::InvalidCode("NAME_REQUIRED"));
    }
    Ok((name, line(&i.topic, 200)?))
}

fn dup(e: sqlx::Error) -> Error {
    if let sqlx::Error::Database(d) = &e
        && d.code().as_deref() == Some("23505")
    {
        return Error::InvalidCode("CHANNEL_EXISTS");
    }
    e.into()
}

async fn one(c: &mut PgConnection, id: Uuid) -> Result<Value> {
    sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'name',name,'topic',topic,'created_by',created_by,'created_at',created_at,'archived_at',archived_at)
           FROM work.channels WHERE id=$1",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

pub async fn create(s: &AppState, h: &HeaderMap, i: ChannelInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let (name, topic) = fields(&i)?;
    let mut tx = s.pool.begin().await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM work.channels WHERE archived_at IS NULL")
        .fetch_one(&mut *tx)
        .await?;
    if n >= 100 {
        return Err(Error::InvalidCode("TOO_MANY_CHANNELS"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO work.channels(id,name,topic,created_by) VALUES($1,$2,$3,$4)")
        .bind(id)
        .bind(&name)
        .bind(&topic)
        .bind(st.actor.user)
        .execute(&mut *tx)
        .await
        .map_err(dup)?;
    audit(&mut tx, &st, Some(id), "work.channel_created", "CHANNEL").await?;
    let v = one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

async fn locked(c: &mut PgConnection, id: Uuid) -> Result<(Option<Uuid>, Option<DateTime<Utc>>)> {
    let r = sqlx::query("SELECT created_by, archived_at FROM work.channels WHERE id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    Ok((r.get("created_by"), r.get("archived_at")))
}

pub async fn update(s: &AppState, h: &HeaderMap, id: Uuid, i: ChannelInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let (name, topic) = fields(&i)?;
    let mut tx = s.pool.begin().await?;
    let (created_by, archived) = locked(&mut tx, id).await?;
    permit(&st, Act::ManageChannel { created_by })?;
    if archived.is_some() {
        return Err(Error::PolicyGate("CHANNEL_ARCHIVED"));
    }
    sqlx::query("UPDATE work.channels SET name=$2, topic=$3, updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(&name)
        .bind(&topic)
        .execute(&mut *tx)
        .await
        .map_err(dup)?;
    audit(&mut tx, &st, Some(id), "work.channel_updated", "CHANNEL").await?;
    let v = one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

pub async fn archive(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let (created_by, archived) = locked(&mut tx, id).await?;
    permit(&st, Act::ManageChannel { created_by })?;
    if archived.is_some() {
        return Err(Error::Conflict);
    }
    sqlx::query(
        "UPDATE work.channels SET archived_at=now(), archived_by=$2, updated_at=now() WHERE id=$1",
    )
    .bind(id)
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, &st, Some(id), "work.channel_archived", "CHANNEL").await?;
    let v = one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(v)
}

/// Mark the channel read up to now.
pub async fn read(s: &AppState, h: &HeaderMap, id: Uuid) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    let n = sqlx::query(
        "INSERT INTO work.channel_reads(user_id, channel_id, last_read_at)
         SELECT $1, c.id, now() FROM work.channels c WHERE c.id=$2
         ON CONFLICT (user_id, channel_id) DO UPDATE SET last_read_at = GREATEST(work.channel_reads.last_read_at, EXCLUDED.last_read_at)",
    )
    .bind(st.actor.user)
    .bind(id)
    .execute(&s.pool)
    .await?;
    if n.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    Ok(json!({"read": id}))
}

/// Messages may only be posted to an open channel.
pub(crate) async fn require_open(c: &mut PgConnection, id: &str) -> Result<()> {
    let archived: Option<Option<DateTime<Utc>>> =
        sqlx::query_scalar("SELECT archived_at FROM work.channels WHERE id::text=$1")
            .bind(id)
            .fetch_optional(&mut *c)
            .await?;
    match archived {
        None => Err(Error::InvalidCode("ENTITY_NOT_FOUND")),
        Some(Some(_)) => Err(Error::PolicyGate("CHANNEL_ARCHIVED")),
        Some(None) => Ok(()),
    }
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
    Json(i): Json<ChannelInput>,
) -> Result<Json<Value>> {
    Ok(Json(create(&s, &h, i).await?))
}
async fn h_update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ChannelInput>,
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
async fn h_read(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(read(&s, &h, id).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/work/channels", get(h_list).post(h_create))
        .route("/api/staff/work/channels/{id}", post(h_update))
        .route("/api/staff/work/channels/{id}/archive", post(h_archive))
        .route("/api/staff/work/channels/{id}/read", post(h_read))
}
