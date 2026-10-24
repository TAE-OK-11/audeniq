//! Artist pages (migration 0151).
//!
//! One public page per artist, `/link/artist/{handle}`: the artist's name, a short
//! introduction, their own pages on each platform (linked
//! [`crate::artist_pages`]) and every release of the artist that has a
//! visible smart link, newest first; each release opens its smart link
//! (`/link/release/{slug}`). A release appears and disappears with its smart link, so
//! the page never shows a release the smart link would hide. Anyone can open
//! it; views are counted per day (once per visitor address every 15 minutes)
//! and shown to the org for the last 30 days. The page disappears while it
//! is switched off or the artist is archived.
//!
//! The artist adds their own links (social, tickets, shop: up to ten, https,
//! with a short label) and an optional contact address, shown on the press
//! page (`/link/artist/{handle}/press`) for curators, press and promoters.
//!
//! The handle (migration 0153) is chosen by the artist: 3–30 lowercase
//! letters, digits, '_' and '-', unique across all orgs (taken →
//! `ARTIST_LINK_HANDLE_TAKEN`), not a reserved word and not a protected
//! artist's name the org is not cleared for. Left empty, a random
//! 8-character handle is made. The old `/a/{slug}` addresses redirect.
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
const LINKS_MAX: usize = 10;
const LABEL_MAX: usize = 40;
const URL_MAX: usize = 500;
const HANDLE_MIN: usize = 3;
const HANDLE_MAX: usize = 30;
/// Words that read like AUDENIQ itself or a page of the site.
const RESERVED: &[&str] = &[
    "admin", "api", "app", "artist", "artists", "audeniq", "help", "link", "links", "login",
    "official", "press", "release", "releases", "settings", "signup", "staff", "studio", "support",
    "team", "www",
];

/// A handle as stored: lowercase letters, digits, '_' and '-', starting and
/// ending with a letter or digit (the random slugs fit the same rule).
pub(crate) fn valid_handle(s: &str) -> bool {
    let b = s.as_bytes();
    (HANDLE_MIN..=HANDLE_MAX).contains(&b.len())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
        && b[0].is_ascii_alphanumeric()
        && b[b.len() - 1].is_ascii_alphanumeric()
}

/// The handle the artist typed, normalised (trimmed, lowercase); empty = none.
fn clean_handle(raw: &str) -> Result<Option<String>> {
    let h = raw.trim().trim_start_matches('@').to_ascii_lowercase();
    if h.is_empty() {
        return Ok(None);
    }
    if !valid_handle(&h) {
        return Err(Error::InvalidCode("ARTIST_LINK_HANDLE_INVALID"));
    }
    if RESERVED.contains(&h.as_str()) {
        return Err(Error::InvalidCode("ARTIST_LINK_HANDLE_RESERVED"));
    }
    Ok(Some(h))
}

/// The handle with its separators as spaces and removed, for the protected
/// names check ('taylor_swift' → 'taylor swift', 'taylorswift').
fn handle_names(h: &str) -> [String; 3] {
    [
        h.to_string(),
        h.replace(['_', '-'], " "),
        h.replace(['_', '-'], ""),
    ]
}

