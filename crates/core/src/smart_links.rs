//! Smart links (migration 0117).
//!
//! One public landing page per submitted release, `/l/{slug}`: cover,
//! title, artist, release date and the platform links the artist pasted
//! (each checked against that platform's hosts, https only). Where a
//! platform reported its own album id for the release when it went live
//! (`execution.live_bindings.partner_release_id`), the server fills that
//! platform's link itself; a link the artist pastes wins. Pasted links are
//! read for the album/track they open and compared with the reported id;
//! before the official launch that is only shown, after it (`strict`) a
//! link that is not a release page or opens another album is refused.
//! Anyone can open
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

/// `strict` turns release checks on early, `test` keeps them off after
/// launch. Unset follows the launch switch ([`crate::launch`]): testers can
/// point a page at any song until the official launch.
pub const VERIFY_ENV: &str = "SMART_LINK_VERIFY";

/// Are pasted links refused when they are not this release's page?
pub fn strict() -> bool {
    match std::env::var(VERIFY_ENV).as_deref().map(str::trim) {
        Ok("strict") => true,
        Ok("test") => false,
        _ => crate::launch::live_transmission_enabled(),
    }
}
pub(crate) const STATS_DAYS: i32 = 30;

/// Release states without a page.
pub(crate) const HIDDEN_STATES: &[&str] = &["DRAFT", "WITHDRAWN", "SUPERSEDED", "ON_HOLD_RIGHTS"];

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Album,
    Track,
}

/// The album or track a platform link opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseRef {
    pub kind: Kind,
    pub id: String,
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}
fn spotify_id(s: &str) -> bool {
    s.len() == 22 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}
