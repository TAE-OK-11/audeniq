//! Platform requests (migration 0120): what an artist asks AUDENIQ to get
//! done at a platform after release, tracked to the end.
//!
//! Artist profiles: a release on the wrong artist page (`PROFILE_MISMATCH`),
//! two artists on one page (`PROFILE_SPLIT`), duplicate pages
//! (`PROFILE_MERGE`), a YouTube Official Artist Channel (`OAC`), a new
//! profile photo at one platform (`PROFILE_IMAGE`, made from the artist
//! screen with the photo it carries: [`crate::artist_studio`]; staff open
//! the photo to send it).
//! YouTube Content ID / UGC: channels never to claim (`CID_ALLOWLIST`), a
//! claim on a video to release (`CID_RELEASE_CLAIM`), someone else's claim on
//! the artist's recording to dispute (`CID_DISPUTE`), Content ID off/on for a
//! release (`CID_OPT_OUT` / `CID_OPT_IN`).
//!
//! Staff with the delivery duty move a request SUBMITTED → IN_PROGRESS →
//! SENT_TO_PLATFORM (with the platform's ticket) → RESOLVED / REJECTED; the
//! artist can cancel until it is sent. Each step is an event the artist sees
//! and a notice. Resolving a Content ID request updates the release's Content
//! ID state (`portal.content_id_status`), which staff also keep current
//! (monetization as YouTube reports it).
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
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

const PROFILE_KINDS: &[&str] = &[
    "PROFILE_MISMATCH",
    "PROFILE_SPLIT",
    "PROFILE_MERGE",
    "OAC",
    "PROFILE_IMAGE",
];
const CID_KINDS: &[&str] = &[
    "CID_ALLOWLIST",
    "CID_RELEASE_CLAIM",
    "CID_DISPUTE",
    "CID_OPT_OUT",
    "CID_OPT_IN",
];
const YOUTUBE_HOSTS: &[&str] = &["youtube.com", "youtu.be"];
const MONETIZATION: &[&str] = &["UNKNOWN", "MONETIZING", "TRACKING", "BLOCKED", "INELIGIBLE"];

fn invalid() -> Error {
    Error::InvalidCode("PLATFORM_REQUEST_INVALID")
}

