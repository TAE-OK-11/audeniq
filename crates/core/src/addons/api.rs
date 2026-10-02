use super::{model::*, workflow};
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    staff::{self, StaffRole},
};
use axum::{
    Json, Router,
    extract::{OriginalUri, Path, Query, State},
    http::HeaderMap,
    routing::{get, post, put},
};
use serde_json::{Value, json};
use sqlx::PgConnection;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    let mut r = Router::new()
        .route("/api/addons/catalog", get(catalog))
        .route("/api/orgs/{org}/addons/orders", post(create).get(list))
        .route("/api/orgs/{org}/addons/orders/{id}", get(detail))
        .route("/api/orgs/{org}/addons/orders/{id}/cancel", post(cancel))
        .route("/api/orgs/{org}/addons/orders/{id}/details", put(revise))
        .route("/api/orgs/{org}/addons/orders/{id}/revisions", post(revise))
        .route(
            "/api/orgs/{org}/addons/orders/{id}/follow-ups",
            post(revise),
        )
        .route("/api/admin/addons/catalog/{code}", put(update_catalog))
        .route("/api/admin/addons/orders", get(admin_list))
        .route("/api/admin/addons/orders/{id}", get(admin_detail))
        .route("/api/admin/addons/orders/{id}/evidence", post(evidence))
        .route("/api/admin/addons/orders/{id}/results", post(results));
    for action in [
        "assign",
        "status",
        "request-info",
        "approve",
        "reject",
        "complete",
        "payment",
        "refund",
    ] {
        r = r.route(
            &format!("/api/admin/addons/orders/{{id}}/{action}"),
            post(admin_action),
        );
    }
    r
}
async fn catalog(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    auth::actor(&s.pool, &h, &s.config, false).await?;
    let mut c = s.pool.acquire().await?;
    Ok(Json(workflow::catalog(&mut c).await?))
}
async fn create(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<CreateOrder>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    auth::rate(&s.pool, &format!("addon-create:{}", a.user), 120).await?;
    let key = h
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or(Error::InvalidCode("IDEMPOTENCY_KEY_REQUIRED"))?;
    Ok(Json(workflow::create(&s.pool, &a, org, key, i).await?))
}
async fn list(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Query(f): Query<Filters>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let mut tx = s.pool.begin().await?;
    let v = workflow::list(&mut tx, Some(&a), Some(org), &f).await?;
    tx.commit().await?;
    Ok(Json(v))
}
async fn detail(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, &a, org, false).await?;
    let o = workflow::load(&mut tx, org, id).await?;
    workflow::authorize_order(&mut tx, &a, &o, false).await?;
    let v = workflow::view(&mut tx, org, id).await?;
    tx.commit().await?;
    Ok(Json(v))
}
async fn cancel(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<Action>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, &a, org, true).await?;
    let o = workflow::load(&mut tx, org, id).await?;
    workflow::authorize_order(&mut tx, &a, &o, true).await?;
    workflow::version(&o, i.row_version)?;
    workflow::transition(&mut tx, Some(&a), org, id, Status::CANCELLED, &i.reason).await?;
    let v = workflow::view(&mut tx, org, id).await?;
    tx.commit().await?;
    Ok(Json(v))
}
async fn revise(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<Revise>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, &a, org, true).await?;
    let o = workflow::load(&mut tx, org, id).await?;
    workflow::revise(&mut tx, &a, &o, &i).await?;
    let v = workflow::view(&mut tx, org, id).await?;
    tx.commit().await?;
    Ok(Json(v))
}

