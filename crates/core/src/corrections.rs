//! Metadata corrections of releases already sent to platforms (migration 0115).
//!
//! Only what platforms take as a plain metadata update can change after
//! delivery: track credits (composer, lyricist, arranger, producer,
//! engineer, ...) and the genre. Title, artist and featured-artist names,
//! audio, identifiers, release date and rights lines stay locked; those need
//! a new release.
//!
//! - The artist asks with the new values and a reason (one open request per
//!   release, only once a platform has the release).
//! - Staff with the review duty approve: credit names resolve to the org's
//!   parties and one `delivery.update` job goes to every platform still
//!   carrying the release. The catalog rows stay frozen (credits can only be
//!   edited on a draft, enforced in the database); the approved correction
//!   is applied on top wherever the release's metadata goes out ([`overlay`]):
//!   the update message, later first deliveries and partner feeds, and the
//!   current values the studio shows.
//! - Staff decline with a note the artist sees.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    preparation_model::PreparedRelease,
    staff::{self, Duty},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool, Row};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreditInput {
    pub name: String,
    pub role: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackCredits {
    pub track_id: Uuid,
    pub credits: Vec<CreditInput>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub tracks: Vec<TrackCredits>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestInput {
    pub changes: Changes,
    pub reason: String,
}

/// Artist and featured-artist credits are display names on the platforms:
/// they never change through a correction.
pub fn locked_role(role: &str) -> bool {
    matches!(
        crate::ddex_ern::contributor_role(role),
        "Artist" | "FeaturedArtist"
    )
}

fn invalid() -> Error {
    Error::InvalidCode("CORRECTION_INVALID")
}

/// Checked, normalized changes as stored (`{genre?, tracks:[{track_id, credits:[{name, role}]}]}`).
fn normalize(c: &Changes, tracks_of_release: &BTreeSet<Uuid>) -> Result<Value> {
    let mut out = serde_json::Map::new();
    if let Some(g) = &c.genre {
        let g = g.trim();
        if g.is_empty() || g.chars().count() > 60 {
            return Err(invalid());
        }
        crate::text_policy::check(g)?;
        out.insert("genre".into(), json!(g));
    }
    let mut seen = BTreeSet::new();
    let mut tracks = Vec::new();
    for t in &c.tracks {
        if !tracks_of_release.contains(&t.track_id)
            || !seen.insert(t.track_id)
            || t.credits.len() > 50
        {
            return Err(invalid());
        }
        let mut keys = BTreeSet::new();
        let mut credits = Vec::new();
        for cr in &t.credits {
            let (name, role) = (cr.name.trim(), cr.role.trim());
            if name.is_empty()
                || name.chars().count() > 200
                || role.is_empty()
                || role.chars().count() > 80
            {
                return Err(invalid());
            }
            if locked_role(role) {
                return Err(Error::InvalidCode("CORRECTION_ARTIST_LOCKED"));
            }
            crate::text_policy::check(name)?;
            crate::text_policy::check(role)?;
            if keys.insert((name.to_string(), role.to_string())) {
                credits.push(json!({"name": name, "role": role}));
            }
        }
        tracks.push(json!({"track_id": t.track_id, "credits": credits}));
    }
    if !tracks.is_empty() {
        out.insert("tracks".into(), json!(tracks));
    }
    if out.is_empty() {
        return Err(Error::InvalidCode("CORRECTION_EMPTY"));
    }
    Ok(Value::Object(out))
}

async fn release_tracks(c: &mut PgConnection, org: Uuid, release: Uuid) -> Result<BTreeSet<Uuid>> {
    Ok(sqlx::query_scalar(
        "SELECT id FROM catalog.tracks WHERE org_id=$1 AND release_id=$2 AND archived_at IS NULL",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *c)
    .await?
    .into_iter()
    .collect())
}

/// The artist asks for a correction of a delivered release.
pub async fn request(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: RequestInput,
) -> Result<Value> {
    let reason = i.reason.trim();
    if reason.is_empty() || reason.chars().count() > 1000 {
        return Err(Error::InvalidCode("CORRECTION_REASON_REQUIRED"));
    }
    crate::text_policy::check_multiline(reason)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let tracks = release_tracks(&mut tx, org, release).await?;
    let changes = normalize(&i.changes, &tracks)?;
    // Credited names are published: same protected-artist block as at submission.
    let names: Vec<String> = changes["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["credits"].as_array().into_iter().flatten())
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    crate::protected_names::enforce(&mut tx, org, &refs).await?;
    let (package, live) = crate::takedown::live_partners(&mut tx, org, release)
        .await?
        .ok_or(Error::PolicyGate("CORRECTION_NOTHING_SENT"))?;
    if live.is_empty() {
        return Err(Error::PolicyGate("CORRECTION_NOTHING_SENT"));
    }
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO portal.metadata_corrections(id, org_id, release_id, package_id, requested_by, changes, reason)
         VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT (release_id) WHERE status='PENDING' DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(release)
    .bind(package)
    .bind(a.user)
    .bind(&changes)
    .bind(reason)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or(Error::PolicyGate("CORRECTION_ALREADY_REQUESTED"))?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.correction_requested",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "PENDING"}))
}

pub async fn cancel(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let id: Uuid = sqlx::query_scalar(
        "UPDATE portal.metadata_corrections SET status='CANCELLED', decided_by=$3, decided_at=now()
          WHERE org_id=$1 AND release_id=$2 AND status='PENDING' RETURNING id",
    )
    .bind(org)
    .bind(release)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.correction_cancelled",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "CANCELLED"}))
}

/// Current editable values, the latest request and whether a new one is possible.
pub async fn status(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let live = crate::takedown::live_partners(&mut tx, org, release).await?;
    let latest: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'status',status,'changes',changes,'reason',reason,
                'decision_note',decision_note,'created_at',created_at,'decided_at',decided_at)
           FROM portal.metadata_corrections WHERE org_id=$1 AND release_id=$2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    // The submitted catalog values; approved corrections are applied below.
    let tracks: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('track_id',t.id,'title',t.title,'credits',
                COALESCE((SELECT jsonb_agg(jsonb_build_object('name',p.display_name,'role',c.role) ORDER BY c.role, p.display_name)
                            FROM catalog.credits c JOIN identity.parties p ON p.org_id=c.org_id AND p.id=c.party_id
                           WHERE c.org_id=t.org_id AND c.track_id=t.id),'[]'::jsonb))
           FROM catalog.tracks t WHERE t.org_id=$1 AND t.release_id=$2 AND t.archived_at IS NULL
          ORDER BY t.disc_number, t.track_number",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    let genre: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(NULLIF(draft->>'genreCustom',''), draft->>'genre') FROM catalog.releases WHERE org_id=$1 AND id=$2",
    )
    .bind(org)
    .bind(release)
    .fetch_one(&mut *tx)
    .await?;
    let applied: Vec<Value> = sqlx::query_scalar(
        "SELECT applied FROM portal.metadata_corrections
          WHERE org_id=$1 AND release_id=$2 AND status='APPROVED' ORDER BY decided_at, id",
    )
    .bind(org)
    .bind(release)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut genre = genre.filter(|g| g != "__other__");
    let mut tracks = tracks;
    for a in &applied {
        if let Some(g) = a.get("genre").and_then(Value::as_str) {
            genre = Some(g.to_string());
        }
        for fix in a
            .get("tracks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for t in tracks
                .iter_mut()
                .filter(|t| t["track_id"] == fix["track_id"])
            {
                let mut credits: Vec<Value> = t["credits"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|c| c["role"].as_str().is_some_and(locked_role))
                    .cloned()
                    .collect();
                credits.extend(
                    fix["credits"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|c| json!({"name": c["party_name"], "role": c["role"]})),
                );
                t["credits"] = json!(credits);
            }
        }
    }
    let can_request = live.as_ref().is_some_and(|(_, p)| !p.is_empty())
        && latest.as_ref().is_none_or(|l| l["status"] != "PENDING");
    Ok(json!({"request": latest, "can_request": can_request, "genre": genre, "tracks": tracks}))
}

// ---------------------------------------------------------------------------
// Staff
// ---------------------------------------------------------------------------
pub async fn staff_list(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Review)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let items: Vec<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',m.id,'org_id',m.org_id,'org_name',o.name,'release_id',m.release_id,'title',r.title,
                'status',m.status,'changes',m.changes,'reason',m.reason,'decision_note',m.decision_note,
                'created_at',m.created_at,'decided_at',m.decided_at,
                'tracks',(SELECT COALESCE(jsonb_agg(jsonb_build_object('track_id',t.id,'title',t.title) ORDER BY t.disc_number,t.track_number),'[]'::jsonb)
                            FROM catalog.tracks t WHERE t.org_id=m.org_id AND t.release_id=m.release_id))
           FROM portal.metadata_corrections m
           JOIN identity.orgs o ON o.id=m.org_id JOIN catalog.releases r ON r.id=m.release_id
          ORDER BY (m.status='PENDING') DESC, m.created_at DESC LIMIT 100",
    )
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInput {
    #[serde(default)]
    pub note: String,
}

