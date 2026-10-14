//! Playlist pitching (migration 0116).
//!
//! The artist describes one focus track of a submitted release for platform
//! editors — what it is, genres, moods, instruments, similar artists and
//! promotion plans — at least [`LEAD_DAYS`] days before the release date
//! (editors plan playlists ahead). Staff with the delivery duty forward it to
//! the platforms' editorial teams and record the outcome; every step
//! notifies the org. One active pitch per release; a withdrawn one can be
//! replaced.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    staff::{self, Duty},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use chrono::NaiveDate;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

/// Days between the pitch and the release date.
pub const LEAD_DAYS: i64 = 7;

/// Release states a pitch is possible in: submitted and not withdrawn.
fn pitchable(status: &str) -> bool {
    !matches!(status, "DRAFT" | "WITHDRAWN" | "SUPERSEDED")
}

fn kst_today() -> NaiveDate {
    (chrono::Utc::now() + chrono::Duration::hours(9)).date_naive()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PitchInput {
    pub track_id: Uuid,
    pub description: String,
    #[serde(default)]
    pub genres: String,
    #[serde(default)]
    pub moods: String,
    #[serde(default)]
    pub instruments: String,
    #[serde(default)]
    pub similar_artists: String,
    #[serde(default)]
    pub marketing: String,
}

fn field(v: &str, min: usize, max: usize, multiline: bool) -> Result<String> {
    let v = v.trim();
    let n = v.chars().count();
    if n < min || n > max {
        return Err(Error::InvalidCode("PITCH_INVALID"));
    }
    if !v.is_empty() {
        if multiline {
            crate::text_policy::check_multiline(v)?;
        } else {
            crate::text_policy::check(v)?;
        }
    }
    Ok(v.to_string())
}

async fn release_state(
    c: &mut sqlx::PgConnection,
    org: Uuid,
    release: Uuid,
) -> Result<(String, Option<NaiveDate>)> {
    let row = sqlx::query("SELECT status, draft->>'release_date' AS rd FROM catalog.releases WHERE org_id=$1 AND id=$2 AND archived_at IS NULL")
        .bind(org)
        .bind(release)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    let date = row
        .get::<Option<String>, _>("rd")
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok());
    Ok((row.get("status"), date))
}

/// The artist pitches a focus track of a submitted release.
pub async fn submit(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: PitchInput,
) -> Result<Value> {
    let description = field(&i.description, 20, 1000, true)?;
    let genres = field(&i.genres, 0, 200, false)?;
    let moods = field(&i.moods, 0, 200, false)?;
    let instruments = field(&i.instruments, 0, 200, false)?;
    let similar = field(&i.similar_artists, 0, 200, false)?;
    let marketing = field(&i.marketing, 0, 1000, true)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let (status, date) = release_state(&mut tx, org, release).await?;
    if !pitchable(&status) {
        return Err(Error::PolicyGate("PITCH_RELEASE_NOT_SUBMITTED"));
    }
    match date {
        Some(d) if (d - kst_today()).num_days() >= LEAD_DAYS => {}
        _ => return Err(Error::PolicyGate("PITCH_TOO_LATE")),
    }
    let track_ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND id=$3 AND archived_at IS NULL)",
    )
    .bind(org)
    .bind(release)
    .bind(i.track_id)
    .fetch_one(&mut *tx)
    .await?;
    if !track_ok {
        return Err(Error::InvalidCode("PITCH_INVALID"));
    }
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO portal.playlist_pitches(id, org_id, release_id, track_id, requested_by, description, genres, moods, instruments, similar_artists, marketing)
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         ON CONFLICT (release_id) WHERE status <> 'WITHDRAWN' DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(release)
    .bind(i.track_id)
    .bind(a.user)
    .bind(&description)
    .bind(&genres)
    .bind(&moods)
    .bind(&instruments)
    .bind(&similar)
    .bind(&marketing)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or(Error::PolicyGate("PITCH_ALREADY_SUBMITTED"))?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.pitch_submitted",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "SUBMITTED"}))
}

/// Withdraw before staff forwarded it.
pub async fn withdraw(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let id: Uuid = sqlx::query_scalar(
        "UPDATE portal.playlist_pitches SET status='WITHDRAWN', updated_by=$3, updated_at=now()
          WHERE org_id=$1 AND release_id=$2 AND status IN ('SUBMITTED','IN_REVIEW') RETURNING id",
    )
    .bind(org)
    .bind(release)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::PolicyGate("PITCH_NOT_WITHDRAWABLE"))?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.pitch_withdrawn",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "WITHDRAWN"}))
}

const PITCH_JSON: &str = "jsonb_build_object('id',p.id,'release_id',p.release_id,'track_id',p.track_id,'track_title',t.title,
    'description',p.description,'genres',p.genres,'moods',p.moods,'instruments',p.instruments,
    'similar_artists',p.similar_artists,'marketing',p.marketing,'status',p.status,'platforms',p.platforms,
    'result',p.result,'staff_note',p.staff_note,'created_at',p.created_at,'updated_at',p.updated_at)";