/// https on one of `hosts` (or a subdomain), no credentials or port.
fn check_url(raw: &str, hosts: &[&str]) -> Result<String> {
    let bad = || Error::InvalidCode("PLATFORM_REQUEST_URL_INVALID");
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
    Ok(u.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRequest {
    pub kind: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub release_id: Option<Uuid>,
    pub artist_name: String,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub description: String,
}

/// Validated (platform, urls, description) for a kind.
fn validate(i: &NewRequest) -> Result<(String, Vec<String>, String)> {
    let kind = i.kind.as_str();
    let youtube = CID_KINDS.contains(&kind) || kind == "OAC";
    let platform = if youtube {
        "youtube".to_string()
    } else {
        i.platform.trim().to_ascii_lowercase()
    };
    let hosts: &[&str] = if youtube {
        YOUTUBE_HOSTS
    } else {
        crate::smart_links::hosts(&platform)
            .ok_or(Error::InvalidCode("PLATFORM_REQUEST_PLATFORM_INVALID"))?
    };
    if i.urls.len() > 10 {
        return Err(invalid());
    }
    let urls = i
        .urls
        .iter()
        .filter(|u| !u.trim().is_empty())
        .map(|u| check_url(u, hosts))
        .collect::<Result<Vec<_>>>()?;
    let description = i.description.trim().to_string();
    if description.chars().count() > 2000 {
        return Err(invalid());
    }
    if !description.is_empty() {
        crate::text_policy::check_multiline(&description)?;
    }
    let need_release = matches!(kind, "PROFILE_MISMATCH" | "CID_OPT_OUT" | "CID_OPT_IN");
    if need_release && i.release_id.is_none() {
        return Err(Error::InvalidCode("PLATFORM_REQUEST_RELEASE_REQUIRED"));
    }
    let (min_urls, need_text) = match kind {
        // The page the release wrongly sits on.
        "PROFILE_MISMATCH" => (1, false),
        "PROFILE_SPLIT" => (1, true),
        "PROFILE_MERGE" => (2, false),
        // The artist's own YouTube channel.
        "OAC" => (1, false),
        "CID_ALLOWLIST" => (1, false),
        "CID_RELEASE_CLAIM" => (1, false),
        "CID_DISPUTE" => (1, true),
        "CID_OPT_OUT" | "CID_OPT_IN" => (0, false),
        _ => return Err(Error::InvalidCode("PLATFORM_REQUEST_KIND_INVALID")),
    };
    if urls.len() < min_urls {
        return Err(Error::InvalidCode("PLATFORM_REQUEST_URL_REQUIRED"));
    }
    if need_text && description.chars().count() < 10 {
        return Err(Error::InvalidCode("PLATFORM_REQUEST_DESCRIPTION_REQUIRED"));
    }
    Ok((platform, urls, description))
}

async fn event(
    c: &mut PgConnection,
    id: Uuid,
    status: &str,
    note: &str,
    by_staff: bool,
) -> Result<()> {
    sqlx::query("INSERT INTO portal.platform_request_events(request_id, status, note, by_staff) VALUES($1,$2,$3,$4)")
        .bind(id)
        .bind(status)
        .bind(note)
        .bind(by_staff)
        .execute(&mut *c)
        .await?;
    Ok(())
}

pub async fn create(s: &AppState, a: &Actor, org: Uuid, i: NewRequest) -> Result<Value> {
    let (platform, urls, description) = validate(&i)?;
    let artist = i.artist_name.trim().to_string();
    if artist.is_empty() || artist.chars().count() > 200 {
        return Err(invalid());
    }
    crate::text_policy::check(&artist)?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    if let Some(r) = i.release_id {
        auth::authorize(&mut tx, a, org, r, "release", true).await?;
        let submitted: bool = sqlx::query_scalar(
            "SELECT status NOT IN ('DRAFT','SUPERSEDED') FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL",
        )
        .bind(org)
        .bind(r)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        if !submitted {
            return Err(Error::PolicyGate("PLATFORM_REQUEST_RELEASE_NOT_SUBMITTED"));
        }
    }
    auth::rate(&s.pool, &format!("platform-request:{org}"), 30).await?;
    // One open request of a kind per release (or per org for release-less kinds and platform).
    let open: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.platform_requests WHERE org_id=$1 AND kind=$2 AND platform=$3
                        AND release_id IS NOT DISTINCT FROM $4 AND status IN ('SUBMITTED','IN_PROGRESS','SENT_TO_PLATFORM')
                        AND urls = $5)",
    )
    .bind(org)
    .bind(&i.kind)
    .bind(&platform)
    .bind(i.release_id)
    .bind(&urls)
    .fetch_one(&mut *tx)
    .await?;
    if open {
        return Err(Error::PolicyGate("PLATFORM_REQUEST_ALREADY_OPEN"));
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.platform_requests(id, org_id, release_id, kind, platform, artist_name, urls, description, created_by)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(id)
    .bind(org)
    .bind(i.release_id)
    .bind(&i.kind)
    .bind(&platform)
    .bind(&artist)
    .bind(&urls)
    .bind(&description)
    .bind(a.user)
    .execute(&mut *tx)
    .await?;
    event(&mut tx, id, "SUBMITTED", "", false).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        i.release_id,
        "platform_request.created",
        &i.kind,
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "SUBMITTED"}))
}

const REQUEST_JSON: &str = "jsonb_build_object('id',p.id,'kind',p.kind,'platform',p.platform,'release_id',p.release_id,
    'release_title',(SELECT r.title FROM catalog.releases r WHERE r.id=p.release_id),'artist_name',p.artist_name,
    'urls',p.urls,'description',p.description,'status',p.status,'platform_ref',p.platform_ref,'resolution',p.resolution,
    'artist_id',p.artist_id,'image',p.asset_id IS NOT NULL,
    'created_at',p.created_at,'updated_at',p.updated_at,
    'events',COALESCE((SELECT jsonb_agg(jsonb_build_object('status',e.status,'note',e.note,'by_staff',e.by_staff,'at',e.created_at)
                        ORDER BY e.created_at) FROM portal.platform_request_events e WHERE e.request_id=p.id),'[]'::jsonb))";

#[derive(Deserialize, Default)]
pub struct ListQuery {
    /// "profile" or "content_id"; empty = all.
    #[serde(default)]
    pub group: String,
}

fn kinds(group: &str) -> Vec<String> {
    let list: &[&str] = match group {
        "profile" => PROFILE_KINDS,
        "content_id" => CID_KINDS,
        _ => &[],
    };
    list.iter().map(|k| k.to_string()).collect()
}

