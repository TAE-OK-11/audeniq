//! Pre-saves (migration 0152).
//!
//! Before the release, a fan on the smart link signs in to Spotify, Deezer or
//! TIDAL and allows "add to library" (authorization code; PKCE where the
//! platform takes it). At the release instant the server finds the album on
//! that platform — the id the platform reported when the release went live,
//! else by UPC — and saves it to the fan's library, once.
//!
//! 1. `POST /api/smart-links/{slug}/presave/{provider}` stores a one-time
//!    state (hashed) and a PKCE verifier and returns the platform URL.
//! 2. The platform sends the browser back to
//!    `{APP_ORIGIN}/api/presave/{provider}/callback`; the server exchanges the
//!    code, seals the token (field PresaveToken) and redirects to the page
//!    with `?presave={provider}` (or `presave_error`).
//! 3. [`dispatch_once`] (API process, next to the release alerts) saves the
//!    albums that are out, retrying while the platform has not listed the
//!    album yet; [`sweep`] expires what was not saved within a week.
//!
//! The token is erased as soon as a pre-save ends. AUDENIQ keeps no account,
//! email or profile of the fan. A platform is offered when its settings are
//! complete (`PRESAVE_{SPOTIFY,DEEZER,TIDAL}_CLIENT_ID` / `_CLIENT_SECRET`).
use crate::config::env_value as env;
use crate::{
    api::AppState,
    auth,
    error::{Error, Result},
    payout_keys::{Field, KeyRing},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{sync::OnceLock, time::Duration};
use uuid::Uuid;

/// Pre-saves handled per round.
pub const BATCH: i64 = 20;
/// Not saved this many days after the release instant → expired.
const EXPIRE_DAYS: i32 = 7;
const STATE_SECONDS: i32 = 900;
const LEASE_MINUTES: i32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Spotify,
    Deezer,
    Tidal,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Spotify, Kind::Deezer, Kind::Tidal];
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "spotify" => Some(Self::Spotify),
            "deezer" => Some(Self::Deezer),
            "tidal" => Some(Self::Tidal),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::Deezer => "deezer",
            Self::Tidal => "tidal",
        }
    }
    fn env(self) -> &'static str {
        match self {
            Self::Spotify => "SPOTIFY",
            Self::Deezer => "DEEZER",
            Self::Tidal => "TIDAL",
        }
    }
}

pub struct Provider {
    kind: Kind,
    client_id: String,
    secret: String,
    authorize: String,
    token: String,
    api: String,
}

/// Development/test only: one loopback HTTP server stands in for every
/// platform at `{base}/{provider}/authorize|token|api`. Ignored in production.
fn fixture() -> Option<String> {
    let base = env("PRESAVE_FIXTURE_BASE")?;
    let local = crate::external_http::local_http_enabled(true);
    crate::external_http::validate_url(&base, local)
        .ok()
        .map(|_| base.trim_end_matches('/').to_owned())
}

/// The platform when its settings are complete, else `None`.
pub fn provider(kind: Kind) -> Option<Provider> {
    let [authorize, token, api] = match fixture() {
        Some(base) => {
            let p = format!("{base}/{}", kind.name());
            [
                format!("{p}/authorize"),
                format!("{p}/token"),
                format!("{p}/api"),
            ]
        }
        None => match kind {
            Kind::Spotify => [
                "https://accounts.spotify.com/authorize".into(),
                "https://accounts.spotify.com/api/token".into(),
                "https://api.spotify.com/v1".into(),
            ],
            Kind::Deezer => [
                "https://connect.deezer.com/oauth/auth.php".into(),
                "https://connect.deezer.com/oauth/access_token.php".into(),
                "https://api.deezer.com".into(),
            ],
            Kind::Tidal => [
                "https://login.tidal.com/authorize".into(),
                "https://auth.tidal.com/v1/oauth2/token".into(),
                "https://openapi.tidal.com/v2".into(),
            ],
        },
    };
    Some(Provider {
        kind,
        client_id: env(&format!("PRESAVE_{}_CLIENT_ID", kind.env()))?,
        secret: env(&format!("PRESAVE_{}_CLIENT_SECRET", kind.env()))?,
        authorize,
        token,
        api,
    })
}

