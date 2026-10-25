//! Press kit (migration 0156): what the artist page's press page
//! (`/link/artist/{handle}/press`) hands curators, journalists and
//! promoters on top of the page itself.
//!
//! - The press profile: a one-line pitch, a short bio to quote as is, facts
//!   (genre, where the artist is based, label, debut year), up to
//!   [`HIGHLIGHTS_MAX`] highlights and [`QUOTES_MAX`] quotes from press or
//!   curators.
//! - Press releases the artist writes — a title, a lead paragraph and the
//!   body — optionally about one of the artist's submitted releases (the
//!   press page then links its smart link and cover). Drafts stay private;
//!   publishing shows them on the press page, newest first.
//!
//! Like the rest of the artist page, the press kit needs the page to exist
//! (`ARTIST_LINK_REQUIRED`) and disappears with it.
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
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

const HIGHLIGHTS_MAX: usize = 8;
const QUOTES_MAX: usize = 6;
const RELEASES_MAX: i64 = 50;

fn bad(code: &'static str) -> Error {
    Error::InvalidCode(code)
}

/// One line of text: trimmed, at most `max` characters, no control characters.
fn line(v: &str, max: usize, code: &'static str) -> Result<String> {
    let v = v.trim();
    if v.chars().count() > max {
        return Err(bad(code));
    }
    text_policy::check(v)?;
    Ok(v.to_string())
}

/// Several lines of text (bio, press release body).
fn text(v: &str, max: usize, code: &'static str) -> Result<String> {
    let v = v.trim();
    if v.chars().count() > max {
        return Err(bad(code));
    }
    text_policy::check_multiline(v)?;
    Ok(v.to_string())
}

async fn link_of(c: &mut PgConnection, org: Uuid, artist: Uuid, lock: bool) -> Result<Uuid> {
    let sql = if lock {
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

const EMPTY_PROFILE: &str = r#"{"tagline":"","bio_short":"","genre":"","based_in":"","label":"","debut":"","highlights":[],"quotes":[]}"#;

async fn profile(c: &mut PgConnection, link: Uuid) -> Result<Value> {
    let v: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('tagline', tagline, 'bio_short', bio_short, 'genre', genre, 'based_in', based_in,
                'label', label, 'debut', debut, 'highlights', highlights, 'quotes', quotes)
           FROM portal.artist_press WHERE link_id=$1",
    )
    .bind(link)
    .fetch_optional(&mut *c)
    .await?;
    Ok(v.unwrap_or_else(|| serde_json::from_str(EMPTY_PROFILE).unwrap_or(Value::Null)))
}

const RELEASE_JSON: &str = "jsonb_build_object('id', p.id, 'release_id', p.release_id,
    'release_title', (SELECT r.title FROM catalog.releases r WHERE r.id=p.release_id),
    'title', p.title, 'lead', p.lead, 'body', p.body, 'published', p.published, 'published_at', p.published_at,
    'created_at', p.created_at, 'updated_at', p.updated_at)";

// ---------------------------------------------------------------------------
// Org
// ---------------------------------------------------------------------------

/// The press profile and every press release (drafts too), newest first.
pub async fn kit(s: &AppState, a: &Actor, org: Uuid, artist: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let link = link_of(&mut tx, org, artist, false).await?;
    let p = profile(&mut tx, link).await?;
    let releases: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {RELEASE_JSON} FROM portal.artist_press_releases p WHERE p.link_id=$1 ORDER BY p.created_at DESC"
    )))
    .bind(link)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"profile": p, "releases": releases, "max": RELEASES_MAX}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quote {
    pub quote: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileInput {
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub bio_short: String,
    #[serde(default)]
    pub genre: String,
    #[serde(default)]
    pub based_in: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub debut: String,
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default)]
    pub quotes: Vec<Quote>,
}

