//! Artist platform pages (migration 0122): each artist's own page on
//! Spotify, Apple Music, Melon, ... recorded once and reused.
//!
//! - At submission ([`record_from_application`]): links the artist gave
//!   ("already released") are LINKED; a first release ("new page") leaves
//!   PENDING rows for Spotify, Apple Music and Melon until the links are known.
//! - The artist (or staff) registers the links once the page exists
//!   ([`set_pages`]).
//! - The next application for the same artist is prefilled ([`pages`]), and
//!   deliveries name the page to that platform ([`platform_ids`] → DDEX
//!   DisplayArtist/PartyId), so a release never lands on a same-name stranger.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    staff::{self, Duty},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, put},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::{PgConnection, Row};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Platforms we keep pages for, with their hosts.
pub const PLATFORMS: &[(&str, &[&str])] = &[
    ("spotify", &["open.spotify.com"]),
    ("apple", &["music.apple.com"]),
    ("melon", &["melon.com"]),
    ("genie", &["genie.co.kr"]),
    ("flo", &["music-flo.com"]),
    ("bugs", &["bugs.co.kr"]),
    ("vibe", &["vibe.naver.com"]),
    ("youtube", &["youtube.com", "music.youtube.com"]),
];
/// A new artist's first release creates pages here (the studio asks for these).
const NEW_PAGE_PLATFORMS: &[&str] = &["spotify", "apple", "melon"];

/// The platform's own artist id in a page link, when the link shape is known.
pub fn external_id(platform: &str, url: &url::Url) -> Option<String> {
    let segs: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    match platform {
        // https://open.spotify.com/artist/{id}
        "spotify" => segs
            .iter()
            .position(|s| *s == "artist")
            .and_then(|i| segs.get(i + 1))
            .filter(|id| id.len() == 22 && id.chars().all(|c| c.is_ascii_alphanumeric()))
            .map(|id| format!("spotify:artist:{id}")),
        // https://music.apple.com/kr/artist/{name}/{id}
        "apple" => segs
            .iter()
            .position(|s| *s == "artist")
            .and_then(|_| segs.last())
            .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
            .map(|id| id.to_string()),
        // https://www.melon.com/artist/detail.htm?artistId={n}
        "melon" => url
            .query_pairs()
            .find(|(k, _)| k == "artistId")
            .map(|(_, v)| v.to_string())
            .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit())),
        _ => None,
    }
}

/// A checked page link: https on the platform's hosts, no credentials/port.
pub fn check_link(platform: &str, raw: &str) -> Result<(String, Option<String>)> {
    let bad = || Error::InvalidCode("ARTIST_PAGE_URL_INVALID");
    let hosts = PLATFORMS
        .iter()
        .find(|(p, _)| *p == platform)
        .map(|(_, h)| *h)
        .ok_or(Error::InvalidCode("ARTIST_PAGE_PLATFORM_INVALID"))?;
    let raw = raw.trim();
    if raw.len() > 500 {
        return Err(bad());
    }
    let u = url::Url::parse(raw).map_err(|_| bad())?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return Err(bad());
    }
    let host = u.host_str().ok_or_else(bad)?.to_ascii_lowercase();
    if !hosts
        .iter()
        .any(|h| host == *h || host.ends_with(&format!(".{h}")))
    {
        return Err(bad());
    }
    let id = external_id(platform, &u);
    Ok((u.to_string(), id))
}

async fn artist_by_name(c: &mut PgConnection, org: Uuid, name: &str) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM catalog.artists WHERE org_id=$1 AND lower(btrim(name))=lower(btrim($2)) AND archived_at IS NULL
          ORDER BY id LIMIT 1",
    )
    .bind(org)
    .bind(name)
    .fetch_optional(&mut *c)
    .await?)
}