/// Platforms offered on the page (settings complete), in page order.
pub fn offered() -> Vec<&'static str> {
    Kind::ALL
        .iter()
        .filter(|k| provider(**k).is_some())
        .map(|k| k.name())
        .collect()
}

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        crate::external_http::client_builder(fixture().is_some())
            .timeout(Duration::from_secs(10))
            .build()
            .expect("HTTP client")
    })
}

/// `base` with the query pairs added (reqwest is built without `query`/`form`).
fn with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    url::Url::parse_with_params(base, pairs)
        .map(String::from)
        .unwrap_or_else(|_| base.to_owned())
}

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

fn redirect_uri(s: &AppState, kind: Kind) -> String {
    format!("{}/api/presave/{}/callback", s.config.origin, kind.name())
}

// ---------------------------------------------------------------------------
// Start / callback
// ---------------------------------------------------------------------------

/// A fan starts a pre-save: the platform URL to send the browser to.
pub async fn start(s: &AppState, h: &HeaderMap, slug: &str, name: &str) -> Result<Value> {
    auth::origin(h, &s.config)?;
    let kind = Kind::parse(name).ok_or(Error::NotFound)?;
    let p = provider(kind).ok_or(Error::PolicyGate("PRESAVE_PROVIDER_NOT_CONFIGURED"))?;
    let row = crate::smart_links::visible(s, slug).await?;
    if crate::smart_links::is_released(
        row.get::<Option<String>, _>("release_date").as_deref(),
        row.get::<Option<String>, _>("release_time").as_deref(),
    ) || row.get::<Option<String>, _>("release_date").is_none()
    {
        return Err(Error::PolicyGate("PRESAVE_ALREADY_RELEASED"));
    }
    let ip = auth::client_ip_key(h).unwrap_or_else(|| "unknown".into());
    auth::rate(&s.pool, &format!("presave:{ip}"), 20).await?;
    auth::rate(&s.pool, "presave:all", 3000).await?;
    let (state, verifier) = (auth::random_token(), auth::random_token());
    sqlx::query("DELETE FROM portal.presave_starts WHERE expires_at < now()")
        .execute(&s.pool)
        .await?;
    sqlx::query(
        "INSERT INTO portal.presave_starts(state_hash, link_id, provider, code_verifier, expires_at)
         VALUES($1,$2,$3,$4, now() + make_interval(secs => $5))",
    )
    .bind(auth::hash_token(&state))
    .bind(row.get::<Uuid, _>("id"))
    .bind(kind.name())
    .bind(&verifier)
    .bind(f64::from(STATE_SECONDS))
    .execute(&s.pool)
    .await?;
    let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
    let redirect = redirect_uri(s, kind);
    let params: Vec<(&str, &str)> = match kind {
        Kind::Spotify => vec![
            ("client_id", p.client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("scope", "user-library-modify"),
            ("state", state.as_str()),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
        ],
        // Deezer's connect flow has no PKCE; the secret stays on the server.
        Kind::Deezer => vec![
            ("app_id", p.client_id.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("perms", "basic_access,manage_library,offline_access"),
            ("state", state.as_str()),
        ],
        Kind::Tidal => vec![
            ("client_id", p.client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", redirect.as_str()),
            ("scope", "collection.write"),
            ("state", state.as_str()),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
        ],
    };
    let url = url::Url::parse_with_params(&p.authorize, &params).map_err(|_| Error::Internal)?;
    Ok(json!({"url": url.as_str()}))
}

#[derive(Deserialize, Default)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    /// Deezer reports a refusal as `error_reason`.
    pub error_reason: Option<String>,
}

/// What the server keeps until the release: a refresh token (Spotify, TIDAL)
/// or the long-lived access token (Deezer `offline_access`).
async fn exchange(s: &AppState, p: &Provider, code: &str, verifier: &str) -> Result<String> {
    let redirect = redirect_uri(s, p.kind);
    let res = match p.kind {
        Kind::Deezer => {
            http()
                .get(with_query(
                    &p.token,
                    &[
                        ("app_id", p.client_id.as_str()),
                        ("secret", p.secret.as_str()),
                        ("code", code),
                        ("output", "json"),
                    ],
                ))
                .send()
                .await
        }
        Kind::Spotify | Kind::Tidal => {
            http()
                .post(&p.token)
                .basic_auth(&p.client_id, Some(&p.secret))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form(&[
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("redirect_uri", redirect.as_str()),
                    ("client_id", p.client_id.as_str()),
                    ("code_verifier", verifier),
                ]))
                .send()
                .await
        }
    }
    .map_err(|_| Error::PolicyGate("PRESAVE_PROVIDER_UNAVAILABLE"))?;
    if !res.status().is_success() {
        return Err(Error::PolicyGate("PRESAVE_TOKEN_REJECTED"));
    }
    let v: Value = res
        .json()
        .await
        .map_err(|_| Error::PolicyGate("PRESAVE_TOKEN_REJECTED"))?;
    let key = if p.kind == Kind::Deezer {
        "access_token"
    } else {
        "refresh_token"
    };
    v[key]
        .as_str()
        .filter(|t| !t.is_empty() && t.len() <= 4096)
        .map(str::to_owned)
        .ok_or(Error::PolicyGate("PRESAVE_TOKEN_REJECTED"))
}

/// The platform sends the browser back here; always ends on the smart link.
pub async fn callback(s: &AppState, name: &str, q: CallbackParams) -> Response {
    let to = |slug: Option<&str>, query: &str| {
        let path = slug
            .map(|sl| format!("/link/release/{sl}"))
            .unwrap_or_else(|| "/".into());
        let mut r = StatusCode::SEE_OTHER.into_response();
        if let Ok(v) = HeaderValue::from_str(&format!("{}{path}?{query}", s.config.origin)) {
            r.headers_mut().insert(header::LOCATION, v);
        }
        r.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        r
    };
    let Some(kind) = Kind::parse(name) else {
        return to(None, "presave_error=PRESAVE_PROVIDER_UNKNOWN");
    };
    // The state is single-use: taken (deleted) before anything else.
    let Some(state) = q
        .state
        .as_deref()
        .filter(|v| !v.is_empty() && v.len() <= 200)
    else {
        return to(None, "presave_error=PRESAVE_STATE_INVALID");
    };
    let taken = sqlx::query(
        "DELETE FROM portal.presave_starts p USING portal.smart_links l
          WHERE p.state_hash=$1 AND p.provider=$2 AND p.expires_at > now() AND l.id=p.link_id
          RETURNING p.link_id, p.code_verifier, l.slug",
    )
    .bind(auth::hash_token(state))
    .bind(kind.name())
    .fetch_optional(&s.pool)
    .await;
    let row = match taken {
        Ok(Some(r)) => r,
        Ok(None) => return to(None, "presave_error=PRESAVE_STATE_INVALID"),
        Err(_) => return to(None, "presave_error=PRESAVE_FAILED"),
    };
    let slug: String = row.get("slug");
    let back = |code: &str| to(Some(&slug), &format!("presave_error={code}"));
    if q.error.is_some() || q.error_reason.is_some() {
        return back("PRESAVE_DENIED");
    }
    let Some(code) = q
        .code
        .as_deref()
        .filter(|c| !c.is_empty() && c.len() <= 2048)
    else {
        return back("PRESAVE_DENIED");
    };
    let Some(p) = provider(kind) else {
        return back("PRESAVE_PROVIDER_NOT_CONFIGURED");
    };
    let token = match exchange(s, &p, code, &row.get::<String, _>("code_verifier")).await {
        Ok(t) => t,
        Err(Error::PolicyGate(c)) => return back(c),
        Err(_) => return back("PRESAVE_FAILED"),
    };
    let id = Uuid::new_v4();
    let sealed = match s.payout_keys.seal_field(Field::PresaveToken, id, &token) {
        Ok(c) => c,
        Err(_) => return back("PRESAVE_FAILED"),
    };
    let saved = sqlx::query(
        "INSERT INTO portal.presaves(id, link_id, provider, token_cipher) VALUES($1,$2,$3,$4)",
    )
    .bind(id)
    .bind(row.get::<Uuid, _>("link_id"))
    .bind(kind.name())
    .bind(sealed)
    .execute(&s.pool)
    .await;
    match saved {
        Ok(_) => to(Some(&slug), &format!("presave={}", kind.name())),
        Err(_) => back("PRESAVE_FAILED"),
    }
}

// ---------------------------------------------------------------------------
// Saving at release
// ---------------------------------------------------------------------------

enum Outcome {
    Saved,
    /// The platform has not listed the album yet (or answered with an error): try again later.
    Retry(&'static str),
    /// The fan withdrew access, or the platform refused for good.
    Failed(&'static str),
}

async fn json_or(res: reqwest::Response) -> std::result::Result<Value, u16> {
    let status = res.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(status);
    }
    Ok(res.json().await.unwrap_or(Value::Null))
}

/// An access token for the saved credential.
async fn access_token(p: &Provider, stored: &str) -> std::result::Result<String, Outcome> {
    if p.kind == Kind::Deezer {
        return Ok(stored.to_owned());
    }
    let res = http()
        .post(&p.token)
        .basic_auth(&p.client_id, Some(&p.secret))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", stored),
            ("client_id", p.client_id.as_str()),
        ]))
        .send()
        .await
        .map_err(|_| Outcome::Retry("PRESAVE_PROVIDER_UNAVAILABLE"))?;
    match json_or(res).await {
        Ok(v) => v["access_token"]
            .as_str()
            .map(str::to_owned)
            .ok_or(Outcome::Retry("PRESAVE_PROVIDER_UNAVAILABLE")),
        Err(400 | 401) => Err(Outcome::Failed("PRESAVE_TOKEN_REVOKED")),
        Err(_) => Err(Outcome::Retry("PRESAVE_PROVIDER_UNAVAILABLE")),
    }
}