/// The org's party with this display name, created when missing (credits
/// point at parties; a party is only a display name inside the org).
async fn party(c: &mut PgConnection, org: Uuid, name: &str) -> Result<Uuid> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM identity.parties WHERE org_id=$1 AND kind='PERSON' AND display_name=$2 ORDER BY id LIMIT 1",
    )
    .bind(org)
    .bind(name)
    .fetch_optional(&mut *c)
    .await?
    {
        return Ok(id);
    }
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON',$3)",
    )
    .bind(id)
    .bind(org)
    .bind(name)
    .execute(&mut *c)
    .await?;
    Ok(id)
}

/// Approve: record the resolved correction and send an update to every
/// platform still carrying the release.
pub async fn approve(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Review)?;
    staff::note_ok(&i.note, 1000)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row = sqlx::query("SELECT org_id, release_id, changes FROM portal.metadata_corrections WHERE id=$1 AND status='PENDING' FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::Conflict)?;
    let (org, release, changes): (Uuid, Uuid, Value) =
        (row.get("org_id"), row.get("release_id"), row.get("changes"));
    let mut applied = serde_json::Map::new();
    if let Some(g) = changes.get("genre").and_then(Value::as_str) {
        applied.insert("genre".into(), json!(g));
    }
    let mut applied_tracks = Vec::new();
    for t in changes
        .get("tracks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let track: Uuid = t["track_id"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .ok_or(Error::Internal)?;
        let mut credits = Vec::new();
        for c in t["credits"].as_array().into_iter().flatten() {
            let (name, role) = (
                c["name"].as_str().unwrap_or(""),
                c["role"].as_str().unwrap_or(""),
            );
            credits.push((
                party(&mut tx, org, name).await?,
                name.to_string(),
                role.to_string(),
            ));
        }
        applied_tracks.push(json!({
            "track_id": track,
            "credits": credits.iter().map(|(p, n, r)| json!({"party_id": p, "party_name": n, "role": r})).collect::<Vec<_>>(),
        }));
    }
    if !applied_tracks.is_empty() {
        applied.insert("tracks".into(), json!(applied_tracks));
    }
    sqlx::query(
        "UPDATE portal.metadata_corrections SET status='APPROVED', applied=$2, decided_by=$3, decided_at=now(), decision_note=$4 WHERE id=$1",
    )
    .bind(id)
    .bind(Value::Object(applied))
    .bind(st.actor.user)
    .bind(i.note.trim())
    .execute(&mut *tx)
    .await?;
    let mut jobs = Vec::new();
    if let Some((package, live)) = crate::takedown::live_partners(&mut tx, org, release).await? {
        for partner in live {
            jobs.push(
                operations::enqueue(
                    &mut tx,
                    "delivery",
                    "delivery.update",
                    &json!({"package_id": package, "partner_id": partner, "correction_id": id}),
                    &format!("delivery.update:{package}:{partner}:{id}"),
                    None,
                )
                .await?,
            );
        }
    }
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "staff.correction_approved",
        &format!("{} platform(s)", jobs.len()),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "APPROVED", "jobs": jobs}))
}