#[allow(clippy::too_many_arguments)]
async fn upsert_link(
    c: &mut PgConnection,
    org: Uuid,
    artist: Uuid,
    platform: &str,
    url: &str,
    id: Option<&str>,
    source: &str,
    user: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO catalog.artist_platform_pages(org_id, artist_id, platform, url, external_id, status, source, updated_by)
         VALUES($1,$2,$3,$4,$5,'LINKED',$6,$7)
         ON CONFLICT (org_id, artist_id, platform) DO UPDATE
           SET url=$4, external_id=$5, status='LINKED', source=$6, updated_by=$7, updated_at=now()",
    )
    .bind(org)
    .bind(artist)
    .bind(platform)
    .bind(url)
    .bind(id)
    .bind(source)
    .bind(user)
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// Record what the frozen application says about the artist's pages. Never
/// fails a submission: a link that does not check is simply not recorded.
pub async fn record_from_application(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
    draft: &Value,
    user: Uuid,
) -> Result<()> {
    let Some(name) = draft
        .get("artist")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
    else {
        return Ok(());
    };
    let Some(artist) = artist_by_name(c, org, name).await? else {
        return Ok(());
    };
    let profile = draft.get("artistProfile");
    let is_new = profile
        .and_then(|p| p.get("isNew"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if is_new {
        for p in NEW_PAGE_PLATFORMS {
            sqlx::query(
                "INSERT INTO catalog.artist_platform_pages(org_id, artist_id, platform, status, source, first_release_id, updated_by)
                 VALUES($1,$2,$3,'PENDING','APPLICATION',$4,$5) ON CONFLICT DO NOTHING",
            )
            .bind(org)
            .bind(artist)
            .bind(p)
            .bind(release)
            .bind(user)
            .execute(&mut *c)
            .await?;
        }
        return Ok(());
    }
    for (platform, _) in PLATFORMS {
        let Some(raw) = profile
            .and_then(|p| p.get(*platform))
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
        else {
            continue;
        };
        if let Ok((url, id)) = check_link(platform, raw) {
            upsert_link(
                c,
                org,
                artist,
                platform,
                &url,
                id.as_deref(),
                "APPLICATION",
                Some(user),
            )
            .await?;
        }
    }
    Ok(())
}

/// Platform slug → the artist's id there, for deliveries of `artist_name`.
pub async fn platform_ids(
    c: &mut PgConnection,
    org: Uuid,
    artist_name: &str,
) -> Result<BTreeMap<String, String>> {
    let rows = sqlx::query(
        "SELECT p.platform, p.external_id FROM catalog.artist_platform_pages p
           JOIN catalog.artists a ON a.org_id=p.org_id AND a.id=p.artist_id
          WHERE p.org_id=$1 AND lower(btrim(a.name))=lower(btrim($2)) AND p.status='LINKED' AND p.external_id IS NOT NULL",
    )
    .bind(org)
    .bind(artist_name)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.get("platform"), r.get("external_id")))
        .collect())
}

fn page_json(rows: Vec<sqlx::postgres::PgRow>) -> Value {
    let mut out = Map::new();
    for r in rows {
        out.insert(
            r.get::<String, _>("platform"),
            json!({"url": r.get::<Option<String>, _>("url"), "status": r.get::<String, _>("status"),
                   "first_release_id": r.get::<Option<Uuid>, _>("first_release_id")}),
        );
    }
    Value::Object(out)
}

#[derive(Deserialize)]
pub struct PagesQuery {
    pub name: String,
}

/// What the studio prefills for an artist name: known pages and whether a
/// first release is still waiting for its new pages to be registered.
pub async fn pages(s: &AppState, a: &Actor, org: Uuid, q: PagesQuery) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let Some(artist) = artist_by_name(&mut tx, org, &q.name).await? else {
        tx.rollback().await?;
        return Ok(json!({"artist_id": null, "pages": {}}));
    };
    let rows = sqlx::query(
        "SELECT platform, url, status, first_release_id FROM catalog.artist_platform_pages WHERE org_id=$1 AND artist_id=$2",
    )
    .bind(org)
    .bind(artist)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"artist_id": artist, "pages": page_json(rows)}))
}

