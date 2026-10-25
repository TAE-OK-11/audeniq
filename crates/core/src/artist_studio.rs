//! Artist studio (migration 0155): one screen for everything fans see of an
//! artist — the artist page, its smart links, news and cheers — and how it
//! is doing, plus the artist's profile photo.
//!
//! Insights (`GET .../artists/{id}/insights?days=7|30|90`) add up what the
//! pages already count per day (KST): artist page views
//! ([`crate::artist_links`]), smart link views, clicks per platform and
//! visits per shared channel ([`crate::smart_links`]) of the artist's
//! releases, each with the period before for comparison, a day-by-day series,
//! and per release the views, clicks, release alert sign-ups and pre-saves.
//!
//! The profile photo is an uploaded IMAGE asset (sanitized like cover art);
//! the artist page shows it. Platforms that take an artist photo from the
//! distributor ([`PHOTO_PLATFORMS`]) get it through a `PROFILE_IMAGE`
//! platform request per platform, which staff send and track
//! ([`crate::platform_requests`]); the request keeps the photo it was made
//! with. Spotify, Apple Music, Deezer and YouTube only take a photo from the
//! artist's own account there, so the screen hands the artist the file
//! instead of promising a delivery.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    smart_links::{self, HIDDEN_STATES},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// Platforms AUDENIQ can send an artist photo to as the distributor.
pub const PHOTO_PLATFORMS: &[&str] = &["melon", "genie", "bugs", "flo", "amazon"];
const PERIODS: &[i32] = &[7, 30, 90];
const VIEW: &str = "_view";
const SOURCE_PREFIX: &str = "src_";

async fn known_artist(c: &mut PgConnection, org: Uuid, artist: Uuid) -> Result<String> {
    sqlx::query_scalar(
        "SELECT name FROM catalog.artists WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(artist)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)
}

// ---------------------------------------------------------------------------
// Profile photo
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhotoInput {
    pub asset_id: Uuid,
}

/// Use an uploaded image as the artist's photo. The image must be one the
/// member can read and already servable (registered and sanitized), so the
/// page never points at a photo it cannot show.
pub async fn set_photo(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    i: PhotoInput,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    known_artist(&mut tx, org, artist).await?;
    auth::authorize(&mut tx, a, org, i.asset_id, "asset", false).await?;
    tx.commit().await?;
    if crate::uploads::stored_image(s, org, i.asset_id)
        .await
        .is_err()
    {
        return Err(Error::InvalidCode("ARTIST_PHOTO_NOT_READY"));
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    sqlx::query(
        "INSERT INTO portal.artist_photos(org_id, artist_id, asset_id, updated_by) VALUES($1,$2,$3,$4)
         ON CONFLICT (org_id, artist_id) DO UPDATE SET asset_id=$3, updated_by=$4, updated_at=now()",
    )
    .bind(org)
    .bind(artist)
    .bind(i.asset_id)
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(artist),
        "artist.photo_set",
        "SET",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"photo": true}))
}

pub async fn remove_photo(s: &AppState, a: &Actor, org: Uuid, artist: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    known_artist(&mut tx, org, artist).await?;
    sqlx::query("DELETE FROM portal.artist_photos WHERE org_id=$1 AND artist_id=$2")
        .bind(org)
        .bind(artist)
        .execute(&mut *tx)
        .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(artist),
        "artist.photo_removed",
        "REMOVED",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"photo": false}))
}

async fn photo_asset(c: &mut PgConnection, org: Uuid, artist: Uuid) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT asset_id FROM portal.artist_photos WHERE org_id=$1 AND artist_id=$2",
    )
    .bind(org)
    .bind(artist)
    .fetch_optional(&mut *c)
    .await?)
}

/// The photo for the org's own screens.
pub async fn photo(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
) -> Result<crate::uploads::StoredImage> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let asset = photo_asset(&mut tx, org, artist)
        .await?
        .ok_or(Error::NotFound)?;
    tx.rollback().await?;
    crate::uploads::stored_image(s, org, asset).await
}

