use crate::{
    auth,
    catalog::{self, Kind},
    config::Config,
    drafts,
    error::{Error, Result},
    storage::ObjectStore,
    uploads,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{HeaderMap, Method},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub storage: Arc<dyn ObjectStore>,
    pub password_slots: Arc<Semaphore>,
    pub dummy_hash: String,
}
impl AppState {
    pub async fn new(pool: PgPool, config: Config, storage: Arc<dyn ObjectStore>) -> Result<Self> {
        Ok(Self {
            pool,
            config,
            storage,
            password_slots: Arc::new(Semaphore::new(2)),
            dummy_hash: auth::password_hash(auth::random_token()).await?,
        })
    }
}
pub fn router(s: AppState) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"service":"audeniq-api","operational_release":false})) }),
        )
        .route("/ready", get(ready))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/csrf", post(csrf))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/logout-all", post(logout_all))
        .route("/api/auth/sessions", get(sessions))
        .route("/api/auth/sessions/{id}/revoke", post(revoke_session))
        .route("/api/auth/password", post(change_password))
        .route("/api/me", get(me))
        .route("/api/orgs", post(create_org).get(orgs))
        .route("/api/orgs/{org}/memberships", put(member))
        .route("/api/orgs/{org}/resources/{id}/acl", put(acl))
        .route("/api/orgs/{org}/uploads", post(upload))
        .route("/api/orgs/{org}/uploads/{id}", get(upload_status))
        .route("/api/orgs/{org}/uploads/{id}/cancel", post(cancel_upload))
        .route("/api/orgs/{org}/uploads/{id}/complete", post(complete))
        .route("/api/orgs/{org}/assets/{id}", get(asset))
        .route("/api/orgs/{org}/releases/{id}/tracks", post(track))
        .route(
            "/api/orgs/{org}/releases/{id}/tracks/{track}",
            put(replace_track).delete(archive_track),
        )
        .route(
            "/api/orgs/{org}/releases/{id}/tracks/{track}/credits",
            put(replace_credits),
        )
        .route("/api/orgs/{org}/releases/{id}/preflight", get(preflight))
        .route("/api/orgs/{org}/releases/{id}/submit", post(submit))
        .route("/api/orgs/{org}/{kind}", post(create).get(list))
        .route(
            "/api/orgs/{org}/{kind}/{id}",
            get(detail).put(update).delete(archive),
        )
        .fallback(|| async { Error::NotFound.into_response() })
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(s.clone(), boundary))
        .with_state(s)
}
async fn boundary(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    let id = Uuid::new_v4();
    let provided = req
        .headers()
        .get("x-audeniq-service")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !auth::secret_eq(provided, &s.config.service_secret) {
        return Error::Forbidden.into_response();
    }
    req.headers_mut()
        .insert("x-request-id", id.to_string().parse().unwrap());
    // No user identity header is read. Writes require Origin even before login.
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        && auth::origin(req.headers(), &s.config).is_err()
    {
        return Error::Forbidden.into_response();
    }
    let started = std::time::Instant::now();
    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("x-request-id", id.to_string().parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    tracing::info!(request_id=%id,status=response.status().as_u16(),elapsed_ms=started.elapsed().as_millis(),"http_request");
    response
}
async fn ready(State(s): State<AppState>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&s.pool).await?;
    Ok(Json(
        json!({"database":true,"submission":false,"distribution":false,"payout":false}),
    ))
}
async fn register(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<auth::Credentials>,
) -> Result<Json<Value>> {
    auth::register(&s, &h, i).await
}
async fn login(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<auth::Credentials>,
) -> Result<(HeaderMap, Json<Value>)> {
    auth::login(&s, &h, i).await
}
async fn logout(State(s): State<AppState>, h: HeaderMap) -> Result<(HeaderMap, Json<Value>)> {
    auth::logout(&s, &h).await
}
async fn csrf(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    auth::csrf(&s, &h).await
}
async fn me(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(json!({"user_id":a.user,"party_id":a.party})))
}
async fn orgs(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',o.id,'name',o.name,'kind',o.kind,'role',m.role) FROM identity.orgs o JOIN identity.memberships m ON m.org_id=o.id WHERE m.user_id=$1 AND m.status='ACTIVE' ORDER BY o.id LIMIT 100").bind(a.user).fetch_all(&s.pool).await?;
    Ok(Json(json!({"items":rows})))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrgInput {
    name: String,
    kind: String,
}
async fn create_org(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<OrgInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    if i.name.is_empty() || i.name.len() > 200 || !matches!(i.kind.as_str(), "LABEL" | "COMPANY") {
        return Err(Error::Invalid);
    }
    let mut tx = s.pool.begin().await?;
    let id = Uuid::new_v4();
    let party = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id,name,kind) VALUES($1,$2,$3)")
        .bind(id)
        .bind(&i.name)
        .bind(i.kind)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'LEGAL_ENTITY',$3)",
    )
    .bind(party)
    .bind(id)
    .bind(i.name)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role) VALUES($1,$2,'OWNER')")
        .bind(id)
        .bind(a.user)
        .execute(&mut *tx)
        .await?;
    crate::operations::audit(
        &mut tx,
        Some(a.user),
        Some(id),
        Some(id),
        "org.created",
        "USER_REQUEST",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"org_id":id,"party_id":party})))
}
async fn create(
    State(s): State<AppState>,
    Path((org, k)): Path<(Uuid, String)>,
    h: HeaderMap,
    Json(i): Json<catalog::Input>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        catalog::create(&s, &a, org, Kind::parse(&k)?, i).await?,
    ))
}
async fn list(
    State(s): State<AppState>,
    Path((org, k)): Path<(Uuid, String)>,
    Query(page): Query<catalog::Page>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(
        catalog::list(&s, &a, org, Kind::parse(&k)?, page).await?,
    ))
}
async fn detail(
    State(s): State<AppState>,
    Path((org, k, id)): Path<(Uuid, String, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(catalog::get(&s, &a, org, Kind::parse(&k)?, id).await?))
}
async fn update(
    State(s): State<AppState>,
    Path((org, k, id)): Path<(Uuid, String, Uuid)>,
    h: HeaderMap,
    Json(i): Json<catalog::Input>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        catalog::update(&s, &a, org, Kind::parse(&k)?, id, i).await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    row_version: i64,
}
async fn archive(
    State(s): State<AppState>,
    Path((org, k, id)): Path<(Uuid, String, Uuid)>,
    h: HeaderMap,
    Json(i): Json<Version>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        catalog::archive(&s, &a, org, Kind::parse(&k)?, id, i.row_version).await?,
    ))
}
async fn track(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<catalog::TrackInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(catalog::track(&s, &a, org, id, i).await?))
}
async fn replace_track(
    State(s): State<AppState>,
    Path((org, release, track)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<catalog::TrackInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        drafts::replace_track(&s, &a, org, release, track, i).await?,
    ))
}
async fn archive_track(
    State(s): State<AppState>,
    Path((org, release, track)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<Version>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        drafts::archive_track(&s, &a, org, release, track, i.row_version).await?,
    ))
}
async fn replace_credits(
    State(s): State<AppState>,
    Path((org, release, track)): Path<(Uuid, Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<drafts::CreditsInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        drafts::replace_credits(&s, &a, org, release, track, i).await?,
    ))
}
async fn preflight(
    State(s): State<AppState>,
    Path((org, release)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(drafts::preflight(&s, &a, org, release).await?))
}
async fn sessions(
    State(s): State<AppState>,
    Query(page): Query<catalog::Page>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    auth::sessions(&s, &h, page).await
}
async fn revoke_session(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    h: HeaderMap,
) -> Result<(HeaderMap, Json<Value>)> {
    auth::revoke_session(&s, &h, id).await
}
async fn logout_all(State(s): State<AppState>, h: HeaderMap) -> Result<(HeaderMap, Json<Value>)> {
    auth::logout_all(&s, &h).await
}
async fn change_password(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<auth::PasswordChange>,
) -> Result<(HeaderMap, Json<Value>)> {
    auth::change_password(&s, &h, i).await
}
async fn submit(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, &a, org, id, "release", true).await?;
    Err(Error::Gated)
}
async fn upload(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<uploads::UploadInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(uploads::issue(&s, &a, org, i).await?))
}
async fn upload_status(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(uploads::status(&s, &a, org, id).await?))
}
async fn cancel_upload(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(uploads::cancel(&s, &a, org, id).await?))
}
async fn complete(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<uploads::CompleteInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(uploads::complete(&s, &a, org, id, i).await?))
}
async fn asset(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(uploads::get(&s, &a, org, id).await?))
}
async fn member(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<catalog::MemberInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(catalog::member(&s, &a, org, i).await?))
}
async fn acl(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<catalog::AclInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(catalog::acl(&s, &a, org, id, i).await?))
}
#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn service_boundary_does_not_accept_forged_identity() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let config = Config {
            database_url: String::new(),
            origin: "https://studio.audeniq.test".into(),
            service_secret: "s".repeat(32),
            secure_cookie: true,
            bind: String::new(),
            session_seconds: 60,
        };
        let s = AppState::new(
            pool,
            config.clone(),
            Arc::new(crate::storage::DisabledStore),
        )
        .await
        .unwrap();
        let app = router(s);
        let request = axum::http::Request::builder()
            .uri("/health")
            .header("x-user-id", Uuid::new_v4().to_string())
            .header("x-audeniq-service", "forged")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            axum::http::StatusCode::FORBIDDEN
        );
        let request = axum::http::Request::builder()
            .uri("/health")
            .header("x-audeniq-service", &config.service_secret)
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}
