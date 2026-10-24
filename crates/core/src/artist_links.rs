//! Artist pages (migration 0151).
//!
//! One public page per artist, `/a/{slug}`: the artist's name, a short
//! introduction, their own pages on each platform (linked
//! [`crate::artist_pages`]) and every release of the artist that has a
//! visible smart link, newest first; each release opens its smart link
//! (`/l/{slug}`). A release appears and disappears with its smart link, so
//! the page never shows a release the smart link would hide. Anyone can open
//! it; views are counted per day (once per visitor address every 15 minutes)
//! and shown to the org for the last 30 days. The page disappears while it
//! is switched off or the artist is archived.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    smart_links::{self, HIDDEN_STATES, STATS_DAYS},
    text_policy,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

const BIO_MAX: usize = 1500;

fn hidden() -> Vec<String> {
    HIDDEN_STATES.iter().map(|v| v.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Org
// ---------------------------------------------------------------------------

/// Every artist of the org with their page (if made), how many releases the
/// page would show and its views in the last 30 days.
pub async fn list(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT jsonb_build_object('artist_id', a.id, 'name', a.name,
                'link', (SELECT jsonb_build_object('slug', k.slug, 'enabled', k.enabled, 'bio', k.bio,
                                  'views', COALESCE((SELECT sum(st.count) FROM portal.artist_link_stats st
                                                      WHERE st.link_id=k.id AND st.day > (now() AT TIME ZONE 'Asia/Seoul')::date - $3), 0))
                           FROM portal.artist_links k WHERE k.org_id=a.org_id AND k.artist_id=a.id),
                'releases', (SELECT count(DISTINCT r.id) FROM portal.smart_links l
                               JOIN catalog.releases r ON r.id=l.release_id AND r.org_id=l.org_id
                              WHERE l.org_id=a.org_id AND {}
                                AND EXISTS (SELECT 1 FROM catalog.tracks t WHERE t.org_id=r.org_id AND t.release_id=r.id
                                             AND t.artist_id=a.id AND t.archived_at IS NULL)))
           FROM catalog.artists a WHERE a.org_id=$1 AND a.archived_at IS NULL ORDER BY a.name LIMIT 500",
        smart_links::visible_sql("$2")
    )))
    .bind(org)
    .bind(hidden())
    .bind(STATS_DAYS)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkInput {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub bio: String,
}
fn yes() -> bool {
    true
}

/// Create the page (first save) or change its introduction / switch it.
pub async fn save(s: &AppState, a: &Actor, org: Uuid, artist: Uuid, i: LinkInput) -> Result<Value> {
    let bio = i.bio.trim();
    if bio.chars().count() > BIO_MAX {
        return Err(Error::Invalid);
    }
    if !bio.is_empty() {
        text_policy::check_multiline(bio)?;
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let known: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM catalog.artists WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(artist)
    .fetch_optional(&mut *tx)
    .await?;
    known.ok_or(Error::NotFound)?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT slug FROM portal.artist_links WHERE org_id=$1 AND artist_id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(artist)
    .fetch_optional(&mut *tx)
    .await?;
    let slug = match existing {
        Some(slug) => {
            sqlx::query(
                "UPDATE portal.artist_links SET enabled=$3, bio=$4, updated_at=now() WHERE org_id=$1 AND artist_id=$2",
            )
            .bind(org)
            .bind(artist)
            .bind(i.enabled)
            .bind(bio)
            .execute(&mut *tx)
            .await?;
            slug
        }
        None => {
            let mut made = None;
            for _ in 0..5 {
                let slug = smart_links::new_slug()?;
                let ok: Option<String> = sqlx::query_scalar(
                    "INSERT INTO portal.artist_links(id, org_id, artist_id, slug, enabled, bio, created_by)
                     VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (slug) DO NOTHING RETURNING slug",
                )
                .bind(Uuid::new_v4())
                .bind(org)
                .bind(artist)
                .bind(&slug)
                .bind(i.enabled)
                .bind(bio)
                .bind(a.user)
                .fetch_optional(&mut *tx)
                .await?;
                if ok.is_some() {
                    made = ok;
                    break;
                }
            }
            made.ok_or(Error::Internal)?
        }
    };
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(artist),
        "artist.page_saved",
        if i.enabled { "ENABLED" } else { "DISABLED" },
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"slug": slug, "enabled": i.enabled}))
}

// ---------------------------------------------------------------------------
// Public
// ---------------------------------------------------------------------------

/// Count once per visitor address per 15 minutes; never fails the request.
async fn count(s: &AppState, h: &HeaderMap, link: Uuid, slug: &str) {
    if let Some(ip) = auth::client_ip_key(h)
        && auth::rate(&s.pool, &format!("artist-link:{slug}:{ip}"), 1)
            .await
            .is_err()
    {
        return;
    }
    let r = sqlx::query(
        "INSERT INTO portal.artist_link_stats(link_id, day, count)
         VALUES($1, (now() AT TIME ZONE 'Asia/Seoul')::date, 1)
         ON CONFLICT (link_id, day) DO UPDATE SET count = portal.artist_link_stats.count + 1",
    )
    .bind(link)
    .execute(&s.pool)
    .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "artist page count failed");
    }
}

pub async fn public_page(s: &AppState, h: &HeaderMap, slug: &str) -> Result<Value> {
    if !smart_links::valid_slug(slug) {
        return Err(Error::NotFound);
    }
    let row = sqlx::query(
        "SELECT k.id, k.org_id, k.artist_id, k.bio, a.name
           FROM portal.artist_links k
           JOIN catalog.artists a ON a.org_id=k.org_id AND a.id=k.artist_id
          WHERE k.slug=$1 AND k.enabled AND a.archived_at IS NULL",
    )
    .bind(slug)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    let org: Uuid = row.get("org_id");
    let artist: Uuid = row.get("artist_id");

    // The artist's own pages, in smart link platform order.
    let page_rows = sqlx::query(
        "SELECT platform, url FROM catalog.artist_platform_pages WHERE org_id=$1 AND artist_id=$2 AND status='LINKED'",
    )
    .bind(org)
    .bind(artist)
    .fetch_all(&s.pool)
    .await?;
    let pages: Vec<Value> = smart_links::PLATFORMS
        .iter()
        .filter_map(|(p, _)| {
            page_rows
                .iter()
                .find(|r| r.get::<String, _>("platform") == *p)
                .map(|r| json!({"platform": p, "url": r.get::<String, _>("url")}))
        })
        .collect();

    // Releases with a visible smart link, newest first (undated last).
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT l.slug, r.title, r.release_type, r.draft->>'release_date' AS release_date,
                r.draft->>'release_time' AS release_time, r.artwork_asset_id IS NOT NULL AS cover
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
    .bind(hidden())
    .bind(artist)
    .fetch_all(&s.pool)
    .await?;
    let releases: Vec<Value> = rows
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
            })
        })
        .collect();
    count(s, h, row.get("id"), slug).await;
    Ok(json!({
        "name": row.get::<String, _>("name"),
        "bio": row.get::<String, _>("bio"),
        "pages": pages,
        "releases": releases,
    }))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list(&s, &a, org).await?))
}
async fn h_save(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<LinkInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(save(&s, &a, org, id, i).await?))
}
async fn h_public(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(public_page(&s, &h, &slug).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/artist-links", get(h_list))
        .route("/api/orgs/{org}/artists/{id}/link", put(h_save))
        .route("/api/artist-links/{slug}", get(h_public))
}