pub async fn save_profile(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    i: ProfileInput,
) -> Result<Value> {
    let tagline = line(&i.tagline, 120, "PRESS_TAGLINE_TOO_LONG")?;
    let bio_short = text(&i.bio_short, 400, "PRESS_BIO_TOO_LONG")?;
    let genre = line(&i.genre, 60, "PRESS_FACT_TOO_LONG")?;
    let based_in = line(&i.based_in, 60, "PRESS_FACT_TOO_LONG")?;
    let label = line(&i.label, 80, "PRESS_FACT_TOO_LONG")?;
    let debut = i.debut.trim().to_string();
    let year_ok = debut.len() == 4
        && debut.chars().all(|c| c.is_ascii_digit())
        && (debut.starts_with("19") || debut.starts_with("20"));
    if !debut.is_empty() && !year_ok {
        return Err(bad("PRESS_DEBUT_INVALID"));
    }
    let mut highlights = Vec::new();
    for h in &i.highlights {
        let h = line(h, 140, "PRESS_HIGHLIGHT_TOO_LONG")?;
        if !h.is_empty() {
            highlights.push(h);
        }
    }
    if highlights.len() > HIGHLIGHTS_MAX {
        return Err(bad("PRESS_TOO_MANY"));
    }
    let mut quotes = Vec::new();
    for q in &i.quotes {
        let quote = text(&q.quote, 300, "PRESS_QUOTE_TOO_LONG")?;
        let source = line(&q.source, 80, "PRESS_QUOTE_TOO_LONG")?;
        if quote.is_empty() && source.is_empty() {
            continue;
        }
        if quote.is_empty() || source.is_empty() {
            return Err(bad("PRESS_QUOTE_SOURCE_REQUIRED"));
        }
        quotes.push(json!({"quote": quote, "source": source}));
    }
    if quotes.len() > QUOTES_MAX {
        return Err(bad("PRESS_TOO_MANY"));
    }
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = link_of(&mut tx, org, artist, true).await?;
    sqlx::query(
        "INSERT INTO portal.artist_press(link_id, tagline, bio_short, genre, based_in, label, debut, highlights, quotes, updated_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
         ON CONFLICT (link_id) DO UPDATE SET tagline=$2, bio_short=$3, genre=$4, based_in=$5, label=$6, debut=$7,
                highlights=$8, quotes=$9, updated_by=$10, updated_at=now()",
    )
    .bind(link)
    .bind(&tagline)
    .bind(&bio_short)
    .bind(&genre)
    .bind(&based_in)
    .bind(&label)
    .bind(&debut)
    .bind(json!(highlights))
    .bind(json!(quotes))
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    let p = profile(&mut tx, link).await?;
    tx.commit().await?;
    Ok(p)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseInput {
    #[serde(default)]
    pub release_id: Option<Uuid>,
    pub title: String,
    #[serde(default)]
    pub lead: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub published: bool,
}

struct CleanRelease {
    title: String,
    lead: String,
    body: String,
}

fn clean_release(i: &ReleaseInput) -> Result<CleanRelease> {
    let title = line(&i.title, 120, "PRESS_RELEASE_TITLE_INVALID")?;
    if title.is_empty() {
        return Err(bad("PRESS_RELEASE_TITLE_INVALID"));
    }
    Ok(CleanRelease {
        title,
        lead: text(&i.lead, 400, "PRESS_RELEASE_TOO_LONG")?,
        body: text(&i.body, 8000, "PRESS_RELEASE_TOO_LONG")?,
    })
}

/// The release a press release is about must be one of this artist's
/// submitted releases in the org.
async fn check_release(
    c: &mut PgConnection,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    release: Option<Uuid>,
) -> Result<()> {
    let Some(r) = release else { return Ok(()) };
    auth::authorize(&mut *c, a, org, r, "release", false).await?;
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.releases r WHERE r.org_id=$1 AND r.id=$2 AND r.archived_at IS NULL
                         AND r.status NOT IN ('DRAFT','SUPERSEDED')
                         AND EXISTS (SELECT 1 FROM catalog.tracks t WHERE t.org_id=r.org_id AND t.release_id=r.id
                                      AND t.artist_id=$3 AND t.archived_at IS NULL))",
    )
    .bind(org)
    .bind(r)
    .bind(artist)
    .fetch_one(&mut *c)
    .await?;
    if !ok {
        return Err(bad("PRESS_RELEASE_RELEASE_INVALID"));
    }
    Ok(())
}

pub async fn create_release(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    i: ReleaseInput,
) -> Result<Value> {
    let c = clean_release(&i)?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = link_of(&mut tx, org, artist, true).await?;
    check_release(&mut tx, a, org, artist, i.release_id).await?;
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM portal.artist_press_releases WHERE link_id=$1")
            .bind(link)
            .fetch_one(&mut *tx)
            .await?;
    if n >= RELEASES_MAX {
        return Err(bad("PRESS_RELEASE_TOO_MANY"));
    }
    let id = Uuid::new_v4();
    let item: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "WITH p AS (INSERT INTO portal.artist_press_releases(id, link_id, release_id, title, lead, body, published, published_at, created_by)
                    VALUES($1,$2,$3,$4,$5,$6,$7, CASE WHEN $7 THEN now() END, $8) RETURNING *)
         SELECT {RELEASE_JSON} FROM p"
    )))
    .bind(id)
    .bind(link)
    .bind(i.release_id)
    .bind(&c.title)
    .bind(&c.lead)
    .bind(&c.body)
    .bind(i.published)
    .bind(a.user)
    .fetch_one(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(artist),
        "artist.press_release_saved",
        if i.published { "PUBLISHED" } else { "DRAFT" },
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(item)
}

