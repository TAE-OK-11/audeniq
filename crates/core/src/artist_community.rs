//! Artist page news, fan cheers and discography (migration 0154).
//!
//! On their page ([`crate::artist_links`]) the artist posts notices and
//! events: a title, text, for an event its date and place, an optional https
//! link (tickets) and a pin to keep it on top; up to [`POSTS_MAX`] per page.
//! The page shows the pinned posts and the newest ones; `/news` lists all.
//!
//! Fans leave cheers without an account: a nickname and a short message,
//! limited per visitor address, per page and overall. The artist can hide or
//! delete each one, mark it with a heart and switch cheers off for the page
//! (existing cheers are then not shown and new ones are refused).
//!
//! The discography lists the same releases as the page (those with a visible
//! smart link) with their tracks, grouped by year on the page.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    smart_links::{self, HIDDEN_STATES},
    text_policy,
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, put},
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

pub const POSTS_MAX: i64 = 100;
const TITLE_MAX: usize = 80;
const BODY_MAX: usize = 2000;
const PLACE_MAX: usize = 80;
const URL_MAX: usize = 500;
const NICK_MAX: usize = 20;
const CHEER_MAX: usize = 300;
const CHEERS_PAGE: i64 = 30;
/// Posts shown on the page itself (pinned first, then the newest).
pub const PREVIEW: i64 = 3;

/// The JSON of a post, from a row of `portal.artist_posts` aliased `p`.
const POST_JSON: &str =
    "jsonb_build_object('id', p.id, 'kind', p.kind, 'title', p.title, 'body', p.body,
    'starts_at', p.starts_at, 'place', p.place, 'url', p.url, 'pinned', p.pinned,
    'created_at', p.created_at, 'updated_at', p.updated_at)";
/// Pinned first, then events still to come (soonest first), then the newest.
const POST_ORDER: &str = "p.pinned DESC, (p.kind='EVENT' AND p.starts_at >= now()) DESC,
    CASE WHEN p.kind='EVENT' AND p.starts_at >= now() THEN p.starts_at END ASC, p.created_at DESC";

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostInput {
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// When the event starts; required for an event, ignored for a notice.
    #[serde(default)]
    pub starts_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub place: String,
    /// Tickets or details, https; empty for none.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub pinned: bool,
}

struct Post {
    kind: &'static str,
    title: String,
    body: String,
    starts_at: Option<DateTime<Utc>>,
    place: String,
    url: String,
    pinned: bool,
}