fn taken(e: sqlx::Error) -> Error {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            Error::InvalidCode("ARTIST_LINK_HANDLE_TAKEN")
        }
        _ => Error::Database(e),
    }
}

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
                'link', (SELECT jsonb_build_object('slug', k.slug, 'enabled', k.enabled, 'bio', k.bio, 'links', k.links, 'contact', k.contact,
                                  'cheers_enabled', k.cheers_enabled,
                                  'posts', (SELECT count(*) FROM portal.artist_posts p WHERE p.link_id=k.id),
                                  'cheers', (SELECT count(*) FROM portal.artist_cheers c WHERE c.link_id=k.id),
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
    /// The artist's own links, in page order.
    #[serde(default)]
    pub links: Vec<OwnLink>,
    /// Contact address for the press page; empty hides it.
    #[serde(default)]
    pub contact: String,
    /// The page address the artist chooses; empty keeps the current one
    /// (or makes a random one).
    #[serde(default)]
    pub handle: String,
    /// Fans can leave cheers on the page; absent keeps the current setting.
    #[serde(default)]
    pub cheers: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnLink {
    pub label: String,
    pub url: String,
}

/// Up to ten links, each an https address with a short label; empty rows are dropped.
fn clean_links(links: &[OwnLink]) -> Result<Value> {
    let mut out = Vec::new();
    for l in links {
        let (label, url) = (l.label.trim(), l.url.trim());
        if label.is_empty() && url.is_empty() {
            continue;
        }
        if label.is_empty() || label.chars().count() > LABEL_MAX {
            return Err(Error::InvalidCode("ARTIST_LINK_LABEL_INVALID"));
        }
        text_policy::check(label)?;
        let parsed =
            url::Url::parse(url).map_err(|_| Error::InvalidCode("ARTIST_LINK_URL_INVALID"))?;
        if parsed.scheme() != "https" || parsed.host_str().is_none() || url.len() > URL_MAX {
            return Err(Error::InvalidCode("ARTIST_LINK_URL_INVALID"));
        }
        out.push(json!({"label": label, "url": parsed.as_str()}));
    }
    if out.len() > LINKS_MAX {
        return Err(Error::InvalidCode("ARTIST_LINK_TOO_MANY"));
    }
    Ok(Value::Array(out))
}

fn clean_contact(v: &str) -> Result<String> {
    let v = v.trim();
    if !v.is_empty() && (v.len() > 254 || !v.contains('@') || v.contains(char::is_whitespace)) {
        return Err(Error::InvalidCode("EMAIL_INVALID"));
    }
    Ok(v.to_string())
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
    let links = clean_links(&i.links)?;
    let contact = clean_contact(&i.contact)?;
    let handle = clean_handle(&i.handle)?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    if let Some(h) = &handle {
        let names = handle_names(h);
        crate::protected_names::enforce(&mut tx, org, &[&names[0], &names[1], &names[2]]).await?;
    }
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
            let next = handle.clone().unwrap_or(slug);
            sqlx::query(
                "UPDATE portal.artist_links SET enabled=$3, bio=$4, links=$5, contact=$6, slug=$7,
                        cheers_enabled=COALESCE($8, cheers_enabled), updated_at=now() WHERE org_id=$1 AND artist_id=$2",
            )
            .bind(org)
            .bind(artist)
            .bind(i.enabled)
            .bind(bio)
            .bind(&links)
            .bind(&contact)
            .bind(&next)
            .bind(i.cheers)
            .execute(&mut *tx)
            .await
            .map_err(taken)?;
            next
        }
        None if handle.is_some() => {
            let h = handle.clone().unwrap_or_default();
            sqlx::query(
                "INSERT INTO portal.artist_links(id, org_id, artist_id, slug, enabled, bio, created_by, links, contact, cheers_enabled)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
            )
            .bind(Uuid::new_v4())
            .bind(org)
            .bind(artist)
            .bind(&h)
            .bind(i.enabled)
            .bind(bio)
            .bind(a.user)
            .bind(&links)
            .bind(&contact)
            .bind(i.cheers.unwrap_or(true))
            .execute(&mut *tx)
            .await
            .map_err(taken)?;
            h
        }
        None => {
            let mut made = None;
            for _ in 0..5 {
                let slug = smart_links::new_slug()?;
                let ok: Option<String> = sqlx::query_scalar(
                    "INSERT INTO portal.artist_links(id, org_id, artist_id, slug, enabled, bio, created_by, links, contact, cheers_enabled)
                     VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT (slug) DO NOTHING RETURNING slug",
                )
                .bind(Uuid::new_v4())
                .bind(org)
                .bind(artist)
                .bind(&slug)
                .bind(i.enabled)
                .bind(bio)
                .bind(a.user)
                .bind(&links)
                .bind(&contact)
                .bind(i.cheers.unwrap_or(true))
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

/// Whether a handle can be chosen by this org's artist: `{handle, available,
/// reason?}` (reason = the error code saving it would give). The artist's own
/// current handle counts as available.
pub async fn check_handle(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    raw: &str,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let h = match clean_handle(raw) {
        Ok(Some(h)) => h,
        Ok(None) => {
            return Ok(
                json!({"handle": "", "available": false, "reason": "ARTIST_LINK_HANDLE_INVALID"}),
            );
        }
        Err(Error::InvalidCode(c)) => {
            return Ok(
                json!({"handle": raw.trim().to_ascii_lowercase(), "available": false, "reason": c}),
            );
        }
        Err(e) => return Err(e),
    };
    let names = handle_names(&h);
    if let Err(Error::PolicyGate(c)) =
        crate::protected_names::enforce(&mut tx, org, &[&names[0], &names[1], &names[2]]).await
    {
        return Ok(json!({"handle": h, "available": false, "reason": c}));
    }
    let owner: Option<(Uuid, Uuid)> =
        sqlx::query_as("SELECT org_id, artist_id FROM portal.artist_links WHERE slug=$1")
            .bind(&h)
            .fetch_optional(&mut *tx)
            .await?;
    tx.rollback().await?;
    Ok(match owner {
        Some(o) if o != (org, artist) => {
            json!({"handle": h, "available": false, "reason": "ARTIST_LINK_HANDLE_TAKEN"})
        }
        _ => json!({"handle": h, "available": true}),
    })
}

pub async fn public_page(s: &AppState, h: &HeaderMap, slug: &str) -> Result<Value> {
    if !valid_handle(slug) {
        return Err(Error::NotFound);
    }
    let row = sqlx::query(
        "SELECT k.id, k.org_id, k.artist_id, k.bio, k.links, k.contact, k.cheers_enabled, a.name
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
    let community =
        crate::artist_community::preview(s, row.get("id"), row.get("cheers_enabled")).await?;
    count(s, h, row.get("id"), slug).await;
    Ok(json!({
        "community": community,
        "name": row.get::<String, _>("name"),
        "bio": row.get::<String, _>("bio"),
        "links": row.get::<Value, _>("links"),
        "contact": row.get::<String, _>("contact"),
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
#[derive(Deserialize)]
pub struct HandleQuery {
    #[serde(default)]
    pub handle: String,
}
async fn h_check_handle(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    axum::extract::Query(q): axum::extract::Query<HandleQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(check_handle(&s, &a, org, id, &q.handle).await?))
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
        .route(
            "/api/orgs/{org}/artists/{id}/link/handle",
            get(h_check_handle),
        )
        .route("/api/artist-links/{slug}", get(h_public))
}
