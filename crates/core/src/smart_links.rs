//! Smart links (migration 0117).
//!
//! One public landing page per submitted release, `/l/{slug}`: cover,
//! title, artist, release date and the platform links the artist pasted
//! (each checked against that platform's hosts, https only). Anyone can open
//! it; views and clicks are counted per day and platform (once per visitor
//! address every 15 minutes) and shown to the org for the last 30 days. The
//! page disappears while the release is withdrawn, on hold, taken down or
//! suspended over a rights claim.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::Row;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Platforms in page order, with the hosts their links may point at.
pub const PLATFORMS: &[(&str, &[&str])] = &[
    ("melon", &["melon.com", "kko.to"]),
    ("genie", &["genie.co.kr"]),
    ("flo", &["music-flo.com", "flomuz.io"]),
    ("bugs", &["bugs.co.kr"]),
    ("vibe", &["vibe.naver.com", "naver.me"]),
    ("spotify", &["open.spotify.com", "spotify.link"]),
    ("apple", &["music.apple.com"]),
    ("youtube", &["music.youtube.com", "youtube.com", "youtu.be"]),
    ("amazon", &["music.amazon.com", "amazon.com"]),
    ("deezer", &["deezer.com", "deezer.page.link"]),
    ("tidal", &["tidal.com"]),
];

const VIEW: &str = "_view";
const STATS_DAYS: i32 = 30;

/// Release states without a page.
const HIDDEN_STATES: &[&str] = &["DRAFT", "WITHDRAWN", "SUPERSEDED", "ON_HOLD_RIGHTS"];

pub(crate) fn hosts(platform: &str) -> Option<&'static [&'static str]> {
    PLATFORMS
        .iter()
        .find(|(p, _)| *p == platform)
        .map(|(_, h)| *h)
}

/// A pasted link: https, no credentials, host of the platform (or a
/// subdomain of it).
fn check_link(platform: &str, raw: &str) -> Result<String> {
    let invalid = || Error::InvalidCode("SMART_LINK_URL_INVALID");
    let allowed = hosts(platform).ok_or(Error::InvalidCode("SMART_LINK_PLATFORM_INVALID"))?;
    let raw = raw.trim();
    if raw.len() > 500 {
        return Err(invalid());
    }
    let u = url::Url::parse(raw).map_err(|_| invalid())?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return Err(invalid());
    }
    let host = u.host_str().ok_or_else(invalid)?.to_ascii_lowercase();
    if !allowed
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
    {
        return Err(invalid());
    }
    Ok(u.to_string())
}

fn new_slug() -> Result<String> {
    use rand::{TryRng, rngs::SysRng};
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut raw = [0u8; 8];
    SysRng
        .try_fill_bytes(&mut raw)
        .map_err(|_| Error::Internal)?;
    Ok(raw
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect())
}