fn clean_post(i: &PostInput) -> Result<Post> {
    let kind = match i.kind.as_str() {
        "NOTICE" => "NOTICE",
        "EVENT" => "EVENT",
        _ => return Err(Error::InvalidCode("ARTIST_POST_KIND_INVALID")),
    };
    let title = i.title.trim();
    if title.is_empty() || title.chars().count() > TITLE_MAX {
        return Err(Error::InvalidCode("ARTIST_POST_TITLE_INVALID"));
    }
    text_policy::check(title)?;
    let body = i.body.trim();
    if body.chars().count() > BODY_MAX {
        return Err(Error::InvalidCode("ARTIST_POST_BODY_TOO_LONG"));
    }
    if !body.is_empty() {
        text_policy::check_multiline(body)?;
    }
    let place = i.place.trim();
    if place.chars().count() > PLACE_MAX {
        return Err(Error::InvalidCode("ARTIST_POST_PLACE_INVALID"));
    }
    if !place.is_empty() {
        text_policy::check(place)?;
    }
    let url = i.url.trim();
    let url = if url.is_empty() {
        String::new()
    } else {
        let parsed =
            url::Url::parse(url).map_err(|_| Error::InvalidCode("ARTIST_LINK_URL_INVALID"))?;
        if parsed.scheme() != "https" || parsed.host_str().is_none() || url.len() > URL_MAX {
            return Err(Error::InvalidCode("ARTIST_LINK_URL_INVALID"));
        }
        parsed.to_string()
    };
    let starts_at = match kind {
        "EVENT" => Some(
            i.starts_at
                .ok_or(Error::InvalidCode("ARTIST_POST_DATE_REQUIRED"))?,
        ),
        _ => None,
    };
    Ok(Post {
        kind,
        title: title.to_string(),
        body: body.to_string(),
        starts_at,
        place: if kind == "EVENT" {
            place.to_string()
        } else {
            String::new()
        },
        url,
        pinned: i.pinned,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheerInput {
    pub nickname: String,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheerPatch {
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub heart: Option<bool>,
}

#[derive(Deserialize)]
pub struct PageQuery {
    /// The last cheer of the previous page; its older neighbours come next.
    #[serde(default)]
    pub before: Option<Uuid>,
}

// ---------------------------------------------------------------------------
// Org
// ---------------------------------------------------------------------------

/// The page of the org's artist, locked for writes.
async fn org_link(c: &mut PgConnection, org: Uuid, artist: Uuid, write: bool) -> Result<Uuid> {
    let sql = if write {
        "SELECT k.id FROM portal.artist_links k JOIN catalog.artists a ON a.org_id=k.org_id AND a.id=k.artist_id
          WHERE k.org_id=$1 AND k.artist_id=$2 AND a.archived_at IS NULL FOR UPDATE OF k"
    } else {
        "SELECT k.id FROM portal.artist_links k JOIN catalog.artists a ON a.org_id=k.org_id AND a.id=k.artist_id
          WHERE k.org_id=$1 AND k.artist_id=$2 AND a.archived_at IS NULL"
    };
    sqlx::query_scalar(sql)
        .bind(org)
        .bind(artist)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::InvalidCode("ARTIST_LINK_REQUIRED"))
}

pub async fn list_posts(s: &AppState, a: &Actor, org: Uuid, artist: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let link = org_link(&mut tx, org, artist, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {POST_JSON} FROM portal.artist_posts p WHERE p.link_id=$1 ORDER BY {POST_ORDER}"
    )))
    .bind(link)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items, "max": POSTS_MAX}))
}

pub async fn save_post(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    post: Option<Uuid>,
    i: PostInput,
) -> Result<Value> {
    let p = clean_post(&i)?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = org_link(&mut tx, org, artist, true).await?;
    let id = match post {
        Some(id) => {
            let done = sqlx::query(
                "UPDATE portal.artist_posts SET kind=$3, title=$4, body=$5, starts_at=$6, place=$7, url=$8, pinned=$9, updated_at=now()
                  WHERE link_id=$1 AND id=$2",
            )
            .bind(link)
            .bind(id)
            .bind(p.kind)
            .bind(&p.title)
            .bind(&p.body)
            .bind(p.starts_at)
            .bind(&p.place)
            .bind(&p.url)
            .bind(p.pinned)
            .execute(&mut *tx)
            .await?;
            if done.rows_affected() == 0 {
                return Err(Error::NotFound);
            }
            id
        }
        None => {
            let n: i64 =
                sqlx::query_scalar("SELECT count(*) FROM portal.artist_posts WHERE link_id=$1")
                    .bind(link)
                    .fetch_one(&mut *tx)
                    .await?;
            if n >= POSTS_MAX {
                return Err(Error::InvalidCode("ARTIST_POST_TOO_MANY"));
            }
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO portal.artist_posts(id, link_id, kind, title, body, starts_at, place, url, pinned, created_by)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
            )
            .bind(id)
            .bind(link)
            .bind(p.kind)
            .bind(&p.title)
            .bind(&p.body)
            .bind(p.starts_at)
            .bind(&p.place)
            .bind(&p.url)
            .bind(p.pinned)
            .bind(a.user)
            .execute(&mut *tx)
            .await?;
            id
        }
    };
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(id),
        "artist.post_saved",
        p.kind,
        a.request,
    )
    .await?;
    let item: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {POST_JSON} FROM portal.artist_posts p WHERE p.id=$1"
    )))
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(item)
}