/// The album's id on the platform: the id it reported when the release went
/// live, else a lookup by UPC.
async fn album_id(
    p: &Provider,
    token: &str,
    known: Option<&str>,
    upc: Option<&str>,
) -> std::result::Result<String, Outcome> {
    if let Some(id) = known {
        return Ok(id.to_owned());
    }
    let upc = upc
        .filter(|u| u.bytes().all(|b| b.is_ascii_digit()) && (12..=14).contains(&u.len()))
        .ok_or(Outcome::Retry("PRESAVE_ALBUM_NOT_FOUND"))?;
    let req = match p.kind {
        Kind::Spotify => http()
            .get(with_query(
                &format!("{}/search", p.api),
                &[
                    ("q", &format!("upc:{upc}")),
                    ("type", "album"),
                    ("limit", "1"),
                ],
            ))
            .bearer_auth(token),
        Kind::Deezer => http().get(format!("{}/album/upc:{upc}", p.api)),
        Kind::Tidal => http()
            .get(with_query(
                &format!("{}/albums", p.api),
                &[("filter[barcodeId]", upc), ("countryCode", "KR")],
            ))
            .bearer_auth(token)
            .header(header::ACCEPT, "application/vnd.api+json"),
    };
    let v = json_or(
        req.send()
            .await
            .map_err(|_| Outcome::Retry("PRESAVE_PROVIDER_UNAVAILABLE"))?,
    )
    .await
    .map_err(|_| Outcome::Retry("PRESAVE_ALBUM_NOT_FOUND"))?;
    let id = match p.kind {
        Kind::Spotify => v["albums"]["items"][0]["id"].as_str().map(str::to_owned),
        // Deezer answers 200 with {"error": …} for an unknown UPC.
        Kind::Deezer => v["id"].as_u64().map(|n| n.to_string()),
        Kind::Tidal => v["data"][0]["id"].as_str().map(str::to_owned),
    };
    id.ok_or(Outcome::Retry("PRESAVE_ALBUM_NOT_FOUND"))
}