/// The photo on the public artist page (only while the page is open).
pub async fn public_photo(s: &AppState, slug: &str) -> Result<crate::uploads::StoredImage> {
    if !crate::artist_links::valid_handle(slug) {
        return Err(Error::NotFound);
    }
    let row = sqlx::query(
        "SELECT k.org_id, p.asset_id FROM portal.artist_links k
           JOIN catalog.artists a ON a.org_id=k.org_id AND a.id=k.artist_id
           JOIN portal.artist_photos p ON p.org_id=k.org_id AND p.artist_id=k.artist_id
          WHERE k.slug=$1 AND k.enabled AND a.archived_at IS NULL",
    )
    .bind(slug)
    .fetch_optional(&s.pool)
    .await?
    .ok_or(Error::NotFound)?;
    crate::uploads::stored_image(s, row.get("org_id"), row.get("asset_id")).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendInput {
    pub platforms: Vec<String>,
}

/// Ask AUDENIQ to put the current photo on each platform: one
/// `PROFILE_IMAGE` request per platform. A platform that already has an open
/// request for this artist is left as it is (`skipped`).
pub async fn send_photo(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    i: SendInput,
) -> Result<Value> {
    let mut platforms: Vec<&str> = Vec::new();
    for p in &i.platforms {
        let p = PHOTO_PLATFORMS
            .iter()
            .find(|x| **x == p.as_str())
            .ok_or(Error::InvalidCode("PLATFORM_REQUEST_PLATFORM_INVALID"))?;
        if !platforms.contains(p) {
            platforms.push(p);
        }
    }
    if platforms.is_empty() {
        return Err(Error::InvalidCode("PLATFORM_REQUEST_PLATFORM_INVALID"));
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let name = known_artist(&mut tx, org, artist).await?;
    let asset = photo_asset(&mut tx, org, artist)
        .await?
        .ok_or(Error::InvalidCode("ARTIST_PHOTO_REQUIRED"))?;
    auth::rate(&s.pool, &format!("platform-request:{org}"), 30).await?;
    // Serialize concurrent sends for the artist.
    sqlx::query("SELECT 1 FROM catalog.artists WHERE org_id=$1 AND id=$2 FOR UPDATE")
        .bind(org)
        .bind(artist)
        .execute(&mut *tx)
        .await?;
    let mut created = Vec::new();
    let mut skipped = Vec::new();
    for p in platforms {
        let open: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM portal.platform_requests WHERE org_id=$1 AND artist_id=$2 AND kind='PROFILE_IMAGE'
                            AND platform=$3 AND status IN ('SUBMITTED','IN_PROGRESS','SENT_TO_PLATFORM'))",
        )
        .bind(org)
        .bind(artist)
        .bind(p)
        .fetch_one(&mut *tx)
        .await?;
        if open {
            skipped.push(p);
            continue;
        }
        // The artist's page on the platform, when known, so staff change the right one.
        let urls: Vec<String> = sqlx::query_scalar(
            "SELECT url FROM catalog.artist_platform_pages WHERE org_id=$1 AND artist_id=$2 AND platform=$3 AND status='LINKED' AND url IS NOT NULL",
        )
        .bind(org)
        .bind(artist)
        .bind(p)
        .fetch_all(&mut *tx)
        .await?;
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO portal.platform_requests(id, org_id, kind, platform, artist_name, urls, created_by, asset_id, artist_id)
             VALUES($1,$2,'PROFILE_IMAGE',$3,$4,$5,$6,$7,$8)",
        )
        .bind(id)
        .bind(org)
        .bind(p)
        .bind(&name)
        .bind(&urls)
        .bind(a.user)
        .bind(asset)
        .bind(artist)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO portal.platform_request_events(request_id, status, note, by_staff) VALUES($1,'SUBMITTED','',false)")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        created.push(json!({"id": id, "platform": p}));
    }
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(artist),
        "platform_request.created",
        "PROFILE_IMAGE",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"created": created, "skipped": skipped}))
}