pub async fn delete_post(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    post: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = org_link(&mut tx, org, artist, true).await?;
    let done = sqlx::query("DELETE FROM portal.artist_posts WHERE link_id=$1 AND id=$2")
        .bind(link)
        .bind(post)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(post),
        "artist.post_deleted",
        "DELETED",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"ok": true}))
}

/// Every cheer of the page, hidden ones included, newest first.
pub async fn list_cheers(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    before: Option<Uuid>,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let link = org_link(&mut tx, org, artist, false).await?;
    let (items, more) = cheers_page(&mut tx, link, before, true).await?;
    let (total, hidden): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE hidden) FROM portal.artist_cheers WHERE link_id=$1",
    )
    .bind(link)
    .fetch_one(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items, "more": more, "total": total, "hidden": hidden}))
}

pub async fn patch_cheer(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    cheer: Uuid,
    i: CheerPatch,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = org_link(&mut tx, org, artist, true).await?;
    let row = sqlx::query(
        "UPDATE portal.artist_cheers SET hidden=COALESCE($3, hidden), heart=COALESCE($4, heart)
          WHERE link_id=$1 AND id=$2 RETURNING hidden, heart",
    )
    .bind(link)
    .bind(cheer)
    .bind(i.hidden)
    .bind(i.heart)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let hidden: bool = row.get("hidden");
    if i.hidden.is_some() {
        operations::audit(
            &mut tx,
            Some(a.user),
            Some(org),
            Some(cheer),
            "artist.cheer_moderated",
            if hidden { "HIDDEN" } else { "SHOWN" },
            a.request,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(json!({"id": cheer, "hidden": hidden, "heart": row.get::<bool, _>("heart")}))
}

pub async fn delete_cheer(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    cheer: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = org_link(&mut tx, org, artist, true).await?;
    let done = sqlx::query("DELETE FROM portal.artist_cheers WHERE link_id=$1 AND id=$2")
        .bind(link)
        .bind(cheer)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(Error::NotFound);
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(cheer),
        "artist.cheer_moderated",
        "DELETED",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"ok": true}))
}

/// Up to [`CHEERS_PAGE`] cheers older than `before`, newest first, and
/// whether more remain. The org sees hidden ones (with the flag).
async fn cheers_page(
    c: &mut PgConnection,
    link: Uuid,
    before: Option<Uuid>,
    org: bool,
) -> Result<(Vec<Value>, bool)> {
    let fields = if org {
        "jsonb_build_object('id', c.id, 'nickname', c.nickname, 'message', c.message, 'heart', c.heart, 'hidden', c.hidden, 'created_at', c.created_at)"
    } else {
        "jsonb_build_object('id', c.id, 'nickname', c.nickname, 'message', c.message, 'heart', c.heart, 'created_at', c.created_at)"
    };
    let mut items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {fields} FROM portal.artist_cheers c
          WHERE c.link_id=$1 AND ($2 OR NOT c.hidden)
            AND ($3::uuid IS NULL OR (c.created_at, c.id) < (SELECT b.created_at, b.id FROM portal.artist_cheers b WHERE b.id=$3 AND b.link_id=$1))
          ORDER BY c.created_at DESC, c.id DESC LIMIT $4"
    )))
    .bind(link)
    .bind(org)
    .bind(before)
    .bind(CHEERS_PAGE + 1)
    .fetch_all(&mut *c)
    .await?;
    let more = items.len() as i64 > CHEERS_PAGE;
    items.truncate(CHEERS_PAGE as usize);
    Ok((items, more))
}

// ---------------------------------------------------------------------------
// Public
// ---------------------------------------------------------------------------

/// A switched-on page: (link id, org, artist, cheers switched on).
async fn public_link(s: &AppState, slug: &str) -> Result<(Uuid, Uuid, Uuid, bool)> {
    if !crate::artist_links::valid_handle(slug) {
        return Err(Error::NotFound);
    }
    sqlx::query_as(
        "SELECT k.id, k.org_id, k.artist_id, k.cheers_enabled FROM portal.artist_links k
           JOIN catalog.artists a ON a.org_id=k.org_id AND a.id=k.artist_id
          WHERE k.slug=$1 AND k.enabled AND a.archived_at IS NULL",
    )
    .bind(slug)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)
}