async fn save_album(p: &Provider, token: &str, album: &str) -> Outcome {
    let req = match p.kind {
        Kind::Spotify => http()
            .put(with_query(
                &format!("{}/me/albums", p.api),
                &[("ids", album)],
            ))
            .bearer_auth(token),
        Kind::Deezer => http().post(with_query(
            &format!("{}/user/me/albums", p.api),
            &[("album_id", album), ("access_token", token)],
        )),
        Kind::Tidal => {
            // The collection belongs to the signed-in user: read their id first.
            let me = match http()
                .get(format!("{}/users/me", p.api))
                .bearer_auth(token)
                .header(header::ACCEPT, "application/vnd.api+json")
                .send()
                .await
            {
                Ok(r) => json_or(r).await,
                Err(_) => return Outcome::Retry("PRESAVE_PROVIDER_UNAVAILABLE"),
            };
            let Some(user) = me
                .ok()
                .and_then(|v| v["data"]["id"].as_str().map(str::to_owned))
            else {
                return Outcome::Retry("PRESAVE_PROVIDER_UNAVAILABLE");
            };
            http()
                .post(with_query(
                    &format!("{}/userCollections/{user}/relationships/albums", p.api),
                    &[("countryCode", "KR")],
                ))
                .bearer_auth(token)
                .header(header::CONTENT_TYPE, "application/vnd.api+json")
                .body(json!({"data": [{"id": album, "type": "albums"}]}).to_string())
        }
    };
    match req.send().await {
        Err(_) => Outcome::Retry("PRESAVE_PROVIDER_UNAVAILABLE"),
        Ok(r) => match json_or(r).await {
            // Deezer answers 200 with {"error": …} when it refuses.
            Ok(v) if v.get("error").is_some() => Outcome::Retry("PRESAVE_SAVE_REFUSED"),
            Ok(_) => Outcome::Saved,
            Err(401 | 403) => Outcome::Failed("PRESAVE_TOKEN_REVOKED"),
            Err(_) => Outcome::Retry("PRESAVE_SAVE_REFUSED"),
        },
    }
}