/// The latest pitch of a release and whether a new one is possible.
pub async fn status(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let (state, date) = release_state(&mut tx, org, release).await?;
    let latest: Option<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {PITCH_JSON} FROM portal.playlist_pitches p JOIN catalog.tracks t ON t.id=p.track_id
          WHERE p.org_id=$1 AND p.release_id=$2 ORDER BY p.created_at DESC LIMIT 1"
    )))
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    let tracks: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'title',title) FROM catalog.tracks
          WHERE org_id=$1 AND release_id=$2 AND archived_at IS NULL ORDER BY disc_number, track_number",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let days_left = date.map(|d| (d - kst_today()).num_days());
    let open = latest.as_ref().is_some_and(|l| l["status"] != "WITHDRAWN");
    let can_pitch = pitchable(&state) && days_left.is_some_and(|d| d >= LEAD_DAYS) && !open;
    Ok(
        json!({"pitch": latest, "tracks": tracks, "can_pitch": can_pitch, "days_until_release": days_left, "lead_days": LEAD_DAYS}),
    )
}

// ---------------------------------------------------------------------------
// Staff
// ---------------------------------------------------------------------------
pub async fn staff_list(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {PITCH_JSON} || jsonb_build_object('org_name',o.name,'title',r.title,'artist',r.draft->>'artist',
                'release_date',r.draft->>'release_date','platforms_requested',r.draft->'platforms')
           FROM portal.playlist_pitches p
           JOIN catalog.tracks t ON t.id=p.track_id
           JOIN catalog.releases r ON r.id=p.release_id
           JOIN identity.orgs o ON o.id=p.org_id
          WHERE p.status <> 'WITHDRAWN'
          ORDER BY (p.status IN ('SUBMITTED','IN_REVIEW')) DESC, r.draft->>'release_date', p.created_at LIMIT 200"
    )))
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StaffUpdate {
    pub status: String,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub note: String,
}

/// Move a pitch forward: IN_REVIEW → PITCHED (to which platforms) →
/// SELECTED (with the playlist) or NOT_SELECTED.
pub async fn update(s: &AppState, h: &HeaderMap, id: Uuid, i: StaffUpdate) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    staff::note_ok(&i.note, 1000)?;
    staff::note_ok(&i.result, 300)?;
    let from: &[&str] = match i.status.as_str() {
        "IN_REVIEW" => &["SUBMITTED"],
        "PITCHED" => &["SUBMITTED", "IN_REVIEW"],
        "SELECTED" | "NOT_SELECTED" => &["PITCHED"],
        _ => return Err(Error::InvalidCode("PITCH_STATUS_INVALID")),
    };
    if i.status == "PITCHED" && i.platforms.is_empty() {
        return Err(Error::InvalidCode("PITCH_PLATFORMS_REQUIRED"));
    }
    if i.status == "SELECTED" && i.result.trim().is_empty() {
        return Err(Error::InvalidCode("PITCH_RESULT_REQUIRED"));
    }
    if i.platforms.len() > 20 || i.platforms.iter().any(|p| p.is_empty() || p.len() > 40) {
        return Err(Error::InvalidCode("PITCH_INVALID"));
    }
    let from: Vec<String> = from.iter().map(|v| v.to_string()).collect();
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row = sqlx::query(
        "UPDATE portal.playlist_pitches
            SET status=$2,
                platforms=CASE WHEN cardinality($3::text[]) > 0 THEN $3 ELSE platforms END,
                result=CASE WHEN $4 <> '' THEN $4 ELSE result END,
                staff_note=CASE WHEN $5 <> '' THEN $5 ELSE staff_note END,
                updated_by=$6, updated_at=now()
          WHERE id=$1 AND status = ANY($7) RETURNING org_id, release_id",
    )
    .bind(id)
    .bind(&i.status)
    .bind(&i.platforms)
    .bind(i.result.trim())
    .bind(i.note.trim())
    .bind(st.actor.user)
    .bind(&from)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        row.get("org_id"),
        row.get("release_id"),
        "staff.pitch_updated",
        &i.status,
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": i.status}))
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
async fn h_submit(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<PitchInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(submit(&s, &a, org, id, i).await?))
}
async fn h_withdraw(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(withdraw(&s, &a, org, id).await?))
}
async fn h_staff_list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(staff_list(&s, &h).await?))
}
async fn h_update(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<StaffUpdate>,
) -> Result<Json<Value>> {
    Ok(Json(update(&s, &h, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/releases/{id}/pitch",
            get(h_status).post(h_submit),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/pitch/withdraw",
            post(h_withdraw),
        )
        .route("/api/staff/pitches", get(h_staff_list))
        .route("/api/staff/pitches/{id}/status", post(h_update))
}