/// What the page itself shows: the first posts, how many there are, and
/// whether cheers are open (with their count).
pub async fn preview(s: &AppState, link: Uuid, cheers_enabled: bool) -> Result<Value> {
    let posts: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {POST_JSON} FROM portal.artist_posts p WHERE p.link_id=$1 ORDER BY {POST_ORDER} LIMIT $2"
    )))
    .bind(link)
    .bind(PREVIEW)
    .fetch_all(&s.pool)
    .await?;
    let (posts_total, cheers_total): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM portal.artist_posts WHERE link_id=$1),
                (SELECT count(*) FROM portal.artist_cheers WHERE link_id=$1 AND NOT hidden)",
    )
    .bind(link)
    .fetch_one(&s.pool)
    .await?;
    Ok(json!({
        "posts": posts,
        "posts_total": posts_total,
        "cheers_enabled": cheers_enabled,
        "cheers_total": if cheers_enabled { cheers_total } else { 0 },
    }))
}

pub async fn public_posts(s: &AppState, slug: &str) -> Result<Value> {
    let (link, ..) = public_link(s, slug).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {POST_JSON} FROM portal.artist_posts p WHERE p.link_id=$1 ORDER BY {POST_ORDER}"
    )))
    .bind(link)
    .fetch_all(&s.pool)
    .await?;
    Ok(json!({"items": items}))
}

pub async fn public_cheers(s: &AppState, slug: &str, before: Option<Uuid>) -> Result<Value> {
    let (link, _, _, enabled) = public_link(s, slug).await?;
    if !enabled {
        return Ok(json!({"items": [], "more": false, "enabled": false}));
    }
    let mut c = s.pool.acquire().await?;
    let (items, more) = cheers_page(&mut c, link, before, false).await?;
    Ok(json!({"items": items, "more": more, "enabled": true}))
}

/// A fan leaves a cheer.
pub async fn cheer(s: &AppState, h: &HeaderMap, slug: &str, i: CheerInput) -> Result<Value> {
    let nickname = i.nickname.trim();
    if nickname.is_empty() || nickname.chars().count() > NICK_MAX {
        return Err(Error::InvalidCode("CHEER_NICKNAME_INVALID"));
    }
    text_policy::check(nickname)?;
    let message = i.message.trim();
    if message.is_empty() || message.chars().count() > CHEER_MAX {
        return Err(Error::InvalidCode("CHEER_MESSAGE_INVALID"));
    }
    text_policy::check_multiline(message)?;
    let (link, _, _, enabled) = public_link(s, slug).await?;
    if !enabled {
        return Err(Error::PolicyGate("CHEERS_CLOSED"));
    }
    let ip = auth::client_ip_key(h).unwrap_or_else(|| "unknown".into());
    auth::rate_all(
        &s.pool,
        &[
            (&format!("cheer:{ip}"), 5),
            (&format!("cheer-page:{link}"), 300),
            ("cheer:all", 5000),
        ],
    )
    .await?;
    let id = Uuid::new_v4();
    let item: Value = sqlx::query_scalar(
        "INSERT INTO portal.artist_cheers(id, link_id, nickname, message) VALUES($1,$2,$3,$4)
         RETURNING jsonb_build_object('id', id, 'nickname', nickname, 'message', message, 'heart', heart, 'created_at', created_at)",
    )
    .bind(id)
    .bind(link)
    .bind(nickname)
    .bind(message)
    .fetch_one(&s.pool)
    .await?;
    Ok(item)
}