async fn finish(pool: &PgPool, id: Uuid, status: &str, code: Option<&str>) -> Result<()> {
    sqlx::query(
        "UPDATE portal.presaves SET status=$2, last_error=$3, token_cipher=NULL,
            saved_at=CASE WHEN $2='SAVED' THEN now() ELSE saved_at END
          WHERE id=$1",
    )
    .bind(id)
    .bind(status)
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}

fn backoff_seconds(attempts: i32) -> f64 {
    match attempts {
        ..=1 => 300.0,
        2 => 900.0,
        3 => 3600.0,
        4 => 10800.0,
        _ => 21600.0,
    }
}

/// The release's UPC (assigned code, else the one on the release) and the
/// album ids the platforms reported.
async fn release_codes(
    pool: &PgPool,
    org: Uuid,
    release: Uuid,
) -> Result<(
    Option<String>,
    std::collections::BTreeMap<&'static str, String>,
)> {
    let mut tx = pool.begin().await?;
    let known = crate::smart_links::known_ids(&mut tx, org, release).await?;
    let upc: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE((SELECT ia.identifier FROM distribution.identifier_assignments ia
                           WHERE ia.org_id=r.org_id AND ia.release_id=r.id AND ia.kind='UPC' AND ia.status='ASSIGNED'
                           ORDER BY ia.created_at DESC LIMIT 1), r.upc)
           FROM catalog.releases r WHERE r.org_id=$1 AND r.id=$2",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    tx.rollback().await?;
    Ok((upc, known))
}