fn asin(s: &str) -> bool {
    s.len() == 10
        && s.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
/// YouTube Music albums are playlists (`OLAK5uy_…`) or browse pages (`MPREb_…`).
fn youtube_album(s: &str) -> bool {
    (s.starts_with("OLAK5uy_") || s.starts_with("MPREb_"))
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn youtube_video(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// What a link opens, for the link shapes each platform uses. `None` for
/// artist pages, playlists, searches and short links (spotify.link, kko.to,
/// …), which cannot be read without following them.
pub(crate) fn release_ref(platform: &str, u: &url::Url) -> Option<ReleaseRef> {
    let segs: Vec<&str> = u
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let query = |k: &str| {
        u.query_pairs()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.into_owned())
    };
    let after = |name: &str| {
        segs.iter()
            .position(|s| *s == name)
            .and_then(|i| segs.get(i + 1))
            .map(|s| s.to_string())
    };
    let found = |kind, id: Option<String>, ok: fn(&str) -> bool| {
        id.filter(|i| ok(i)).map(|id| ReleaseRef { kind, id })
    };
    let by_path = |ok: fn(&str) -> bool| {
        found(Kind::Album, after("album"), ok).or_else(|| found(Kind::Track, after("track"), ok))
    };
    match platform {
        // https://open.spotify.com/(intl-ko/)album/{id}, …/track/{id}
        "spotify" => by_path(spotify_id),
        // https://music.apple.com/kr/album/{name}/{id}(?i={track}), …/song/{name}/{id}
        "apple" => {
            let last = segs.last().map(|s| s.to_string());
            if segs.contains(&"album") {
                found(Kind::Track, query("i"), digits).or_else(|| found(Kind::Album, last, digits))
            } else if segs.contains(&"song") {
                found(Kind::Track, last, digits)
            } else {
                None
            }
        }
        // https://www.melon.com/album/detail.htm?albumId={n}, …/song/detail.htm?songId={n}
        "melon" => found(Kind::Album, query("albumId"), digits)
            .or_else(|| found(Kind::Track, query("songId"), digits)),
        // https://www.genie.co.kr/detail/albumInfo?axnm={n}, …/songInfo?xgnm={n}
        "genie" => found(Kind::Album, query("axnm"), digits)
            .or_else(|| found(Kind::Track, query("xgnm"), digits)),
        "flo" | "bugs" | "vibe" | "deezer" | "tidal" => by_path(digits),
        // https://music.amazon.com/albums/{ASIN}(?trackAsin={ASIN})
        "amazon" => found(Kind::Track, query("trackAsin"), asin)
            .or_else(|| found(Kind::Album, after("albums"), asin)),
        // https://music.youtube.com/playlist?list=OLAK5uy_…, …/browse/MPREb_…, …/watch?v={id}
        "youtube" => found(Kind::Album, query("list"), youtube_album)
            .or_else(|| found(Kind::Album, after("browse"), youtube_album))
            .or_else(|| found(Kind::Track, query("v"), youtube_video)),
        _ => None,
    }
}

/// The album page for a platform's own id, when the id has that platform's shape.
pub(crate) fn album_url(platform: &str, id: &str) -> Option<String> {
    let ok = match platform {
        "spotify" => spotify_id(id),
        "amazon" => asin(id),
        "youtube" => youtube_album(id),
        _ => digits(id),
    };
    if !ok {
        return None;
    }
    Some(match platform {
        "melon" => format!("https://www.melon.com/album/detail.htm?albumId={id}"),
        "genie" => format!("https://www.genie.co.kr/detail/albumInfo?axnm={id}"),
        "flo" => format!("https://www.music-flo.com/detail/album/{id}/albumtrack"),
        "bugs" => format!("https://music.bugs.co.kr/album/{id}"),
        "vibe" => format!("https://vibe.naver.com/album/{id}"),
        "spotify" => format!("https://open.spotify.com/album/{id}"),
        "apple" => format!("https://music.apple.com/kr/album/{id}"),
        "youtube" if id.starts_with("OLAK5uy_") => {
            format!("https://music.youtube.com/playlist?list={id}")
        }
        "youtube" => format!("https://music.youtube.com/browse/{id}"),
        "amazon" => format!("https://music.amazon.com/albums/{id}"),
        "deezer" => format!("https://www.deezer.com/album/{id}"),
        "tidal" => format!("https://tidal.com/album/{id}"),
        _ => return None,
    })
}

/// Album ids the platforms reported for this release when it went live,
/// by smart link platform. Ids may come as URIs (`spotify:album:{id}`).
async fn known_ids(
    c: &mut sqlx::PgConnection,
    org: Uuid,
    release: Uuid,
) -> Result<BTreeMap<&'static str, String>> {
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *c)
        .await?;
    let rows = sqlx::query(
        "SELECT DISTINCT ON (b.partner_id) b.partner_id, b.partner_release_id
           FROM distribution.canonical_releases cr
           JOIN distribution.distribution_packages dp ON dp.canonical_release_id=cr.id
           JOIN execution.live_bindings b ON b.org_id=cr.org_id AND b.package_id=dp.id
          WHERE cr.org_id=$1 AND cr.release_id=$2 AND b.live_status='LIVE' AND b.partner_release_id IS NOT NULL
          ORDER BY b.partner_id, b.updated_at DESC",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *c)
    .await?;
    let mut out = BTreeMap::new();
    for r in rows {
        let Some(dsp) = crate::dsp_registry::Dsp::from_code(r.get::<&str, _>("partner_id")) else {
            continue;
        };
        let Some(platform) = PLATFORMS
            .iter()
            .map(|(p, _)| *p)
            .find(|p| *p == dsp.spec().slug)
        else {
            continue;
        };
        let raw: &str = r.get("partner_release_id");
        let id = raw.rsplit(':').next().unwrap_or_default().trim();
        if album_url(platform, id).is_some() {
            out.insert(platform, id.to_string());
        }
    }
    Ok(out)
}

/// Links the server fills itself: the album page for every reported id.
fn auto_links(known: &BTreeMap<&'static str, String>) -> Map<String, Value> {
    known
        .iter()
        .filter_map(|(p, id)| album_url(p, id).map(|u| (p.to_string(), json!(u))))
        .collect()
}

/// How a pasted (already host-checked) link relates to this release:
/// `verified` the reported album, `mismatch` another album, `unverified`
/// a release page we cannot compare yet (no id reported, or a track),
/// `not_release` anything else (artist page, playlist, short link).
fn verdict(platform: &str, link: &str, known: Option<&String>) -> &'static str {
    let Some(r) = url::Url::parse(link)
        .ok()
        .and_then(|u| release_ref(platform, &u))
    else {
        return "not_release";
    };
    match (known, r.kind) {
        (Some(k), Kind::Album) if *k == r.id => "verified",
        (Some(_), Kind::Album) => "mismatch",
        _ => "unverified",
    }
}