pub async fn decline(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Review)?;
    staff::note_ok(&i.note, 1000)?;
    if i.note.trim().is_empty() {
        return Err(Error::InvalidCode("CORRECTION_NOTE_REQUIRED"));
    }
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row = sqlx::query(
        "UPDATE portal.metadata_corrections SET status='DECLINED', decided_by=$2, decided_at=now(), decision_note=$3
          WHERE id=$1 AND status='PENDING' RETURNING org_id, release_id",
    )
    .bind(id)
    .bind(st.actor.user)
    .bind(i.note.trim())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        row.get("org_id"),
        row.get("release_id"),
        "staff.correction_declined",
        "DECLINED",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "DECLINED"}))
}

// ---------------------------------------------------------------------------
// Applying approved corrections to a frozen snapshot
// ---------------------------------------------------------------------------
/// Apply every approved correction of the release, oldest first, to a
/// release prepared from its frozen snapshot: track credits (artist-type
/// credits kept) and the genre, returned for partner feeds. The snapshot
/// and its hashes are untouched; only the outgoing metadata changes.
pub async fn overlay(pool: &PgPool, prepared: &mut PreparedRelease) -> Result<Option<String>> {
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT applied FROM portal.metadata_corrections
          WHERE org_id=$1 AND release_id=$2 AND status='APPROVED' ORDER BY decided_at, id",
    )
    .bind(prepared.org_id)
    .bind(prepared.release_id)
    .fetch_all(pool)
    .await?;
    let mut genre = None;
    for applied in rows {
        if let Some(g) = applied.get("genre").and_then(Value::as_str) {
            genre = Some(g.to_string());
        }
        let by_track: HashMap<Uuid, Vec<crate::distribution::CanonicalCredit>> = applied
            .get("tracks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let id: Uuid = t["track_id"].as_str()?.parse().ok()?;
                let credits = t["credits"]
                    .as_array()?
                    .iter()
                    .filter_map(|c| {
                        Some(crate::distribution::CanonicalCredit {
                            party_id: c["party_id"].as_str()?.parse().ok()?,
                            party_name: c["party_name"].as_str()?.to_string(),
                            role: c["role"].as_str()?.to_string(),
                        })
                    })
                    .collect();
                Some((id, credits))
            })
            .collect();
        for track in &mut prepared.canonical.tracks {
            if let Some(new) = by_track.get(&track.track_id) {
                track.credits.retain(|c| locked_role(&c.role));
                track.credits.extend(new.iter().cloned());
            }
        }
    }
    Ok(genre)
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
async fn h_request(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<RequestInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(request(&s, &a, org, id, i).await?))
}
async fn h_cancel(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(cancel(&s, &a, org, id).await?))
}
async fn h_staff_list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(staff_list(&s, &h).await?))
}
async fn h_approve(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    Ok(Json(approve(&s, &h, id, i).await?))
}
async fn h_decline(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<DecisionInput>,
) -> Result<Json<Value>> {
    Ok(Json(decline(&s, &h, id, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/releases/{id}/corrections",
            get(h_status).post(h_request),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/corrections/cancel",
            post(h_cancel),
        )
        .route("/api/staff/corrections", get(h_staff_list))
        .route("/api/staff/corrections/{id}/approve", post(h_approve))
        .route("/api/staff/corrections/{id}/decline", post(h_decline))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_fields_change() {
        let t = Uuid::new_v4();
        let tracks = BTreeSet::from([t]);
        let ok = Changes {
            genre: Some(" 발라드 ".into()),
            tracks: vec![TrackCredits {
                track_id: t,
                credits: vec![
                    CreditInput {
                        name: "김작곡".into(),
                        role: "Composer".into(),
                    },
                    CreditInput {
                        name: "김작곡".into(),
                        role: "Composer".into(),
                    },
                ],
            }],
        };
        let v = normalize(&ok, &tracks).unwrap();
        assert_eq!(v["genre"], "발라드");
        assert_eq!(
            v["tracks"][0]["credits"].as_array().unwrap().len(),
            1,
            "duplicates collapse"
        );
        let artist = Changes {
            genre: None,
            tracks: vec![TrackCredits {
                track_id: t,
                credits: vec![CreditInput {
                    name: "X".into(),
                    role: "Featured Artist".into(),
                }],
            }],
        };
        assert!(matches!(
            normalize(&artist, &tracks),
            Err(Error::InvalidCode("CORRECTION_ARTIST_LOCKED"))
        ));
        let other_track = Changes {
            genre: None,
            tracks: vec![TrackCredits {
                track_id: Uuid::new_v4(),
                credits: vec![],
            }],
        };
        assert!(normalize(&other_track, &tracks).is_err());
        assert!(matches!(
            normalize(
                &Changes {
                    genre: None,
                    tracks: vec![]
                },
                &tracks
            ),
            Err(Error::InvalidCode("CORRECTION_EMPTY"))
        ));
    }
}
