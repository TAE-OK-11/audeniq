//! Taking a release down from platforms (migration 0111).
//!
//! Withdrawal ([`crate::withdraw`]) covers applications nothing was sent
//! for. Once a platform has the release, the artist asks for a takedown with
//! a reason; staff with the delivery duty approve or decline it.
//!
//! - Approve: one `delivery.takedown` job per platform the latest package
//!   was delivered to and that has not taken it down yet. The job sends the
//!   ERN takedown ([`crate::execution::takedown_release`]); the binding
//!   moves to TAKEDOWN_REQUESTED, then TAKEN_DOWN on the partner's
//!   acknowledgement or poll.
//! - Platforms that take no takedown message (capability off, or the job
//!   fails) are confirmed by staff by hand: `delivery.mark_taken_down`.
//! - Every step notifies the org (trigger `takedown_requests_notice`; the
//!   live-binding trigger announces each platform that went down).
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
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};
use uuid::Uuid;

/// One platform of the release's latest package that received it.
struct Sent {
    partner_id: String,
    live_status: Option<String>,
}

/// The latest package of `release` and the platforms it was delivered to.
async fn delivered(
    c: &mut PgConnection,
    org: Uuid,
    release: Uuid,
) -> Result<Option<(Uuid, Vec<Sent>)>> {
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(org.to_string())
        .execute(&mut *c)
        .await?;
    let package: Option<Uuid> = sqlx::query_scalar(
        "SELECT dp.id FROM distribution.canonical_releases cr
           JOIN distribution.distribution_packages dp ON dp.canonical_release_id=cr.id
          WHERE cr.org_id=$1 AND cr.release_id=$2
          ORDER BY dp.created_at DESC LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *c)
    .await?;
    let Some(package) = package else {
        return Ok(None);
    };
    let rows = sqlx::query(
        "SELECT j.partner_id, b.live_status
           FROM execution.delivery_jobs j
           LEFT JOIN execution.live_bindings b ON b.org_id=j.org_id AND b.package_id=j.package_id AND b.partner_id=j.partner_id
          WHERE j.org_id=$1 AND j.package_id=$2 AND j.status='DELIVERED'
          ORDER BY j.partner_id",
    )
    .bind(org)
    .bind(package)
    .fetch_all(&mut *c)
    .await?;
    Ok(Some((
        package,
        rows.into_iter()
            .map(|r| Sent {
                partner_id: r.get("partner_id"),
                live_status: r.get("live_status"),
            })
            .collect(),
    )))
}

fn platforms(sent: &[Sent]) -> Value {
    Value::Array(
        sent.iter()
            .map(|p| {
                let dsp = crate::dsp_registry::Dsp::from_code(&p.partner_id);
                let (slug, name) = match dsp {
                    Some(d) => (d.spec().slug, d.spec().name_ko),
                    None if p.partner_id == "mockdsp" => {
                        (crate::mockdsp::STUDIO_PARTNER, crate::mockdsp::STUDIO_NAME)
                    }
                    None => ("", ""),
                };
                json!({
                    "partner_id": p.partner_id,
                    "dsp": dsp.map(|d| d.code()),
                    "slug": slug,
                    "name": name,
                    "live_status": p.live_status,
                })
            })
            .collect(),
    )
}