fn valid_slug(slug: &str) -> bool {
    slug.len() == 8
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// Org
// ---------------------------------------------------------------------------

/// The release's link, its counts for the last 30 days and whether one can
/// be made.
pub async fn status(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let state: String = sqlx::query_scalar(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let link = sqlx::query(
        "SELECT id, slug, enabled, links FROM portal.smart_links WHERE org_id=$1 AND release_id=$2",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let mut stats =
        json!({"views": 0, "clicks": {}, "days": STATS_DAYS, "alerts": 0, "alerts_waiting": 0});
    if let Some(l) = &link {
        let rows = sqlx::query(
            "SELECT platform, sum(count)::bigint AS n FROM portal.smart_link_stats
              WHERE link_id=$1 AND day > (now() AT TIME ZONE 'Asia/Seoul')::date - $2 GROUP BY platform",
        )
        .bind(l.get::<Uuid, _>("id"))
        .bind(STATS_DAYS)
        .fetch_all(&mut *tx)
        .await?;
        let mut clicks = Map::new();
        for r in rows {
            let p: String = r.get("platform");
            let n: i64 = r.get("n");
            if p == VIEW {
                stats["views"] = json!(n);
            } else {
                clicks.insert(p, json!(n));
            }
        }
        stats["clicks"] = Value::Object(clicks);
        // Release alert sign-ups (all time) and how many still wait.
        let (alerts, waiting): (i64, i64) = sqlx::query_as(
            "SELECT count(*), count(*) FILTER (WHERE status='PENDING') FROM portal.release_alerts WHERE link_id=$1",
        )
        .bind(l.get::<Uuid, _>("id"))
        .fetch_one(&mut *tx)
        .await?;
        stats["alerts"] = json!(alerts);
        stats["alerts_waiting"] = json!(waiting);
    }
    tx.commit().await?;
    Ok(json!({
        "link": link.map(|l| json!({"slug": l.get::<String, _>("slug"), "enabled": l.get::<bool, _>("enabled"), "links": l.get::<Value, _>("links")})),
        "stats": stats,
        "platforms": PLATFORMS.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        "can_create": !HIDDEN_STATES.contains(&state.as_str()),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkInput {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub links: BTreeMap<String, String>,
}
fn yes() -> bool {
    true
}

/// Create the page (first save) or replace its links.
pub async fn save(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: LinkInput,
) -> Result<Value> {
    let mut links = Map::new();
    for (platform, url) in &i.links {
        if url.trim().is_empty() {
            continue;
        }
        links.insert(platform.clone(), json!(check_link(platform, url)?));
    }
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let state: String = sqlx::query_scalar(
        "SELECT status FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT slug FROM portal.smart_links WHERE org_id=$1 AND release_id=$2 FOR UPDATE",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let slug = match existing {
        Some(slug) => {
            sqlx::query("UPDATE portal.smart_links SET enabled=$3, links=$4, updated_at=now() WHERE org_id=$1 AND release_id=$2")
                .bind(org)
                .bind(release)
                .bind(i.enabled)
                .bind(Value::Object(links))
                .execute(&mut *tx)
                .await?;
            slug
        }
        None => {
            if HIDDEN_STATES.contains(&state.as_str()) {
                return Err(Error::PolicyGate("SMART_LINK_RELEASE_NOT_SUBMITTED"));
            }
            let mut made = None;
            for _ in 0..5 {
                let slug = new_slug()?;
                let ok: Option<String> = sqlx::query_scalar(
                    "INSERT INTO portal.smart_links(id, org_id, release_id, slug, enabled, links, created_by)
                     VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (slug) DO NOTHING RETURNING slug",
                )
                .bind(Uuid::new_v4())
                .bind(org)
                .bind(release)
                .bind(&slug)
                .bind(i.enabled)
                .bind(Value::Object(links.clone()))
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
        Some(release),
        "release.smart_link_saved",
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

/// Whether the release is out now (KST): its date at its simultaneous
/// release time, or at midnight when it has none.
pub(crate) fn is_released(date: Option<&str>, time: Option<&str>) -> bool {
    let Some(date) = date.filter(|d| !d.is_empty()) else {
        return true;
    };
    let time = time.filter(|t| t.len() == 5).unwrap_or("00:00");
    let now = (chrono::Utc::now() + chrono::Duration::hours(9))
        .format("%Y-%m-%d %H:%M")
        .to_string();
    format!("{date} {time}") <= now
}

/// The visible link behind `slug`: (link id, org, release, artwork, links).
pub(crate) async fn visible(s: &AppState, slug: &str) -> Result<sqlx::postgres::PgRow> {
    if !valid_slug(slug) {
        return Err(Error::NotFound);
    }
    visible_where(&s.pool, "l.slug=$1", slug).await
}

/// The visible link with id `link` (release alerts check it before sending).
pub(crate) async fn visible_by_id(
    pool: &sqlx::PgPool,
    link: Uuid,
) -> Result<sqlx::postgres::PgRow> {
    visible_where(pool, "l.id=$1::uuid", &link.to_string()).await
}

async fn visible_where(
    pool: &sqlx::PgPool,
    key: &'static str,
    value: &str,
) -> Result<sqlx::postgres::PgRow> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT l.id, l.slug, l.org_id, l.links, r.title, r.draft->>'artist' AS artist, r.draft->>'release_date' AS release_date,
                r.draft->>'release_time' AS release_time, r.artwork_asset_id
           FROM portal.smart_links l
           JOIN catalog.releases r ON r.id=l.release_id AND r.org_id=l.org_id
          WHERE {key} AND l.enabled AND r.archived_at IS NULL AND NOT (r.status = ANY($2))
            AND NOT EXISTS (SELECT 1 FROM portal.takedown_requests t WHERE t.release_id=r.id AND t.status='APPROVED')
            AND NOT EXISTS (SELECT 1 FROM portal.infringement_claims c WHERE c.release_id=r.id
                             AND c.status IN ('SUSPENDED','COUNTER_NOTICED','UPHELD'))
            AND NOT EXISTS (SELECT 1 FROM portal.release_terms tm WHERE tm.release_id=r.id AND tm.status IN ('ENDED','FINAL_SETTLED'))"
    )))
    .bind(value)
    .bind(HIDDEN_STATES.iter().map(|v| v.to_string()).collect::<Vec<_>>())
    .fetch_optional(pool)
    .await?
    .ok_or(Error::NotFound)
}

/// Count once per visitor address per 15 minutes; never fails the request.
async fn count(s: &AppState, h: &HeaderMap, link: Uuid, slug: &str, platform: &str) {
    if let Some(ip) = auth::client_ip_key(h)
        && auth::rate(&s.pool, &format!("smart-link:{slug}:{platform}:{ip}"), 1)
            .await
            .is_err()
    {
        return;
    }
    let r = sqlx::query(
        "INSERT INTO portal.smart_link_stats(link_id, day, platform, count)
         VALUES($1, (now() AT TIME ZONE 'Asia/Seoul')::date, $2, 1)
         ON CONFLICT (link_id, day, platform) DO UPDATE SET count = portal.smart_link_stats.count + 1",
    )
    .bind(link)
    .bind(platform)
    .execute(&s.pool)
    .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "smart link count failed");
    }
}

pub async fn public_page(s: &AppState, h: &HeaderMap, slug: &str) -> Result<Value> {
    let row = visible(s, slug).await?;
    let links: Value = row.get("links");
    let list: Vec<Value> = PLATFORMS
        .iter()
        .filter_map(|(p, _)| {
            links
                .get(*p)
                .and_then(|u| u.as_str())
                .map(|u| json!({"platform": p, "url": u}))
        })
        .collect();
    let release_date: Option<String> = row.get("release_date");
    let released = is_released(
        release_date.as_deref(),
        row.get::<Option<String>, _>("release_time").as_deref(),
    );
    count(s, h, row.get("id"), slug, VIEW).await;
    Ok(json!({
        "title": row.get::<String, _>("title"),
        "artist": row.get::<Option<String>, _>("artist"),
        "release_date": release_date,
        "released": released,
        "cover": row.get::<Option<Uuid>, _>("artwork_asset_id").is_some(),
        "links": list,
    }))
}

pub async fn public_cover(s: &AppState, slug: &str) -> Result<crate::uploads::StoredImage> {
    let row = visible(s, slug).await?;
    let asset: Uuid = row
        .get::<Option<Uuid>, _>("artwork_asset_id")
        .ok_or(Error::NotFound)?;
    crate::uploads::stored_image(s, row.get("org_id"), asset).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Click {
    pub platform: String,
}

pub async fn click(s: &AppState, h: &HeaderMap, slug: &str, i: Click) -> Result<Value> {
    let row = visible(s, slug).await?;
    let links: Value = row.get("links");
    if hosts(&i.platform).is_none() || links.get(&i.platform).is_none() {
        return Err(Error::InvalidCode("SMART_LINK_PLATFORM_INVALID"));
    }
    count(s, h, row.get("id"), slug, &i.platform).await;
    Ok(json!({"ok": true}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_status(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(status(&s, &a, org, id).await?))
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
async fn h_cover(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    h: HeaderMap,
) -> Result<Response> {
    Ok(crate::uploads::image_response(
        public_cover(&s, &slug).await?,
        &h,
    ))
}
async fn h_click(
    State(s): State<AppState>,
    Path(slug): Path<String>,
    h: HeaderMap,
    Json(i): Json<Click>,
) -> Result<Json<Value>> {
    Ok(Json(click(&s, &h, &slug, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/releases/{id}/smart-link",
            get(h_status).put(h_save),
        )
        .route("/api/smart-links/{slug}", get(h_public))
        .route("/api/smart-links/{slug}/cover", get(h_cover))
        .route("/api/smart-links/{slug}/click", post(h_click))
}