/// Save the albums that are out (from the release instant, KST).
pub async fn dispatch_once(pool: &PgPool, keys: &KeyRing) -> Result<usize> {
    let now_kst = (chrono::Utc::now() + chrono::Duration::hours(9))
        .format("%Y-%m-%d %H:%M")
        .to_string();
    let rows = sqlx::query(
        "UPDATE portal.presaves p
            SET attempts=p.attempts+1, next_attempt_at=now() + make_interval(mins => $2)
          WHERE p.id IN (
            SELECT p2.id FROM portal.presaves p2
              JOIN portal.smart_links l ON l.id=p2.link_id
              JOIN catalog.releases r ON r.id=l.release_id
             WHERE p2.status='PENDING' AND p2.next_attempt_at <= now()
               AND (r.draft->>'release_date') || ' ' || COALESCE(NULLIF(r.draft->>'release_time', ''), '00:00') <= $3
             ORDER BY p2.next_attempt_at LIMIT $1 FOR UPDATE OF p2 SKIP LOCKED)
          RETURNING p.id, p.link_id, p.provider, p.attempts, p.token_cipher",
    )
    .bind(BATCH)
    .bind(LEASE_MINUTES)
    .bind(&now_kst)
    .fetch_all(pool)
    .await?;
    for row in &rows {
        let id: Uuid = row.get("id");
        let attempts: i32 = row.get("attempts");
        let page = match crate::smart_links::visible_by_id(pool, row.get("link_id")).await {
            Ok(p) => p,
            Err(Error::NotFound) => {
                finish(pool, id, "CANCELLED", Some("PRESAVE_PAGE_GONE")).await?;
                continue;
            }
            Err(e) => return Err(e),
        };
        let Some(kind) = Kind::parse(&row.get::<String, _>("provider")) else {
            finish(pool, id, "FAILED", Some("PRESAVE_PROVIDER_UNKNOWN")).await?;
            continue;
        };
        let Some(p) = provider(kind) else {
            // Settings removed: wait (the sweep expires it after a week).
            sqlx::query(
                "UPDATE portal.presaves SET last_error='PRESAVE_PROVIDER_NOT_CONFIGURED',
                    next_attempt_at=now() + interval '6 hours' WHERE id=$1",
            )
            .bind(id)
            .execute(pool)
            .await?;
            continue;
        };
        let sealed: Option<Vec<u8>> = row.get("token_cipher");
        let Some(Ok(stored)) = sealed.map(|c| keys.open_field(Field::PresaveToken, id, &c)) else {
            finish(pool, id, "FAILED", Some("PRESAVE_TOKEN_UNAVAILABLE")).await?;
            continue;
        };
        let (upc, known) = release_codes(pool, page.get("org_id"), page.get("release_id")).await?;
        let outcome = match access_token(&p, &stored).await {
            Err(o) => o,
            Ok(token) => {
                match album_id(
                    &p,
                    &token,
                    known.get(kind.name()).map(String::as_str),
                    upc.as_deref(),
                )
                .await
                {
                    Err(o) => o,
                    Ok(album) => save_album(&p, &token, &album).await,
                }
            }
        };
        match outcome {
            Outcome::Saved => finish(pool, id, "SAVED", None).await?,
            Outcome::Failed(code) => finish(pool, id, "FAILED", Some(code)).await?,
            Outcome::Retry(code) => {
                sqlx::query(
                    "UPDATE portal.presaves SET last_error=$2, next_attempt_at=now() + make_interval(secs => $3) WHERE id=$1",
                )
                .bind(id)
                .bind(code)
                .bind(backoff_seconds(attempts))
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(rows.len())
}

/// Expire pre-saves not saved within a week of the release date (or whose
/// release date was removed), and drop stale start states.
pub async fn sweep(pool: &PgPool) -> Result<()> {
    let cutoff = (chrono::Utc::now() + chrono::Duration::hours(9)
        - chrono::Duration::days(EXPIRE_DAYS.into()))
    .date_naive()
    .to_string();
    sqlx::query(
        "UPDATE portal.presaves p SET status='EXPIRED', last_error=COALESCE(p.last_error, 'PRESAVE_EXPIRED'), token_cipher=NULL
           FROM portal.smart_links l JOIN catalog.releases r ON r.id=l.release_id
          WHERE p.link_id=l.id AND p.status='PENDING'
            AND (r.draft->>'release_date' IS NULL OR r.draft->>'release_date' < $1)",
    )
    .bind(cutoff)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM portal.presave_starts WHERE expires_at < now()")
        .execute(pool)
        .await?;
    Ok(())
}

/// Pre-saves per platform for the org's smart link screen (all time).
pub(crate) async fn counts_in(c: &mut sqlx::PgConnection, link: Uuid) -> Result<Value> {
    let rows = sqlx::query(
        "SELECT provider, count(*)::bigint AS n, count(*) FILTER (WHERE status='SAVED')::bigint AS saved
           FROM portal.presaves WHERE link_id=$1 GROUP BY provider",
    )
    .bind(link)
    .fetch_all(&mut *c)
    .await?;
    let mut out = serde_json::Map::new();
    for r in rows {
        out.insert(
            r.get::<String, _>("provider"),
            json!({"total": r.get::<i64, _>("n"), "saved": r.get::<i64, _>("saved")}),
        );
    }
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_start(
    State(s): State<AppState>,
    Path((slug, provider)): Path<(String, String)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(start(&s, &h, &slug, &provider).await?))
}
async fn h_callback(
    State(s): State<AppState>,
    Path(provider): Path<String>,
    Query(q): Query<CallbackParams>,
) -> Response {
    callback(&s, &provider, q).await
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/smart-links/{slug}/presave/{provider}", post(h_start))
        .route("/api/presave/{provider}/callback", get(h_callback))
}