/// Latest photo request per platform for the artist.
async fn photo_requests(c: &mut PgConnection, org: Uuid, artist: Uuid) -> Result<Value> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT DISTINCT ON (p.platform) jsonb_build_object('id',p.id,'platform',p.platform,'status',p.status,
                'resolution',p.resolution,'created_at',p.created_at,'updated_at',p.updated_at,
                'current', p.asset_id = (SELECT f.asset_id FROM portal.artist_photos f WHERE f.org_id=p.org_id AND f.artist_id=p.artist_id))
           FROM portal.platform_requests p
          WHERE p.org_id=$1 AND p.artist_id=$2 AND p.kind='PROFILE_IMAGE'
          ORDER BY p.platform, p.created_at DESC",
    )
    .bind(org)
    .bind(artist)
    .fetch_all(&mut *c)
    .await?;
    Ok(Value::Array(rows))
}

// ---------------------------------------------------------------------------
// Insights
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct InsightsQuery {
    #[serde(default)]
    pub days: Option<i32>,
}

fn hidden() -> Vec<String> {
    HIDDEN_STATES.iter().map(|v| v.to_string()).collect()
}

pub async fn insights(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    q: InsightsQuery,
) -> Result<Value> {
    let days = q.days.unwrap_or(30);
    if !PERIODS.contains(&days) {
        return Err(Error::InvalidCode("INSIGHTS_PERIOD_INVALID"));
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let name = known_artist(&mut tx, org, artist).await?;

    let page = sqlx::query(
        "SELECT id, slug, enabled, cheers_enabled FROM portal.artist_links WHERE org_id=$1 AND artist_id=$2",
    )
    .bind(org)
    .bind(artist)
    .fetch_optional(&mut *tx)
    .await?;
    let page_id: Option<Uuid> = page.as_ref().map(|r| r.get("id"));

    // The artist's submitted releases (newest first) with their smart link, if any.
    let releases = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT r.id, r.title, r.status, r.release_type, r.draft->>'release_date' AS release_date, r.draft->>'release_time' AS release_time,
                r.artwork_asset_id IS NOT NULL AS cover, l.id AS link_id, l.slug, l.enabled,
                (l.id IS NOT NULL AND {}) AS visible
           FROM catalog.releases r
           LEFT JOIN portal.smart_links l ON l.release_id=r.id AND l.org_id=r.org_id
          WHERE r.org_id=$1 AND r.archived_at IS NULL AND r.status NOT IN ('DRAFT','SUPERSEDED')
            AND EXISTS (SELECT 1 FROM catalog.tracks t WHERE t.org_id=r.org_id AND t.release_id=r.id
                         AND t.artist_id=$3 AND t.archived_at IS NULL)
          ORDER BY (r.draft->>'release_date') DESC NULLS LAST, r.created_at DESC
          LIMIT 200",
        smart_links::visible_sql("$2")
    )))
    .bind(org)
    .bind(hidden())
    .bind(artist)
    .fetch_all(&mut *tx)
    .await?;
    let links: Vec<Uuid> = releases
        .iter()
        .filter_map(|r| r.get::<Option<Uuid>, _>("link_id"))
        .collect();

    // Day by day for the period (KST), with zero days filled in.
    let series_rows = sqlx::query(
        "WITH today AS (SELECT (now() AT TIME ZONE 'Asia/Seoul')::date AS d),
              days AS (SELECT generate_series((SELECT d FROM today) - ($1 - 1), (SELECT d FROM today), interval '1 day')::date AS day)
         SELECT days.day,
                COALESCE((SELECT sum(count) FROM portal.artist_link_stats st WHERE st.link_id=$2 AND st.day=days.day), 0)::bigint AS page_views,
                COALESCE((SELECT sum(count) FROM portal.smart_link_stats st WHERE st.link_id=ANY($3) AND st.day=days.day AND st.platform=$4), 0)::bigint AS link_views,
                COALESCE((SELECT sum(count) FROM portal.smart_link_stats st WHERE st.link_id=ANY($3) AND st.day=days.day
                           AND st.platform<>$4 AND st.platform NOT LIKE 'src\\_%'), 0)::bigint AS clicks
           FROM days ORDER BY days.day",
    )
    .bind(days)
    .bind(page_id)
    .bind(&links)
    .bind(VIEW)
    .fetch_all(&mut *tx)
    .await?;
    let series: Vec<Value> = series_rows
        .iter()
        .map(|r| {
            json!({
                "day": r.get::<chrono::NaiveDate, _>("day").to_string(),
                "page_views": r.get::<i64, _>("page_views"),
                "link_views": r.get::<i64, _>("link_views"),
                "clicks": r.get::<i64, _>("clicks"),
            })
        })
        .collect();

    // Totals of the period before, for comparison.
    let prev = sqlx::query(
        "WITH today AS (SELECT (now() AT TIME ZONE 'Asia/Seoul')::date AS d)
         SELECT COALESCE((SELECT sum(count) FROM portal.artist_link_stats st WHERE st.link_id=$2
                           AND st.day > (SELECT d FROM today) - 2*$1 AND st.day <= (SELECT d FROM today) - $1), 0)::bigint AS page_views,
                COALESCE((SELECT sum(count) FROM portal.smart_link_stats st WHERE st.link_id=ANY($3) AND st.platform=$4
                           AND st.day > (SELECT d FROM today) - 2*$1 AND st.day <= (SELECT d FROM today) - $1), 0)::bigint AS link_views,
                COALESCE((SELECT sum(count) FROM portal.smart_link_stats st WHERE st.link_id=ANY($3)
                           AND st.platform<>$4 AND st.platform NOT LIKE 'src\\_%'
                           AND st.day > (SELECT d FROM today) - 2*$1 AND st.day <= (SELECT d FROM today) - $1), 0)::bigint AS clicks",
    )
    .bind(days)
    .bind(page_id)
    .bind(&links)
    .bind(VIEW)
    .fetch_one(&mut *tx)
    .await?;

    // Per link and platform in the period: views, clicks and shared channels.
    let per = sqlx::query(
        "SELECT link_id, platform, sum(count)::bigint AS n FROM portal.smart_link_stats
          WHERE link_id=ANY($1) AND day > (now() AT TIME ZONE 'Asia/Seoul')::date - $2
          GROUP BY link_id, platform",
    )
    .bind(&links)
    .bind(days)
    .fetch_all(&mut *tx)
    .await?;
    let mut platforms = Map::new();
    let mut sources = Map::new();
    let add = |m: &mut Map<String, Value>, k: &str, n: i64| {
        let cur = m.get(k).and_then(Value::as_i64).unwrap_or(0);
        m.insert(k.to_string(), json!(cur + n));
    };
    for r in &per {
        let p: String = r.get("platform");
        let n: i64 = r.get("n");
        if p == VIEW {
            continue;
        }
        if let Some(src) = p.strip_prefix(SOURCE_PREFIX) {
            add(&mut sources, src, n);
        } else {
            add(&mut platforms, &p, n);
        }
    }
    let alerts: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT link_id, count(*), count(*) FILTER (WHERE created_at > now() - make_interval(days => $2))
           FROM portal.release_alerts WHERE link_id=ANY($1) GROUP BY link_id",
    )
    .bind(&links)
    .bind(days)
    .fetch_all(&mut *tx)
    .await?;
    let presaves: Vec<(Uuid, i64, i64)> = sqlx::query_as(
        "SELECT link_id, count(*), count(*) FILTER (WHERE created_at > now() - make_interval(days => $2))
           FROM portal.presaves WHERE link_id=ANY($1) GROUP BY link_id",
    )
    .bind(&links)
    .bind(days)
    .fetch_all(&mut *tx)
    .await?;
    let release_items: Vec<Value> = releases
        .iter()
        .map(|r| {
            let link: Option<Uuid> = r.get("link_id");
            let sum = |want: &dyn Fn(&str) -> bool| -> i64 {
                per.iter()
                    .filter(|x| Some(x.get::<Uuid, _>("link_id")) == link)
                    .filter(|x| want(&x.get::<String, _>("platform")))
                    .map(|x| x.get::<i64, _>("n"))
                    .sum()
            };
            let views = sum(&|p| p == VIEW);
            let clicks = sum(&|p| p != VIEW && !p.starts_with(SOURCE_PREFIX));
            let find = |list: &[(Uuid, i64, i64)]| {
                list.iter()
                    .find(|x| Some(x.0) == link)
                    .map(|x| (x.1, x.2))
                    .unwrap_or((0, 0))
            };
            let (alerts_all, alerts_new) = find(&alerts);
            let (presaves_all, presaves_new) = find(&presaves);
            let date: Option<String> = r.get("release_date");
            json!({
                "release_id": r.get::<Uuid, _>("id"),
                "title": r.get::<String, _>("title"),
                "type": r.get::<String, _>("release_type"),
                "release_date": date,
                "released": smart_links::is_released(date.as_deref(), r.get::<Option<String>, _>("release_time").as_deref()),
                "cover": r.get::<bool, _>("cover"),
                "link": link.map(|_| json!({
                    "slug": r.get::<Option<String>, _>("slug"),
                    "enabled": r.get::<Option<bool>, _>("enabled"),
                    "visible": r.get::<bool, _>("visible"),
                })),
                "views": views,
                "clicks": clicks,
                "alerts": alerts_all,
                "alerts_new": alerts_new,
                "presaves": presaves_all,
                "presaves_new": presaves_new,
            })
        })
        .collect();

    let community = match page_id {
        Some(id) => {
            let (posts, cheers, cheers_new, hearts): (i64, i64, i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM portal.artist_posts WHERE link_id=$1),
                        (SELECT count(*) FROM portal.artist_cheers WHERE link_id=$1 AND NOT hidden),
                        (SELECT count(*) FROM portal.artist_cheers WHERE link_id=$1 AND NOT hidden
                           AND created_at > now() - make_interval(days => $2)),
                        (SELECT count(*) FROM portal.artist_cheers WHERE link_id=$1 AND heart)",
            )
            .bind(id)
            .bind(days)
            .fetch_one(&mut *tx)
            .await?;
            json!({"posts": posts, "cheers": cheers, "cheers_new": cheers_new, "hearts": hearts})
        }
        None => json!({"posts": 0, "cheers": 0, "cheers_new": 0, "hearts": 0}),
    };
    let photo = photo_asset(&mut tx, org, artist).await?.is_some();
    let requests = photo_requests(&mut tx, org, artist).await?;
    tx.rollback().await?;

    let total = |k: &str| {
        series
            .iter()
            .map(|d| d[k].as_i64().unwrap_or(0))
            .sum::<i64>()
    };
    Ok(json!({
        "artist_id": artist,
        "name": name,
        "days": days,
        "page": page.map(|r| json!({
            "slug": r.get::<String, _>("slug"),
            "enabled": r.get::<bool, _>("enabled"),
            "cheers_enabled": r.get::<bool, _>("cheers_enabled"),
        })),
        "photo": photo,
        "photo_platforms": PHOTO_PLATFORMS,
        "photo_requests": requests,
        "totals": {
            "page_views": total("page_views"),
            "link_views": total("link_views"),
            "clicks": total("clicks"),
        },
        "previous": {
            "page_views": prev.get::<i64, _>("page_views"),
            "link_views": prev.get::<i64, _>("link_views"),
            "clicks": prev.get::<i64, _>("clicks"),
        },
        "series": series,
        "platforms": platforms,
        "sources": sources,
        "releases": release_items,
        "community": community,
    }))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_insights(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<InsightsQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(insights(&s, &a, org, id, q).await?))
}
async fn h_photo(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Response> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(crate::uploads::image_response(
        photo(&s, &a, org, id).await?,
        &h,
    ))
}
async fn h_set_photo(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<PhotoInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(set_photo(&s, &a, org, id, i).await?))
}
async fn h_remove_photo(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(remove_photo(&s, &a, org, id).await?))
}
async fn h_send_photo(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<SendInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(send_photo(&s, &a, org, id, i).await?))
}
async fn h_public_photo(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    h: HeaderMap,
) -> Result<Response> {
    Ok(crate::uploads::image_response(
        public_photo(&s, &slug).await?,
        &h,
    ))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/artists/{id}/insights", get(h_insights))
        .route(
            "/api/orgs/{org}/artists/{id}/photo",
            get(h_photo).put(h_set_photo).delete(h_remove_photo),
        )
        .route(
            "/api/orgs/{org}/artists/{id}/photo/send",
            post(h_send_photo),
        )
        .route("/api/artist-links/{slug}/photo", get(h_public_photo))
}