pub async fn update_release(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    id: Uuid,
    i: ReleaseInput,
) -> Result<Value> {
    let c = clean_release(&i)?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = link_of(&mut tx, org, artist, true).await?;
    check_release(&mut tx, a, org, artist, i.release_id).await?;
    // Publishing keeps the first publish time; taking it back clears it.
    let item: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "WITH p AS (UPDATE portal.artist_press_releases
                       SET release_id=$3, title=$4, lead=$5, body=$6, published=$7,
                           published_at=CASE WHEN $7 THEN COALESCE(published_at, now()) END, updated_at=now()
                     WHERE id=$1 AND link_id=$2 RETURNING *)
         SELECT {RELEASE_JSON} FROM p"
    )))
    .bind(id)
    .bind(link)
    .bind(i.release_id)
    .bind(&c.title)
    .bind(&c.lead)
    .bind(&c.body)
    .bind(i.published)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(artist),
        "artist.press_release_saved",
        if i.published { "PUBLISHED" } else { "DRAFT" },
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(item)
}

pub async fn delete_release(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    id: Uuid,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let link = link_of(&mut tx, org, artist, true).await?;
    let n = sqlx::query("DELETE FROM portal.artist_press_releases WHERE id=$1 AND link_id=$2")
        .bind(id)
        .bind(link)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    tx.commit().await?;
    Ok(json!({"deleted": true}))
}

// ---------------------------------------------------------------------------
// Public
// ---------------------------------------------------------------------------

/// The press kit of an open page: profile and published press releases, each
/// with its release's smart link while that link is visible.
pub async fn public(s: &AppState, slug: &str) -> Result<Value> {
    if !crate::artist_links::valid_handle(slug) {
        return Err(Error::NotFound);
    }
    let mut c = s.pool.acquire().await?;
    let link: Uuid = sqlx::query_scalar(
        "SELECT k.id FROM portal.artist_links k JOIN catalog.artists a ON a.org_id=k.org_id AND a.id=k.artist_id
          WHERE k.slug=$1 AND k.enabled AND a.archived_at IS NULL",
    )
    .bind(slug)
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    let p = profile(&mut c, link).await?;
    let hidden: Vec<String> = HIDDEN_STATES.iter().map(|v| v.to_string()).collect();
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT p.id, p.title, p.lead, p.body, p.published_at,
                l.slug AS link_slug, r.title AS release_title, r.artwork_asset_id IS NOT NULL AS cover
           FROM portal.artist_press_releases p
           LEFT JOIN catalog.releases r ON r.id=p.release_id
           LEFT JOIN portal.smart_links l ON l.release_id=r.id AND l.org_id=r.org_id AND {}
          WHERE p.link_id=$1 AND p.published
          ORDER BY p.published_at DESC LIMIT 50",
        smart_links::visible_sql("$2")
    )))
    .bind(link)
    .bind(&hidden)
    .fetch_all(&mut *c)
    .await?;
    let releases: Vec<Value> = rows
        .iter()
        .map(|r| {
            let slug: Option<String> = r.get("link_slug");
            json!({
                "id": r.get::<Uuid, _>("id"),
                "title": r.get::<String, _>("title"),
                "lead": r.get::<String, _>("lead"),
                "body": r.get::<String, _>("body"),
                "published_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("published_at"),
                "release": slug.map(|slug| json!({
                    "slug": slug,
                    "title": r.get::<Option<String>, _>("release_title"),
                    "cover": r.get::<Option<bool>, _>("cover").unwrap_or(false),
                })),
            })
        })
        .collect();
    Ok(json!({"profile": p, "releases": releases}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_get(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(kit(&s, &a, org, id).await?))
}
async fn h_save(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<ProfileInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(save_profile(&s, &a, org, id, i).await?))
}
async fn h_create(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<ReleaseInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(create_release(&s, &a, org, id, i).await?))
}
async fn h_update(
    State(s): State<AppState>,
    Path((org, id, pr)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<ReleaseInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(update_release(&s, &a, org, id, pr, i).await?))
}
async fn h_delete(
    State(s): State<AppState>,
    Path((org, id, pr)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(delete_release(&s, &a, org, id, pr).await?))
}
async fn h_public(State(s): State<AppState>, Path(slug): Path<String>) -> Result<Json<Value>> {
    Ok(Json(public(&s, &slug).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/artists/{id}/link/press",
            get(h_get).put(h_save),
        )
        .route(
            "/api/orgs/{org}/artists/{id}/link/press/releases",
            axum::routing::post(h_create),
        )
        .route(
            "/api/orgs/{org}/artists/{id}/link/press/releases/{pr}",
            put(h_update).delete(h_delete),
        )
        .route("/api/artist-links/{slug}/press", get(h_public))
}