pub async fn list(s: &AppState, a: &Actor, org: Uuid, q: ListQuery) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REQUEST_JSON} FROM portal.platform_requests p
          WHERE p.org_id=$1 AND (cardinality($2::text[])=0 OR p.kind = ANY($2))
          ORDER BY p.created_at DESC LIMIT 300"
    )))
    .bind(org)
    .bind(kinds(&q.group))
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

pub async fn cancel(s: &AppState, a: &Actor, org: Uuid, id: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, true).await?;
    let n = sqlx::query(
        "UPDATE portal.platform_requests SET status='CANCELLED', updated_at=now()
          WHERE org_id=$1 AND id=$2 AND status IN ('SUBMITTED','IN_PROGRESS')",
    )
    .bind(org)
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(Error::PolicyGate("PLATFORM_REQUEST_NOT_CANCELLABLE"));
    }
    event(&mut tx, id, "CANCELLED", "", false).await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "CANCELLED"}))
}

/// The release's Content ID state (defaults when staff have recorded none).
pub async fn content_id(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let row: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('enabled',enabled,'monetization',monetization,'allowlisted_channels',allowlisted_channels,
                'note',note,'updated_at',updated_at) FROM portal.content_id_status WHERE release_id=$1 AND org_id=$2",
    )
    .bind(release)
    .bind(org)
    .fetch_optional(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(row.unwrap_or_else(|| json!({"enabled": true, "monetization": "UNKNOWN", "allowlisted_channels": [], "note": "", "updated_at": null})))
}

// ---------------------------------------------------------------------------
// Staff
// ---------------------------------------------------------------------------
#[derive(Deserialize, Default)]
pub struct StaffQuery {
    /// "open" (default) or "all".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub group: String,
}

pub async fn staff_list(s: &AppState, h: &HeaderMap, q: StaffQuery) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {REQUEST_JSON} || jsonb_build_object('org_name',o.name) FROM portal.platform_requests p
           JOIN identity.orgs o ON o.id=p.org_id
          WHERE ($1 OR p.status IN ('SUBMITTED','IN_PROGRESS','SENT_TO_PLATFORM'))
            AND (cardinality($2::text[])=0 OR p.kind = ANY($2))
          ORDER BY (p.status IN ('SUBMITTED','IN_PROGRESS','SENT_TO_PLATFORM')) DESC, p.created_at LIMIT 300"
    )))
    .bind(q.status == "all")
    .bind(kinds(&q.group))
    .fetch_all(&mut *tx)
    .await?;
    tx.rollback().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffUpdate {
    pub status: String,
    #[serde(default)]
    pub platform_ref: String,
    #[serde(default)]
    pub note: String,
}

pub async fn staff_update(s: &AppState, h: &HeaderMap, id: Uuid, i: StaffUpdate) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    staff::note_ok(&i.note, 1000)?;
    staff::note_ok(&i.platform_ref, 200)?;
    let from: &[&str] = match i.status.as_str() {
        "IN_PROGRESS" => &["SUBMITTED"],
        "SENT_TO_PLATFORM" => &["SUBMITTED", "IN_PROGRESS"],
        "RESOLVED" | "REJECTED" => &["SUBMITTED", "IN_PROGRESS", "SENT_TO_PLATFORM"],
        _ => return Err(Error::InvalidCode("PLATFORM_REQUEST_STATUS_INVALID")),
    };
    if i.status == "SENT_TO_PLATFORM" && i.platform_ref.trim().is_empty() {
        return Err(Error::InvalidCode("PLATFORM_REQUEST_REF_REQUIRED"));
    }
    if i.status == "REJECTED" && i.note.trim().is_empty() {
        return Err(Error::InvalidCode("PLATFORM_REQUEST_NOTE_REQUIRED"));
    }
    let from: Vec<String> = from.iter().map(|v| v.to_string()).collect();
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row = sqlx::query(
        "UPDATE portal.platform_requests
            SET status=$2,
                platform_ref=CASE WHEN $3 <> '' THEN $3 ELSE platform_ref END,
                resolution=CASE WHEN $2 IN ('RESOLVED','REJECTED') THEN $4 ELSE resolution END,
                updated_at=now()
          WHERE id=$1 AND status = ANY($5)
          RETURNING org_id, release_id, kind, urls",
    )
    .bind(id)
    .bind(&i.status)
    .bind(i.platform_ref.trim())
    .bind(i.note.trim())
    .bind(&from)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    event(&mut tx, id, &i.status, i.note.trim(), true).await?;
    let org: Uuid = row.get("org_id");
    let release: Option<Uuid> = row.get("release_id");
    let kind: String = row.get("kind");
    // A resolved Content ID request is now the release's state.
    if i.status == "RESOLVED"
        && let Some(release) = release
    {
        let urls: Vec<String> = row.get("urls");
        let sql = match kind.as_str() {
            "CID_OPT_OUT" => Some("enabled=false"),
            "CID_OPT_IN" => Some("enabled=true"),
            "CID_ALLOWLIST" => Some(
                "allowlisted_channels=(SELECT array_agg(DISTINCT c) FROM unnest(portal.content_id_status.allowlisted_channels || $4) c)",
            ),
            _ => None,
        };
        if let Some(set) = sql {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO portal.content_id_status(release_id, org_id, enabled, allowlisted_channels, updated_by)
                 VALUES($1, $2, $5, CASE WHEN $6 THEN $4 ELSE '{{}}'::text[] END, $3)
                 ON CONFLICT (release_id) DO UPDATE SET {set}, updated_by=$3, updated_at=now()"
            )))
            .bind(release)
            .bind(org)
            .bind(st.actor.user)
            .bind(&urls)
            .bind(kind != "CID_OPT_OUT")
            .bind(kind == "CID_ALLOWLIST")
            .execute(&mut *tx)
            .await?;
        }
    }
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        release,
        "staff.platform_request",
        &i.status,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": i.status}))
}