/// Every artist of the org with their pages (the artist pages screen).
pub async fn list(s: &AppState, a: &Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('artist_id', a.id, 'name', a.name,
                'pages', COALESCE((SELECT jsonb_object_agg(p.platform, jsonb_build_object('url',p.url,'status',p.status,
                                     'first_release_id',p.first_release_id,'source',p.source,'updated_at',p.updated_at))
                                   FROM catalog.artist_platform_pages p WHERE p.org_id=a.org_id AND p.artist_id=a.id), '{}'::jsonb),
                'releases', (SELECT count(DISTINCT t.release_id) FROM catalog.tracks t WHERE t.org_id=a.org_id AND t.artist_id=a.id AND t.archived_at IS NULL))
           FROM catalog.artists a WHERE a.org_id=$1 AND a.archived_at IS NULL ORDER BY a.name LIMIT 500",
    )
    .bind(org)
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagesInput {
    /// platform → link; an empty link leaves that platform as it is.
    pub links: BTreeMap<String, String>,
}

async fn set_in(
    c: &mut PgConnection,
    org: Uuid,
    artist: Uuid,
    i: &PagesInput,
    source: &str,
    user: Uuid,
) -> Result<usize> {
    let mut n = 0;
    for (platform, raw) in &i.links {
        if raw.trim().is_empty() {
            continue;
        }
        let (url, id) = check_link(platform, raw)?;
        upsert_link(
            c,
            org,
            artist,
            platform,
            &url,
            id.as_deref(),
            source,
            Some(user),
        )
        .await?;
        n += 1;
    }
    Ok(n)
}

/// The artist registers (or corrects) their pages.
pub async fn set_pages(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    artist: Uuid,
    i: PagesInput,
) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, artist, "artist", true).await?;
    let n = set_in(&mut tx, org, artist, &i, "ARTIST", a.user).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(artist),
        "artist.pages_set",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"artist_id": artist, "saved": n}))
}

/// Staff set the pages (after a platform fixed a mismatch, say).
pub async fn staff_set_pages(
    s: &AppState,
    h: &HeaderMap,
    artist: Uuid,
    i: PagesInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM catalog.artists WHERE id=$1")
        .bind(artist)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    let n = set_in(&mut tx, org, artist, &i, "STAFF", st.actor.user).await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(artist),
        "staff.artist_pages_set",
        "STAFF",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"artist_id": artist, "saved": n}))
}

async fn h_pages(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    Query(q): Query<PagesQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(pages(&s, &a, org, q).await?))
}
async fn h_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list(&s, &a, org).await?))
}
async fn h_set(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<PagesInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(set_pages(&s, &a, org, id, i).await?))
}
async fn h_staff_set(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<PagesInput>,
) -> Result<Json<Value>> {
    Ok(Json(staff_set_pages(&s, &h, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/orgs/{org}/artist-pages", get(h_pages))
        .route("/api/orgs/{org}/artist-pages/all", get(h_list))
        .route("/api/orgs/{org}/artists/{id}/pages", put(h_set))
        .route("/api/staff/artists/{id}/pages", put(h_staff_set))
}

#[cfg(test)]
mod tests {
    use super::check_link;

    #[test]
    fn links_checked_and_ids_parsed() {
        let (_, id) = check_link(
            "spotify",
            "https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb?si=x",
        )
        .unwrap();
        assert_eq!(id.as_deref(), Some("spotify:artist:4Z8W4fKeB5YxbusRsdQVPb"));
        let (_, id) = check_link("apple", "https://music.apple.com/kr/artist/iu/1234567").unwrap();
        assert_eq!(id.as_deref(), Some("1234567"));
        let (_, id) = check_link(
            "melon",
            "https://www.melon.com/artist/detail.htm?artistId=261143",
        )
        .unwrap();
        assert_eq!(id.as_deref(), Some("261143"));
        let (_, id) =
            check_link("genie", "https://www.genie.co.kr/detail/artistInfo?xxnm=1").unwrap();
        assert_eq!(id, None);
        assert!(check_link("spotify", "https://evil.example/artist/x").is_err());
        assert!(
            check_link(
                "spotify",
                "http://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb"
            )
            .is_err()
        );
        assert!(check_link("myspace", "https://myspace.com/x").is_err());
    }
}