/// The releases of the page with their tracks, newest first.
pub async fn discography(s: &AppState, slug: &str) -> Result<Value> {
    let (_, org, artist, _) = public_link(s, slug).await?;
    let hidden: Vec<String> = HIDDEN_STATES.iter().map(|v| v.to_string()).collect();
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT l.slug, r.title, r.release_type, r.draft->>'release_date' AS release_date,
                r.draft->>'release_time' AS release_time, r.artwork_asset_id IS NOT NULL AS cover,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('title', t.title, 'disc', t.disc_number, 'number', t.track_number,
                                   'artist', CASE WHEN t.artist_id <> $3 THEN ta.name END)
                                 ORDER BY t.disc_number, t.track_number)
                            FROM catalog.tracks t JOIN catalog.artists ta ON ta.org_id=t.org_id AND ta.id=t.artist_id
                           WHERE t.org_id=r.org_id AND t.release_id=r.id AND t.archived_at IS NULL), '[]') AS tracks
           FROM portal.smart_links l
           JOIN catalog.releases r ON r.id=l.release_id AND r.org_id=l.org_id
          WHERE l.org_id=$1 AND {}
            AND EXISTS (SELECT 1 FROM catalog.tracks t WHERE t.org_id=r.org_id AND t.release_id=r.id
                         AND t.artist_id=$3 AND t.archived_at IS NULL)
          ORDER BY (r.draft->>'release_date') DESC NULLS LAST, r.created_at DESC
          LIMIT 100",
        smart_links::visible_sql("$2")
    )))
    .bind(org)
    .bind(hidden)
    .bind(artist)
    .fetch_all(&s.pool)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let date: Option<String> = r.get("release_date");
            let released = smart_links::is_released(
                date.as_deref(),
                r.get::<Option<String>, _>("release_time").as_deref(),
            );
            json!({
                "slug": r.get::<String, _>("slug"),
                "title": r.get::<String, _>("title"),
                "type": r.get::<String, _>("release_type"),
                "release_date": date,
                "released": released,
                "cover": r.get::<bool, _>("cover"),
                "tracks": r.get::<Value, _>("tracks"),
            })
        })
        .collect();
    Ok(json!({"items": items}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_posts(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list_posts(&s, &a, org, id).await?))
}
async fn h_create_post(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<PostInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(save_post(&s, &a, org, id, None, i).await?))
}
async fn h_update_post(
    State(s): State<AppState>,
    Path((org, id, post)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<PostInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(save_post(&s, &a, org, id, Some(post), i).await?))
}
async fn h_delete_post(
    State(s): State<AppState>,
    Path((org, id, post)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(delete_post(&s, &a, org, id, post).await?))
}
async fn h_cheers(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<PageQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list_cheers(&s, &a, org, id, q.before).await?))
}
async fn h_patch_cheer(
    State(s): State<AppState>,
    Path((org, id, cheer)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<CheerPatch>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(patch_cheer(&s, &a, org, id, cheer, i).await?))
}
async fn h_delete_cheer(
    State(s): State<AppState>,
    Path((org, id, cheer)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(delete_cheer(&s, &a, org, id, cheer).await?))
}
async fn h_public_posts(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(public_posts(&s, &slug).await?))
}
async fn h_public_cheers(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    Query(q): Query<PageQuery>,
) -> Result<Json<Value>> {
    Ok(Json(public_cheers(&s, &slug, q.before).await?))
}
async fn h_cheer(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    h: HeaderMap,
    Json(i): Json<CheerInput>,
) -> Result<Json<Value>> {
    Ok(Json(cheer(&s, &h, &slug, i).await?))
}
async fn h_discography(State(s): State<AppState>, Path(slug): Path<String>) -> Result<Json<Value>> {
    Ok(Json(discography(&s, &slug).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/artists/{id}/link/posts",
            get(h_posts).post(h_create_post),
        )
        .route(
            "/api/orgs/{org}/artists/{id}/link/posts/{post}",
            put(h_update_post).delete(h_delete_post),
        )
        .route("/api/orgs/{org}/artists/{id}/link/cheers", get(h_cheers))
        .route(
            "/api/orgs/{org}/artists/{id}/link/cheers/{cheer}",
            put(h_patch_cheer).delete(h_delete_cheer),
        )
        .route("/api/artist-links/{slug}/posts", get(h_public_posts))
        .route(
            "/api/artist-links/{slug}/cheers",
            get(h_public_cheers).post(h_cheer),
        )
        .route("/api/artist-links/{slug}/discography", get(h_discography))
}