/// Recheck and lock the existing staff ACL in the write transaction. Revocation
/// cannot race an authenticated admin request or cross-tenant query.
async fn admin(c: &mut PgConnection, a: &Actor) -> Result<()> {
    let role: Option<String> = sqlx::query_scalar("SELECT identity.lock_active_staff_role($1)")
        .bind(a.user)
        .fetch_one(&mut *c)
        .await?;
    if role.as_deref() != Some("ADMIN") {
        return Err(Error::Forbidden);
    }
    sqlx::query("SELECT set_config('app.staff','on',true)")
        .execute(c)
        .await?;
    Ok(())
}
async fn admin_actor(s: &AppState, h: &HeaderMap, write: bool) -> Result<Actor> {
    let st = staff::staff(s, h, write).await?;
    if st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    Ok(st.actor)
}
async fn admin_order(c: &mut PgConnection, id: Uuid) -> Result<Order> {
    let org: Uuid = sqlx::query_scalar("SELECT org_id FROM catalog.addon_orders WHERE id=$1")
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    workflow::load(c, org, id).await
}
async fn admin_list(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(f): Query<Filters>,
) -> Result<Json<Value>> {
    let a = admin_actor(&s, &h, false).await?;
    let mut tx = s.pool.begin().await?;
    admin(&mut tx, &a).await?;
    let v = workflow::list(&mut tx, None, None, &f).await?;
    tx.commit().await?;
    Ok(Json(v))
}
async fn admin_detail(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = admin_actor(&s, &h, false).await?;
    let mut tx = s.pool.begin().await?;
    admin(&mut tx, &a).await?;
    let o = admin_order(&mut tx, id).await?;
    let mut v = workflow::view(&mut tx, o.org_id, id).await?;
    let audit:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(e) FROM operations.audit_events e WHERE resource_id=$1 AND org_id=$2 ORDER BY occurred_at DESC,id DESC LIMIT 200").bind(id).bind(o.org_id).fetch_all(&mut *tx).await?;
    v["audit_events"] = json!(audit);
    tx.commit().await?;
    Ok(Json(v))
}
async fn admin_action(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    OriginalUri(uri): OriginalUri,
    h: HeaderMap,
    Json(i): Json<Action>,
) -> Result<Json<Value>> {
    let a = admin_actor(&s, &h, true).await?;
    let mut tx = s.pool.begin().await?;
    admin(&mut tx, &a).await?;
    let o = admin_order(&mut tx, id).await?;
    workflow::version(&o, i.row_version)?;
    workflow::text(&i.reason, 1000, true)?;
    match uri.path().rsplit('/').next().ok_or(Error::Invalid)? {
        "assign" => {
            if let Some(user) = i.assigned_admin_user_id {
                let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM identity.staff_members sm JOIN identity.users u ON u.id=sm.user_id WHERE sm.user_id=$1 AND sm.status='ACTIVE' AND sm.role='ADMIN' AND u.status='ACTIVE')").bind(user).fetch_one(&mut *tx).await?;
                if !valid {
                    return Err(Error::Invalid);
                }
            }
            let before: Option<Uuid> = sqlx::query_scalar(
                "SELECT assigned_admin_user_id FROM catalog.addon_orders WHERE id=$1",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            sqlx::query("UPDATE catalog.addon_orders SET assigned_admin_user_id=$3,row_version=row_version+1 WHERE org_id=$1 AND id=$2").bind(o.org_id).bind(id).bind(i.assigned_admin_user_id).execute(&mut *tx).await?;
            workflow::audit(
                &mut tx,
                Some(&a),
                o.org_id,
                id,
                "addon.assigned",
                &i.reason,
                json!({"assigned_admin_user_id":before}),
                json!({"assigned_admin_user_id":i.assigned_admin_user_id}),
            )
            .await?;
        }
        "payment" => {
            workflow::pay(
                &mut tx,
                &a,
                &o,
                i.payment_reference.as_deref().ok_or(Error::Invalid)?,
                &i.reason,
            )
            .await?;
        }
        "refund" => {
            workflow::refund(
                &mut tx,
                &a,
                &o,
                i.refund_reference.as_deref().ok_or(Error::Invalid)?,
                &i.reason,
            )
            .await?;
        }
        "status" => {
            let next = i.status.ok_or(Error::Invalid)?;
            if next == Status::PAID {
                workflow::pay(
                    &mut tx,
                    &a,
                    &o,
                    i.payment_reference.as_deref().ok_or(Error::Invalid)?,
                    &i.reason,
                )
                .await?;
            } else {
                workflow::transition(&mut tx, Some(&a), o.org_id, id, next, &i.reason).await?;
            }
            workflow::audit(
                &mut tx,
                Some(&a),
                o.org_id,
                id,
                "addon.admin.override",
                &i.reason,
                json!({"status":o.status}),
                json!({"status":next}),
            )
            .await?;
        }
        "request-info" => {
            workflow::transition(
                &mut tx,
                Some(&a),
                o.org_id,
                id,
                Status::NEEDS_INFO,
                &i.reason,
            )
            .await?;
            workflow::audit(
                &mut tx,
                Some(&a),
                o.org_id,
                id,
                "addon.info.requested",
                &i.reason,
                json!({"status":o.status}),
                json!({"status":"NEEDS_INFO","message":i.reason}),
            )
            .await?;
        }
        "approve" | "reject" => {
            if o.status == "QUEUED" {
                workflow::transition(
                    &mut tx,
                    Some(&a),
                    o.org_id,
                    id,
                    Status::UNDER_REVIEW,
                    &i.reason,
                )
                .await?;
            }
            let next = if uri.path().ends_with("/approve") {
                Status::APPROVED
            } else {
                Status::REJECTED
            };
            workflow::transition(&mut tx, Some(&a), o.org_id, id, next, &i.reason).await?;
        }
        "complete" => {
            workflow::transition(
                &mut tx,
                Some(&a),
                o.org_id,
                id,
                Status::COMPLETED,
                &i.reason,
            )
            .await?;
        }
        _ => return Err(Error::Invalid),
    }
    let v = workflow::view(&mut tx, o.org_id, id).await?;
    tx.commit().await?;
    Ok(Json(v))
}
async fn evidence(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<EvidenceDecision>,
) -> Result<Json<Value>> {
    let a = admin_actor(&s, &h, true).await?;
    let mut tx = s.pool.begin().await?;
    admin(&mut tx, &a).await?;
    let o = admin_order(&mut tx, id).await?;
    workflow::decide_evidence(&mut tx, &a, &o, &i).await?;
    let v = workflow::view(&mut tx, o.org_id, id).await?;
    tx.commit().await?;
    Ok(Json(v))
}
async fn results(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<Results>,
) -> Result<Json<Value>> {
    let a = admin_actor(&s, &h, true).await?;
    let mut tx = s.pool.begin().await?;
    admin(&mut tx, &a).await?;
    let o = admin_order(&mut tx, id).await?;
    workflow::record_results(&mut tx, &a, &o, &i).await?;
    let v = workflow::view(&mut tx, o.org_id, id).await?;
    tx.commit().await?;
    Ok(Json(v))
}
async fn update_catalog(
    State(s): State<AppState>,
    Path(code): Path<String>,
    h: HeaderMap,
    Json(i): Json<CatalogUpdate>,
) -> Result<Json<Value>> {
    let a = admin_actor(&s, &h, true).await?;
    let mut tx = s.pool.begin().await?;
    admin(&mut tx, &a).await?;
    let v = workflow::update_catalog(&mut tx, &a, &code, &i).await?;
    tx.commit().await?;
    Ok(Json(v))
}