fn still_up(sent: &[Sent]) -> impl Iterator<Item = &Sent> {
    sent.iter()
        .filter(|p| p.live_status.as_deref() != Some("TAKEN_DOWN"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestInput {
    pub reason: String,
}

/// The artist asks for a takedown of a delivered release.
pub async fn request(
    s: &AppState,
    a: &Actor,
    org: Uuid,
    release: Uuid,
    i: RequestInput,
) -> Result<Value> {
    let reason = i.reason.trim();
    if reason.is_empty() {
        return Err(Error::InvalidCode("TAKEDOWN_REASON_REQUIRED"));
    }
    if reason.chars().count() > 1000 {
        return Err(Error::InvalidCode("NOTE_TOO_LONG"));
    }
    crate::text_policy::check_multiline(reason)?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let (package, sent) = delivered(&mut tx, org, release)
        .await?
        .ok_or(Error::PolicyGate("TAKEDOWN_NOTHING_SENT"))?;
    if sent.is_empty() {
        // Nothing reached a platform: the application is withdrawn instead.
        return Err(Error::PolicyGate("TAKEDOWN_NOTHING_SENT"));
    }
    if still_up(&sent).next().is_none() {
        return Err(Error::PolicyGate("TAKEDOWN_ALREADY_DONE"));
    }
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO portal.takedown_requests(id, org_id, release_id, package_id, requested_by, reason)
         VALUES($1,$2,$3,$4,$5,$6)
         ON CONFLICT (release_id) WHERE status='PENDING' DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(release)
    .bind(package)
    .bind(a.user)
    .bind(reason)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or(Error::PolicyGate("TAKEDOWN_ALREADY_REQUESTED"))?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.takedown_requested",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "PENDING", "platforms": platforms(&sent)}))
}

/// The artist withdraws a request staff have not decided yet.
pub async fn cancel(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", true).await?;
    let id: Option<Uuid> = sqlx::query_scalar(
        "UPDATE portal.takedown_requests SET status='CANCELLED', decided_by=$3, decided_at=now()
          WHERE org_id=$1 AND release_id=$2 AND status='PENDING' RETURNING id",
    )
    .bind(org)
    .bind(release)
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?;
    let id = id.ok_or(Error::NotFound)?;
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(org),
        Some(release),
        "release.takedown_cancelled",
        "ARTIST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "CANCELLED"}))
}

/// The latest request for `release` (if any) and each platform's state.
pub async fn status(s: &AppState, a: &Actor, org: Uuid, release: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, a, org, release, "release", false).await?;
    let sent = delivered(&mut tx, org, release).await?;
    let latest: Option<Value> = sqlx::query_scalar(
        "SELECT jsonb_build_object('id',id,'status',status,'reason',reason,'decision_note',decision_note,
                'created_at',created_at,'decided_at',decided_at)
           FROM portal.takedown_requests WHERE org_id=$1 AND release_id=$2
          ORDER BY created_at DESC LIMIT 1",
    )
    .bind(org)
    .bind(release)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    let (can_request, list) = match &sent {
        Some((_, p)) => (
            still_up(p).next().is_some()
                && latest.as_ref().is_none_or(|l| l["status"] != "PENDING"),
            platforms(p),
        ),
        None => (false, json!([])),
    };
    Ok(json!({"request": latest, "platforms": list, "can_request": can_request}))
}

/// Staff queue: open requests first, then the latest decided ones.
pub async fn staff_list(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let st = staff::staff(s, h, false).await?;
    staff::require(&st, Duty::Delivery)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let rows = sqlx::query(
        "SELECT t.id, t.org_id, o.name AS org_name, t.release_id, r.title, t.package_id, t.reason,
                t.status, t.decision_note, t.created_at, t.decided_at
           FROM portal.takedown_requests t
           JOIN identity.orgs o ON o.id=t.org_id
           JOIN catalog.releases r ON r.id=t.release_id
          ORDER BY (t.status='PENDING') DESC, t.created_at DESC LIMIT 100",
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut items = Vec::with_capacity(rows.len());
    for r in rows {
        let org: Uuid = r.get("org_id");
        let release: Uuid = r.get("release_id");
        let sent = delivered(&mut tx, org, release).await?;
        items.push(json!({
            "id": r.get::<Uuid,_>("id"), "org_id": org, "org_name": r.get::<String,_>("org_name"),
            "release_id": release, "title": r.get::<String,_>("title"),
            "package_id": r.get::<Uuid,_>("package_id"), "reason": r.get::<String,_>("reason"),
            "status": r.get::<String,_>("status"), "decision_note": r.get::<String,_>("decision_note"),
            "created_at": r.get::<chrono::DateTime<chrono::Utc>,_>("created_at"),
            "decided_at": r.get::<Option<chrono::DateTime<chrono::Utc>>,_>("decided_at"),
            "platforms": sent.as_ref().map(|(_, p)| platforms(p)).unwrap_or_else(|| json!([])),
        }));
    }
    tx.commit().await?;
    Ok(json!({"items": items}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionInput {
    #[serde(default)]
    pub note: String,
}

/// Approve: a takedown job per platform still carrying the release.
pub async fn approve(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    staff::note_ok(&i.note, 1000)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row = sqlx::query(
        "SELECT org_id, release_id FROM portal.takedown_requests WHERE id=$1 AND status='PENDING' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::Conflict)?;
    let (org, release): (Uuid, Uuid) = (row.get("org_id"), row.get("release_id"));
    let (package, sent) = delivered(&mut tx, org, release)
        .await?
        .ok_or(Error::PolicyGate("TAKEDOWN_NOTHING_SENT"))?;
    let mut jobs = Vec::new();
    for p in still_up(&sent) {
        jobs.push(
            operations::enqueue(
                &mut tx,
                "delivery",
                "delivery.takedown",
                &json!({"package_id": package, "partner_id": p.partner_id}),
                &format!("delivery.takedown:{package}:{}", p.partner_id),
                None,
            )
            .await?,
        );
    }
    sqlx::query(
        "UPDATE portal.takedown_requests SET status='APPROVED', decided_by=$2, decided_at=now(), decision_note=$3 WHERE id=$1",
    )
    .bind(id)
    .bind(st.actor.user)
    .bind(i.note.trim())
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(release),
        "staff.takedown_approved",
        &format!("{} platform(s)", jobs.len()),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"id": id, "status": "APPROVED", "package_id": package, "jobs": jobs, "platforms": platforms(&sent)}),
    )
}

/// Decline with a note the artist sees.
pub async fn decline(s: &AppState, h: &HeaderMap, id: Uuid, i: DecisionInput) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    if i.note.trim().is_empty() {
        return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
    }
    staff::note_ok(&i.note, 1000)?;
    let mut tx = s.pool.begin().await?;
    staff::staff_scope(&mut tx).await?;
    let row = sqlx::query(
        "UPDATE portal.takedown_requests SET status='DECLINED', decided_by=$2, decided_at=now(), decision_note=$3
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
        Some(row.get("org_id")),
        Some(row.get("release_id")),
        "staff.takedown_declined",
        "STAFF",
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "status": "DECLINED"}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualInput {
    pub note: String,
}

/// Staff confirm by hand that a platform took the release down (platforms
/// with no takedown message): the worker records TAKEN_DOWN.
pub async fn mark_taken_down(
    s: &AppState,
    h: &HeaderMap,
    package: Uuid,
    partner: &str,
    i: ManualInput,
) -> Result<Value> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Delivery)?;
    if !crate::partner_config::valid_partner_id(partner) {
        return Err(Error::NotFound);
    }
    if i.note.trim().is_empty() {
        return Err(Error::PolicyGate("REVIEW_NOTE_REQUIRED"));
    }
    staff::note_ok(&i.note, 1000)?;
    let mut tx = s.pool.begin().await?;
    let org: Uuid =
        sqlx::query_scalar("SELECT org_id FROM distribution.distribution_packages WHERE id=$1")
            .bind(package)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
    let job = operations::enqueue(
        &mut tx,
        "delivery",
        "delivery.mark_taken_down",
        &json!({"package_id": package, "partner_id": partner, "staff_user_id": st.actor.user}),
        &format!(
            "delivery.mark_taken_down:{package}:{partner}:{}",
            Uuid::new_v4()
        ),
        None,
    )
    .await?;
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        Some(org),
        Some(package),
        "staff.delivery_taken_down_recorded",
        &format!(
            "{partner}:{}",
            i.note.trim().chars().take(200).collect::<String>()
        ),
        st.actor.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"package_id": package, "partner_id": partner, "job_id": job}))
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
async fn h_status(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(status(&s, &a, org, id).await?))
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
async fn h_mark(
    State(s): State<AppState>,
    Path((package, partner)): Path<(Uuid, String)>,
    h: HeaderMap,
    Json(i): Json<ManualInput>,
) -> Result<Json<Value>> {
    Ok(Json(mark_taken_down(&s, &h, package, &partner, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/orgs/{org}/releases/{id}/takedown",
            get(h_status).post(h_request),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/takedown/cancel",
            post(h_cancel),
        )
        .route("/api/staff/takedowns", get(h_staff_list))
        .route("/api/staff/takedowns/{id}/approve", post(h_approve))
        .route("/api/staff/takedowns/{id}/decline", post(h_decline))
        .route(
            "/api/staff/deliveries/{package}/{partner}/taken-down",
            post(h_mark),
        )
}