/// The photo a `PROFILE_IMAGE` request carries, for the staff member who
/// sends it to the platform.
pub async fn staff_image(
    s: &AppState,
    h: &HeaderMap,
    id: Uuid,
) -> Result<crate::uploads::StoredImage> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row: Option<(Uuid, Option<Uuid>)> =
        sqlx::query_as("SELECT org_id, asset_id FROM portal.platform_requests WHERE id=$1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    tx.rollback().await?;
    let (org, asset) = row.ok_or(Error::NotFound)?;
    crate::uploads::stored_image(s, org, asset.ok_or(Error::NotFound)?).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentIdInput {
    pub monetization: String,
    #[serde(default)]
    pub note: String,
}

/// Staff record what YouTube shows for the release.
pub async fn staff_content_id(
    s: &AppState,
    h: &HeaderMap,
    release: Uuid,
    i: ContentIdInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    staff::note_ok(&i.note, 1000)?;
    if !MONETIZATION.contains(&i.monetization.as_str()) {
        return Err(Error::InvalidCode("CONTENT_ID_STATUS_INVALID"));
    }
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    sqlx::query(
        "INSERT INTO portal.content_id_status(release_id, org_id, monetization, note, updated_by) VALUES($1,$2,$3,$4,$5)
         ON CONFLICT (release_id) DO UPDATE SET monetization=$3, note=$4, updated_by=$5, updated_at=now()",
    )
    .bind(release)
    .bind(org)
    .bind(&i.monetization)
    .bind(i.note.trim())
    .bind(st.actor.user)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "staff.content_id_status",
        &i.monetization,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"release_id": release, "monetization": i.monetization}))
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------
async fn h_create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<NewRequest>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(create(&s, &a, org, i).await?))
}
async fn h_list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    Query(q): Query<ListQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(list(&s, &a, org, q).await?))
}
async fn h_cancel(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(cancel(&s, &a, org, id).await?))
}
async fn h_content_id(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(content_id(&s, &a, org, id).await?))
}
async fn h_staff_list(
    State(s): State<AppState>,
    Query(q): Query<StaffQuery>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(staff_list(&s, &h, q).await?))
}
async fn h_staff_update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<StaffUpdate>,
) -> Result<Json<Value>> {
    Ok(Json(staff_update(&s, &h, id, i).await?))
}
async fn h_staff_image(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<axum::response::Response> {
    Ok(crate::uploads::image_response(
        crate::image_variants::negotiate(&s, staff_image(&s, &h, id).await?, &h).await,
        &h,
    ))
}
async fn h_staff_content_id(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<ContentIdInput>,
) -> Result<Json<Value>> {
    Ok(Json(staff_content_id(&s, &h, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/platform-requests",
            get(h_list).post(h_create),
        )
        .route(
            "/api/orgs/{org}/platform-requests/{id}/cancel",
            post(h_cancel),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/content-id",
            get(h_content_id),
        )
        .route("/api/staff/platform-requests", get(h_staff_list))
        .route(
            "/api/staff/platform-requests/{id}/status",
            post(h_staff_update),
        )
        .route(
            "/api/staff/platform-requests/{id}/image",
            get(h_staff_image),
        )
        .route(
            "/api/staff/releases/{id}/content-id",
            post(h_staff_content_id),
        )
}