fn verdicts(
    links: &Map<String, Value>,
    known: &BTreeMap<&'static str, String>,
) -> Map<String, Value> {
    links
        .iter()
        .filter_map(|(p, u)| {
            let u = u.as_str()?;
            Some((p.clone(), json!(verdict(p, u, known.get(p.as_str())))))
        })
        .collect()
}

/// After launch: a link must open a release page, and the reported album
/// when the platform reported one.
fn refuse_unverifiable(checks: &Map<String, Value>) -> Result<()> {
    for v in checks.values() {
        match v.as_str() {
            Some("not_release") => return Err(Error::InvalidCode("SMART_LINK_URL_NOT_RELEASE")),
            Some("mismatch") => return Err(Error::InvalidCode("SMART_LINK_URL_MISMATCH")),
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn new_slug() -> Result<String> {
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

pub(crate) fn valid_slug(slug: &str) -> bool {
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
    let known = known_ids(&mut tx, org, release).await?;
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
    let checks = link
        .as_ref()
        .and_then(|l| {
            l.get::<Value, _>("links")
                .as_object()
                .map(|m| verdicts(m, &known))
        })
        .unwrap_or_default();
    Ok(json!({
        "link": link.map(|l| json!({"slug": l.get::<String, _>("slug"), "enabled": l.get::<bool, _>("enabled"), "links": l.get::<Value, _>("links")})),
        "auto": auto_links(&known),
        "checks": checks,
        "verify": if strict() { "strict" } else { "test" },
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
    if strict() {
        let known = known_ids(&mut tx, org, release).await?;
        refuse_unverifiable(&verdicts(&links, &known))?;
    }
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckInput {
    #[serde(default)]
    pub links: BTreeMap<String, String>,
}

/// Check links without saving, one verdict per platform: the host check
/// (`invalid`) and, for links that pass it, how they relate to this release.
/// The studio shows these next to each field as the artist types.
pub async fn check(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: CheckInput,
) -> Result<Value> {
    if i.links.len() > PLATFORMS.len() {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let known = known_ids(&mut tx, org, release).await?;
    tx.commit().await?;
    let mut out = Map::new();
    for (platform, raw) in &i.links {
        if raw.trim().is_empty() || hosts(platform).is_none() {
            continue;
        }
        let v = match check_link(platform, raw) {
            Ok(u) => verdict(platform, &u, known.get(platform.as_str())),
            Err(_) => "invalid",
        };
        out.insert(platform.clone(), json!(v));
    }
    Ok(json!({"checks": out, "verify": if strict() { "strict" } else { "test" }}))
}

// ---------------------------------------------------------------------------
// Public
// ---------------------------------------------------------------------------

/// The links the page shows: pasted ones, and the server's for platforms
/// the artist left empty.
async fn page_links(s: &AppState, row: &sqlx::postgres::PgRow) -> Result<Value> {
    let mut links = row.get::<Value, _>("links");
    let mut tx = s.pool.begin().await?;
    let known = known_ids(&mut tx, row.get("org_id"), row.get("release_id")).await?;
    tx.commit().await?;
    if let Some(m) = links.as_object_mut() {
        for (p, u) in auto_links(&known) {
            m.entry(p).or_insert(u);
        }
    }
    Ok(links)
}

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

/// Conditions under which link `l` on release `r` may be shown publicly;
/// `hidden` is the parameter holding [`HIDDEN_STATES`]. Shared with the
/// artist page, which lists the same links.
pub(crate) fn visible_sql(hidden: &str) -> String {
    format!(
        "l.enabled AND r.archived_at IS NULL AND NOT (r.status = ANY({hidden}))
            AND NOT EXISTS (SELECT 1 FROM portal.takedown_requests t WHERE t.release_id=r.id AND t.status='APPROVED')
            AND NOT EXISTS (SELECT 1 FROM portal.infringement_claims c WHERE c.release_id=r.id
                             AND c.status IN ('SUSPENDED','COUNTER_NOTICED','UPHELD'))
            AND NOT EXISTS (SELECT 1 FROM portal.release_terms tm WHERE tm.release_id=r.id AND tm.status IN ('ENDED','FINAL_SETTLED'))"
    )
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
        "SELECT l.id, l.slug, l.org_id, l.release_id, l.links, r.title, r.draft->>'artist' AS artist, r.draft->>'release_date' AS release_date,
                r.draft->>'release_time' AS release_time, r.artwork_asset_id
           FROM portal.smart_links l
           JOIN catalog.releases r ON r.id=l.release_id AND r.org_id=l.org_id
          WHERE {key} AND {}",
        visible_sql("$2")
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
    let links = page_links(s, &row).await?;
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
    let links = page_links(s, &row).await?;
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
async fn h_check(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<CheckInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(check(&s, &a, org, id, i).await?))
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
        .route(
            "/api/orgs/{org}/releases/{id}/smart-link/check",
            post(h_check),
        )
        .route("/api/smart-links/{slug}", get(h_public))
        .route("/api/smart-links/{slug}/cover", get(h_cover))
        .route("/api/smart-links/{slug}/click", post(h_click))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(platform: &str, link: &str) -> Option<(Kind, String)> {
        release_ref(platform, &url::Url::parse(link).unwrap()).map(|r| (r.kind, r.id))
    }

    #[test]
    fn release_pages_are_read_per_platform() {
        let sp = "4aawyAB9vmqN3uQ7FjRGTy";
        assert_eq!(
            r(
                "spotify",
                &format!("https://open.spotify.com/album/{sp}?si=x")
            ),
            Some((Kind::Album, sp.into()))
        );
        assert_eq!(
            r(
                "spotify",
                &format!("https://open.spotify.com/intl-ko/track/{sp}")
            ),
            Some((Kind::Track, sp.into()))
        );
        assert_eq!(
            r("apple", "https://music.apple.com/kr/album/name/1440857781"),
            Some((Kind::Album, "1440857781".into()))
        );
        assert_eq!(
            r(
                "apple",
                "https://music.apple.com/kr/album/name/1440857781?i=1440857795"
            ),
            Some((Kind::Track, "1440857795".into()))
        );
        assert_eq!(
            r(
                "melon",
                "https://www.melon.com/album/detail.htm?albumId=11205713"
            ),
            Some((Kind::Album, "11205713".into()))
        );
        assert_eq!(
            r(
                "melon",
                "https://www.melon.com/song/detail.htm?songId=3591324"
            ),
            Some((Kind::Track, "3591324".into()))
        );
        assert_eq!(
            r(
                "genie",
                "https://www.genie.co.kr/detail/albumInfo?axnm=81234567"
            ),
            Some((Kind::Album, "81234567".into()))
        );
        assert_eq!(
            r(
                "flo",
                "https://www.music-flo.com/detail/album/412345678/albumtrack"
            ),
            Some((Kind::Album, "412345678".into()))
        );
        assert_eq!(
            r("bugs", "https://music.bugs.co.kr/track/6112345"),
            Some((Kind::Track, "6112345".into()))
        );
        assert_eq!(
            r("deezer", "https://www.deezer.com/kr/album/302127"),
            Some((Kind::Album, "302127".into()))
        );
        assert_eq!(
            r("amazon", "https://music.amazon.com/albums/B0ABCDEF12"),
            Some((Kind::Album, "B0ABCDEF12".into()))
        );
        assert_eq!(
            r(
                "youtube",
                "https://music.youtube.com/playlist?list=OLAK5uy_kAbc-123"
            ),
            Some((Kind::Album, "OLAK5uy_kAbc-123".into()))
        );
        assert_eq!(
            r("youtube", "https://music.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some((Kind::Track, "dQw4w9WgXcQ".into()))
        );
    }

    #[test]
    fn other_pages_are_not_releases() {
        for (p, link) in [
            (
                "spotify",
                "https://open.spotify.com/artist/4aawyAB9vmqN3uQ7FjRGTy",
            ),
            ("spotify", "https://spotify.link/abcdef"),
            ("spotify", "https://open.spotify.com/album/short"),
            ("apple", "https://music.apple.com/kr/artist/iu/1234567"),
            (
                "melon",
                "https://www.melon.com/artist/detail.htm?artistId=1",
            ),
            ("melon", "https://kko.to/abc"),
            ("youtube", "https://music.youtube.com/playlist?list=PL123"),
            ("bugs", "https://music.bugs.co.kr/album/12a"),
        ] {
            assert_eq!(r(p, link), None, "{p} {link}");
        }
    }

    #[test]
    fn album_urls_round_trip_and_reject_bad_ids() {
        for (p, id) in [
            ("melon", "11205713"),
            ("genie", "81234567"),
            ("flo", "412345678"),
            ("bugs", "20412345"),
            ("vibe", "1234567"),
            ("spotify", "4aawyAB9vmqN3uQ7FjRGTy"),
            ("apple", "1440857781"),
            ("youtube", "OLAK5uy_kAbc-123"),
            ("youtube", "MPREb_abc123"),
            ("amazon", "B0ABCDEF12"),
            ("deezer", "302127"),
            ("tidal", "77646168"),
        ] {
            let u = album_url(p, id).unwrap_or_else(|| panic!("{p} {id}"));
            assert!(check_link(p, &u).is_ok(), "{u}");
            assert_eq!(r(p, &u), Some((Kind::Album, id.to_string())), "{u}");
        }
        assert_eq!(album_url("melon", "1&x=2"), None);
        assert_eq!(album_url("spotify", "../../x"), None);
        assert_eq!(album_url("myspace", "1"), None);
    }

    #[test]
    fn verdicts_compare_with_the_reported_album() {
        let id = "11205713".to_string();
        let album = "https://www.melon.com/album/detail.htm?albumId=11205713";
        let other = "https://www.melon.com/album/detail.htm?albumId=99";
        let song = "https://www.melon.com/song/detail.htm?songId=3591324";
        let artist = "https://www.melon.com/artist/detail.htm?artistId=1";
        assert_eq!(verdict("melon", album, Some(&id)), "verified");
        assert_eq!(verdict("melon", other, Some(&id)), "mismatch");
        assert_eq!(verdict("melon", song, Some(&id)), "unverified");
        assert_eq!(verdict("melon", album, None), "unverified");
        assert_eq!(verdict("melon", artist, None), "not_release");
    }

    #[test]
    fn strict_mode_refuses_other_albums_and_non_release_pages() {
        let m = |v: &str| Map::from_iter([("melon".to_string(), json!(v))]);
        assert!(refuse_unverifiable(&m("verified")).is_ok());
        assert!(refuse_unverifiable(&m("unverified")).is_ok());
        assert!(matches!(
            refuse_unverifiable(&m("mismatch")),
            Err(Error::InvalidCode("SMART_LINK_URL_MISMATCH"))
        ));
        assert!(matches!(
            refuse_unverifiable(&m("not_release")),
            Err(Error::InvalidCode("SMART_LINK_URL_NOT_RELEASE"))
        ));
    }
}
