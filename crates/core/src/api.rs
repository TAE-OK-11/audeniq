use crate::{
    auth,
    catalog::{self, Kind},
    config::Config,
    drafts, dsp_registry,
    error::{Error, Result},
    review, routing,
    storage::ObjectStore,
    submission, uploads,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, MatchedPath, Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::Instrument;
use uuid::Uuid;
#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Config,
    pub storage: Arc<dyn ObjectStore>,
    pub password_slots: Arc<Semaphore>,
    /// Bound R2 completion IO before opening database transactions.
    pub upload_slots: Arc<Semaphore>,
    /// Lossless → FLAC conversions at upload completion (CPU and temp disk).
    /// Held for the conversion only, not for downloads or uploads.
    pub transcode_slots: Arc<Semaphore>,
    /// How long a completion waits in line for `transcode_slots` before
    /// answering UPLOAD_BUSY (the semaphore is FIFO). A conversion takes
    /// seconds, so waiting beats the client's 2–10 s retry backoff; it stays
    /// well under the edge's 100 s request limit.
    pub transcode_wait: std::time::Duration,
    /// Inline images (electronic signatures, profile images): a few KB each,
    /// so they never wait behind an audio conversion in `transcode_slots`.
    pub inline_slots: Arc<Semaphore>,
    pub dummy_hash: String,
    pub payout_keys: Arc<crate::payout_keys::KeyRing>,
    pub antivirus_enabled: bool,
    /// AI assistant access (crate::mcp). Off unless AUDENIQ_MCP_ENABLED.
    pub mcp: Arc<crate::mcp::Settings>,
    /// The staff operations login (crate::staff_ops), only when
    /// `AUDENIQ_STAFF_OPS_DATABASE_URL` is set: the admin console's former
    /// CLI-only operations are unavailable without it.
    pub staff_ops: Option<PgPool>,
    service_secret_digest: [u8; 32],
}
impl AppState {
    pub async fn new(pool: PgPool, config: Config, storage: Arc<dyn ObjectStore>) -> Result<Self> {
        let service_secret_digest = auth::token_digest(&config.service_secret);
        auth::init_rate_key(&config.service_secret);
        Ok(Self {
            pool,
            config,
            storage,
            password_slots: Arc::new(Semaphore::new(2)),
            upload_slots: Arc::new(Semaphore::new(6)),
            transcode_slots: Arc::new(Semaphore::new(1)),
            transcode_wait: std::time::Duration::from_secs(45),
            inline_slots: Arc::new(Semaphore::new(2)),
            payout_keys: Arc::new(crate::payout_keys::KeyRing::from_env(false)?),
            antivirus_enabled: crate::config::antivirus_enabled()?,
            mcp: Arc::new(crate::mcp::Settings::from_env()),
            staff_ops: crate::staff_ops::pool_from_env()?,
            dummy_hash: auth::password_hash(auth::random_token()).await?,
            service_secret_digest,
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
        .route("/api/auth/passkey/options", post(passkey_login_options))
        .route("/api/auth/passkey/login", post(passkey_login))
        .route("/api/auth/passkeys", get(passkey_list))
        .route("/api/auth/passkeys/options", post(passkey_register_options))
        .route("/api/auth/passkeys/register", post(passkey_register))
        .route(
            "/api/auth/passkeys/{id}",
            axum::routing::delete(passkey_remove),
        )
        .route("/api/document-keys", get(document_keys))
        .route("/api/print-stamps", post(print_stamp))
        .route("/api/print-stamps/verify", get(print_verify))
        .route("/api/auth/oauth/providers", get(oauth_providers))
        .route("/api/auth/oauth/identities", get(oauth_identities))
        .route(
            "/api/auth/oauth/identities/{id}",
            axum::routing::delete(oauth_unlink),
        )
        .route("/api/auth/oauth/{provider}/start", post(oauth_start))
        .route(
            "/api/auth/oauth/{provider}/callback",
            get(oauth_callback_get).post(oauth_callback_post),
        )
        .route("/api/me", get(me))
        .route("/api/orgs", post(create_org).get(orgs))
        .route("/api/orgs/{org}/memberships", put(member))
        .route(
            "/api/orgs/{org}/memberships/accept",
            post(accept_membership),
        )
        .route("/api/orgs/{org}/resources/{id}/acl", put(acl))
        .route("/api/orgs/{org}/uploads", post(upload))
        .route("/api/orgs/{org}/uploads/{id}", get(upload_status))
        .route("/api/orgs/{org}/uploads/{id}/cancel", post(cancel_upload))
        .route("/api/orgs/{org}/uploads/{id}/complete", post(complete))
        .route(
            "/api/orgs/{org}/uploads/{id}/parts",
            get(upload_parts).post(upload_part_urls),
        )
        .route("/api/orgs/{org}/assets/{id}", get(asset))
        .route("/api/orgs/{org}/assets/{id}/checks", get(asset_checks))
        .route("/api/orgs/{org}/assets/{id}/image", get(asset_image))
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
        .route(
            "/api/orgs/{org}/releases/{id}/delivery",
            get(delivery_status),
        )
        .route("/api/orgs/{org}/releases/{id}/presubmit", get(presubmit))
        .route(
            "/api/orgs/{org}/releases/{id}/consents",
            post(create_consent),
        )
        .route("/api/orgs/{org}/releases/{id}/submit", post(submit))
        .route(
            "/api/orgs/{org}/releases/{id}/withdraw",
            post(withdraw_release),
        )
        .route("/api/orgs/{org}/withdrawals", get(withdraw_quota))
        .route(
            "/api/orgs/{org}/releases/{id}/submission",
            get(submission_status),
        )
        .route(
            "/api/orgs/{org}/reviews/overrides",
            post(create_override).get(list_override_requests),
        )
        .route(
            "/api/orgs/{org}/reviews/overrides/{request}/approve",
            post(approve_override),
        )
        .route(
            "/api/orgs/{org}/reviews/overrides/{request}/decline",
            post(decline_override),
        )
        .route("/api/orgs/{org}/parties", post(create_party))
        .route("/api/orgs/{org}/dsps", get(available_dsps))
        .route("/api/orgs/{org}/{kind}", post(create).get(list))
        .route(
            "/api/orgs/{org}/{kind}/{id}",
            get(detail).put(update).delete(archive),
        )
        .merge(crate::portal::routes())
        .merge(crate::account::routes())
        .merge(crate::password_reset::routes())
        .merge(crate::email::routes())
        .merge(crate::signing::routes())
        .merge(crate::staff::routes())
        .merge(crate::staff_ops::routes())
        .merge(crate::identity_desk::routes())
        .merge(crate::work::routes())
        .merge(crate::live::routes())
        .merge(crate::members::routes())
        .merge(crate::takedown::routes())
        .merge(crate::corrections::routes())
        .merge(crate::infringement::routes())
        .merge(crate::pitching::routes())
        .merge(crate::smart_links::routes())
        .merge(crate::codes::routes())
        .merge(crate::release_alerts::routes())
        .merge(crate::presave::routes())
        .merge(crate::splits::routes())
        .merge(crate::platform_requests::routes())
        .merge(crate::contract_terms::routes())
        .merge(crate::artist_pages::routes())
        .merge(crate::artist_links::routes())
        .merge(crate::artist_community::routes())
        .merge(crate::artist_studio::routes())
        .merge(crate::artist_press::routes())
        .merge(crate::profanity::routes())
        .merge(crate::partner_hooks::routes())
        .merge(crate::site_content::routes())
        .merge(crate::mcp::routes())
        .fallback(|| async { Error::NotFound.into_response() })
        // Innermost: the boundary middleware still stamps the request id,
        // no-store and the access log on a panic's 500.
        .layer(tower_http::catch_panic::CatchPanicLayer::custom(
            panic_response,
        ))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(request_deadline))
        // 서비스 비밀 확인 뒤: 점검 중 API 차단 (crate::site_content)
        .layer(middleware::from_fn_with_state(
            s.clone(),
            crate::site_content::gate,
        ))
        .layer(middleware::from_fn_with_state(s.clone(), boundary))
        // boundary의 Origin 검사 전에: 관리자·설문 화면의 경로·Origin 규칙 (crate::sites)
        .layer(middleware::from_fn_with_state(
            s.clone(),
            crate::sites::policy,
        ))
        .layer(middleware::from_fn(header_limit))
        .layer(compression())
        .with_state(s)
}
/// Past this a request is abandoned: the tunnel in front gave up on it
/// already (about 100 s), and it would otherwise keep holding a database
/// connection (pool of 6) or a storage call for up to 15 minutes. Covers the
/// handler up to its response headers; a streamed body is not cut.
const REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(120);

/// Not bounded: live streams (they end on their own) and upload completion
/// (cancel-safe, and retried by the client while the conversion runs).
fn unbounded(path: &str) -> bool {
    path.ends_with("/live") || (path.starts_with("/api/orgs/") && path.ends_with("/complete"))
}

async fn request_deadline(req: Request, next: middleware::Next) -> Response {
    if unbounded(req.uri().path()) {
        return next.run(req).await;
    }
    let route = req.uri().path().to_owned();
    match tokio::time::timeout(REQUEST_DEADLINE, next.run(req)).await {
        Ok(response) => response,
        Err(_) => {
            tracing::warn!(%route, "request abandoned at the deadline");
            Error::Timeout.into_response()
        }
    }
}

/// A handler bug that panics answers like any other server error (500 with
/// the standard error body) and is logged, instead of dropping the
/// connection, which the edge reports as "Private API unavailable".
fn panic_response(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detail = err
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| err.downcast_ref::<&str>().copied())
        .unwrap_or("non-string panic");
    tracing::error!(panic = detail, "request handler panicked");
    Error::Internal.into_response()
}
/// JSON responses are compressed for whichever encoding the caller accepts
/// (brotli, then gzip; the edge forwards the browser's Accept-Encoding).
/// Both codecs are pure Rust; zstd was dropped with its C library (the
/// Workers only ever ask the API for br/gzip, web/shared/transport.ts). Level 4: most of the size win of the maximum levels at
/// a fraction of the CPU (brotli's default is 11, far too slow per request).
/// Bodies under 1 KiB are sent as is: the framing costs more than it saves.
fn compression() -> tower_http::compression::CompressionLayer<
    tower_http::compression::predicate::And<
        tower_http::compression::predicate::DefaultPredicate,
        tower_http::compression::predicate::SizeAbove,
    >,
> {
    use tower_http::compression::{
        CompressionLayer, CompressionLevel,
        predicate::{DefaultPredicate, Predicate, SizeAbove},
    };
    CompressionLayer::new()
        .quality(CompressionLevel::Precise(4))
        .compress_when(DefaultPredicate::new().and(SizeAbove::new(1024)))
}
/// Total request-header budget. Browsers and the edge send a few KiB; hyper
/// alone accepted a single 200 KB header (sandbox round 2).
pub const MAX_HEADER_BYTES: usize = 16 * 1024;
async fn header_limit(req: Request, next: Next) -> Response {
    let total: usize = req
        .headers()
        .iter()
        .map(|(k, v)| k.as_str().len() + v.as_bytes().len() + 4)
        .sum();
    if total > MAX_HEADER_BYTES {
        return Error::HeadersTooLarge.into_response();
    }
    next.run(req).await
}
async fn boundary(State(s): State<AppState>, mut req: Request, next: Next) -> Response {
    let id = Uuid::new_v4();
    let provided = req
        .headers()
        .get("x-audeniq-service")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !auth::secret_matches_digest(provided, &s.service_secret_digest) {
        return Error::Forbidden.into_response();
    }
    let request_id = HeaderValue::from_str(&id.to_string()).expect("uuid is a valid header");
    req.headers_mut().insert("x-request-id", request_id.clone());
    // No user identity header is read. Writes require Origin even before login.
    // Partner webhooks carry no browser Origin; they are authenticated by
    // the partner's HMAC signature in the handler (crate::partner_hooks).
    let partner_hook = *req.method() == Method::POST
        && req
            .uri()
            .path()
            .starts_with(crate::partner_hooks::HOOK_PREFIX);
    // Apple's sign-in returns with a cross-site form POST; the one-time state
    // and the code exchange authenticate it (crate::oauth::callback).
    let oauth_form_post =
        *req.method() == Method::POST && req.uri().path() == "/api/auth/oauth/apple/callback";
    // AI clients (crate::mcp) call the MCP and OAuth endpoints server to
    // server: bearer token / PKCE / client credentials authenticate them.
    let mcp_client = crate::mcp::origin_exempt(req.method(), req.uri().path());
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)
        && !partner_hook
        && !oauth_form_post
        && !mcp_client
        && auth::origin(req.headers(), &s.config).is_err()
    {
        return Error::Forbidden.into_response();
    }
    // Derive audit metadata here, after service authentication. Never trust
    // browser supplied copies and never persist bearer tokens from raw paths.
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|v| v.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".into());
    let method = req.method().as_str().to_owned();
    let resource = req
        .uri()
        .path()
        .split('/')
        .filter_map(|v| Uuid::parse_str(v).ok())
        .next_back();
    for name in [
        "x-audeniq-audit-route",
        "x-audeniq-audit-method",
        "x-audeniq-audit-resource",
    ] {
        req.headers_mut().remove(name);
    }
    if let Ok(value) = HeaderValue::from_str(&route) {
        req.headers_mut().insert("x-audeniq-audit-route", value);
    }
    req.headers_mut().insert(
        "x-audeniq-audit-method",
        HeaderValue::from_str(&method).expect("valid method"),
    );
    if let Some(resource) = resource {
        req.headers_mut().insert(
            "x-audeniq-audit-resource",
            HeaderValue::from_str(&resource.to_string()).expect("uuid"),
        );
    }
    // Correlation fields on every log line of this request; release-scoped
    // routes also carry the release id (see docs/PIPELINE_ARCHITECTURE.md).
    let span = tracing::info_span!(
        "http",
        request_id = %id,
        method = %req.method(),
        path = %route,
        release_id = tracing::field::Empty,
    );
    if let Some(release) = release_in_path(req.uri().path()) {
        span.record("release_id", tracing::field::display(release));
    }
    let started = std::time::Instant::now();
    let mut response = next.run(req).instrument(span.clone()).await;
    let _entered = span.enter();
    let headers = response.headers_mut();
    headers.insert("x-request-id", request_id);
    // no-store unless the handler chose a browser-only (private) policy for
    // content-addressed bytes (a stored cover image keyed by its hash).
    let private_cache = headers
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("private,") && v.contains("immutable"));
    if !private_cache {
        headers.insert("cache-control", HeaderValue::from_static("no-store"));
    }
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    // Validation failures (4xx) log their stable error code with the request
    // id so a 400 can be traced to the check that rejected it (BE-005).
    match response.extensions().get::<crate::error::ErrorCode>() {
        Some(code) => {
            tracing::info!(request_id=%id,status=response.status().as_u16(),error_code=code.0,elapsed_ms=started.elapsed().as_millis(),"http_request")
        }
        None => {
            tracing::info!(request_id=%id,status=response.status().as_u16(),elapsed_ms=started.elapsed().as_millis(),"http_request")
        }
    }
    response
}
/// The release a request path is about: `/api/orgs/{org}/releases/{id}/...`
/// and `/api/staff/releases/{id}/...`.
fn release_in_path(path: &str) -> Option<Uuid> {
    let mut parts = path.split('/');
    while let Some(p) = parts.next() {
        if p == "releases" {
            return parts.next().and_then(|id| Uuid::parse_str(id).ok());
        }
    }
    None
}
async fn ready(State(s): State<AppState>) -> Result<Json<Value>> {
    // Doubles as the database check: contracted partners switched on.
    let live_partners: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM execution.adapter_profiles WHERE delivery_enabled AND activation_kind='CONTRACTED'",
    )
    .fetch_one(&s.pool)
    .await?;
    Ok(Json(json!({"database":true,"submission":false,
               "distribution":live_partners > 0 && crate::launch::live_transmission_enabled(),
               "live_partners":live_partners,
               "live_transmission":crate::launch::live_transmission_enabled(),"payout":false})))
}
async fn available_dsps(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(org): Path<Uuid>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(dsp_availability(&s, &a, org).await?))
}
/// Platforms this workspace can request, with whether each is routable now
/// (Studio platform step, MCP `list_distribution_platforms`).
pub async fn dsp_availability(s: &AppState, a: &auth::Actor, org: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    auth::membership(&mut tx, a, org, false).await?;
    tx.commit().await?;
    let ids: Vec<Uuid> = dsp_registry::Dsp::ALL.iter().map(|d| d.uuid()).collect();
    let decisions = routing::public_routes(&s.pool, org, &ids).await?;
    let mut items: Vec<Value> = dsp_registry::REGISTRY
        .iter()
        .zip(decisions.iter())
        .map(|(spec, route)| {
            json!({"slug":spec.slug,"name":spec.name_ko,"region":spec.region,
                                    "category":spec.category,"available":route.routable})
        })
        .collect();
    // Pre-launch test platform: the in-process MockDSP (migration 0087).
    if crate::mockdsp::studio_route(&s.pool, org).await?.is_some() {
        items.push(
            json!({"slug":crate::mockdsp::STUDIO_PARTNER,"name":crate::mockdsp::STUDIO_NAME,
                          "region":"GLOBAL","category":"STREAMING","available":true}),
        );
    }
    Ok(json!({"items":items}))
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
/// Public keys of the document seals and archive manifests (verification).
async fn document_keys(State(s): State<AppState>) -> Json<Value> {
    Json(crate::seal::public_keys(&s.payout_keys))
}
async fn print_stamp(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<crate::print_stamp::StampInput>,
) -> Result<Json<Value>> {
    Ok(Json(crate::print_stamp::stamp(&s, &h, i).await?))
}
async fn print_verify(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<crate::print_stamp::VerifyQuery>,
) -> Result<Json<Value>> {
    Ok(Json(crate::print_stamp::verify(&s, &h, q).await?))
}
async fn oauth_providers() -> Json<Value> {
    Json(crate::oauth::providers())
}
async fn oauth_identities(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(crate::oauth::identities(&s, &h).await?))
}
async fn oauth_unlink(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(crate::oauth::unlink(&s, &h, id).await?))
}
async fn oauth_start(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(provider): Path<String>,
    Json(i): Json<crate::oauth::StartInput>,
) -> Result<Json<Value>> {
    Ok(Json(crate::oauth::start(&s, &h, &provider, i).await?))
}
async fn oauth_callback_get(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(provider): Path<String>,
    Query(p): Query<crate::oauth::CallbackParams>,
) -> Response {
    crate::oauth::callback(&s, &h, &provider, p).await
}
/// Apple returns with a form POST (response_mode=form_post).
async fn oauth_callback_post(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(provider): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let mut p = crate::oauth::CallbackParams::default();
    for (k, v) in url::form_urlencoded::parse(&body) {
        match &*k {
            "code" => p.code = Some(v.into_owned()),
            "state" => p.state = Some(v.into_owned()),
            "error" => p.error = Some(v.into_owned()),
            _ => {}
        }
    }
    crate::oauth::callback(&s, &h, &provider, p).await
}
async fn passkey_login_options(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(crate::passkey::login_options(&s, &h).await?))
}
async fn passkey_login(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<crate::passkey::LoginInput>,
) -> Result<(HeaderMap, Json<Value>)> {
    crate::passkey::login(&s, &h, i).await
}
async fn passkey_list(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(crate::passkey::list(&s, &h).await?))
}
async fn passkey_register_options(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    Ok(Json(crate::passkey::register_options(&s, &h).await?))
}
async fn passkey_register(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<crate::passkey::RegisterInput>,
) -> Result<Json<Value>> {
    Ok(Json(crate::passkey::register(&s, &h, i).await?))
}
async fn passkey_remove(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(crate::passkey::remove(&s, &h, id).await?))
}
async fn logout(State(s): State<AppState>, h: HeaderMap) -> Result<(HeaderMap, Json<Value>)> {
    auth::logout(&s, &h).await
}
async fn csrf(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    auth::csrf(&s, &h).await
}
async fn me(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let email = crate::pii::user_email(&s.payout_keys, &s.pool, a.user).await?;
    Ok(Json(
        json!({"user_id":a.user,"party_id":a.party,"email":email}),
    ))
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
    crate::text_policy::check(&i.name)?;
    let mut tx = s.pool.begin().await?;
    let id = Uuid::new_v4();
    let party = Uuid::new_v4();
    // The org name becomes the party credited on tracks: a brand-new org
    // holds no protected-name exception.
    crate::protected_names::enforce(&mut tx, id, &[i.name.as_str()]).await?;
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartyInput {
    display_name: String,
}
/// A named person that track credits (composer, lyricist, ...) point at.
/// It is only a display name inside the org: no rights, no verified identity.
/// The same name in the same org returns the existing party.
async fn create_party(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<PartyInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    let (id, created) = catalog::ensure_party(&s, &a, org, &i.display_name).await?;
    Ok(Json(json!({"party_id":id,"created":created})))
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
/// Per-DSP delivery status of the release's latest staged package.
async fn delivery_status(
    State(s): State<AppState>,
    Path((org, release)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let mut tx = s.pool.begin().await?;
    auth::authorize(&mut tx, &a, org, release, "release", false).await?;
    tx.commit().await?;
    Ok(Json(
        crate::delivery_staging::release_delivery_view(&s.pool, org, release).await?,
    ))
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
    Json(i): Json<submission::SubmitInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(submission::submit(&s, &a, org, id, i).await?))
}
async fn withdraw_release(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        crate::withdraw::withdraw_by_artist(&s.pool, &a, org, id).await?,
    ))
}
async fn withdraw_quota(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(crate::withdraw::quota(&s.pool, &a, org).await?))
}
async fn presubmit(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(submission::presubmit(&s, &a, org, id).await?))
}
async fn create_consent(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<submission::ConsentInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(submission::create_consent(&s, &a, org, id, i).await?))
}
async fn submission_status(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(submission::submission_status(&s, &a, org, id).await?))
}
async fn create_override(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
    Json(i): Json<review::OverrideInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(review::create_override_api(&s, &a, org, i).await?))
}
async fn list_override_requests(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(review::list_override_requests(&s, &a, org).await?))
}
async fn approve_override(
    State(s): State<AppState>,
    Path((org, request)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        review::approve_override_request(&s, &a, org, request).await?,
    ))
}
async fn decline_override(
    State(s): State<AppState>,
    Path((org, request)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(
        review::decline_override_request(&s, &a, org, request).await?,
    ))
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
async fn upload_parts(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(uploads::stored_parts(&s, &a, org, id).await?))
}
async fn upload_part_urls(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(i): Json<uploads::PartsInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(uploads::part_urls(&s, &a, org, id, i).await?))
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
async fn asset_checks(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(uploads::checks(&s, &a, org, id).await?))
}
async fn asset_image(
    State(s): State<AppState>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
) -> Result<Response> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    let img = uploads::image(&s, &a, org, id).await?;
    Ok(uploads::image_response(img, &h))
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
async fn accept_membership(
    State(s): State<AppState>,
    Path(org): Path<Uuid>,
    h: HeaderMap,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(catalog::accept_membership(&s, &a, org).await?))
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
            test_only_bypass_dsp_gate: false,
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
    #[tokio::test]
    async fn json_is_compressed_for_the_accepted_encoding() {
        use http_body_util::BodyExt;
        let big = json!({"items": (0..200).map(|i| json!({"id": i, "status": "READY_FOR_DELIVERY", "upc": null})).collect::<Vec<_>>()});
        let raw_len = serde_json::to_vec(&big).unwrap().len();
        let app = Router::new()
            .route(
                "/big",
                get(move || {
                    let big = big.clone();
                    async move { Json(big) }
                }),
            )
            .route("/small", get(|| async { Json(json!({"ok": true})) }))
            .layer(compression());
        let call = |path: &'static str, accept: &'static str| {
            let app = app.clone();
            async move {
                let r = app
                    .oneshot(
                        axum::http::Request::builder()
                            .uri(path)
                            .header("accept-encoding", accept)
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let enc = r
                    .headers()
                    .get("content-encoding")
                    .map(|v| v.to_str().unwrap().to_string());
                let len = r.into_body().collect().await.unwrap().to_bytes().len();
                (enc, len)
            }
        };
        // Browsers also offer zstd; brotli wins, then gzip.
        let (enc, len) = call("/big", "gzip, deflate, br, zstd").await;
        assert_eq!(enc.as_deref(), Some("br"));
        assert!(len * 5 < raw_len, "br {len} vs {raw_len} bytes");
        assert_eq!(call("/big", "zstd").await, (None, raw_len));
        let (enc, len) = call("/big", "gzip, br").await;
        assert_eq!(enc.as_deref(), Some("br"));
        assert!(len * 5 < raw_len, "br {len} vs {raw_len} bytes");
        assert_eq!(call("/big", "gzip").await.0.as_deref(), Some("gzip"));
        assert_eq!(
            call("/big", "zstd;q=0, br;q=0, gzip").await.0.as_deref(),
            Some("gzip")
        );
        assert_eq!(
            call("/big", "zstd;q=1, br;q=0.2").await.0.as_deref(),
            Some("br")
        );
        // No Accept-Encoding, or a tiny body: sent as is.
        assert_eq!(call("/big", "identity").await, (None, raw_len));
        assert_eq!(call("/small", "br").await.0, None);

        // Exercise actual decoding, not only headers or compressed size.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        for encoding in ["br", "gzip"] {
            let decoded: Value = client
                .get(format!("http://{address}/big"))
                .header("accept-encoding", encoding)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(serde_json::to_vec(&decoded).unwrap().len(), raw_len);
            assert_eq!(decoded["items"][199]["status"], "READY_FOR_DELIVERY");
        }
        server.abort();
    }

    #[tokio::test]
    async fn compression_preserves_streaming_and_already_compressed_media() {
        let app = Router::new()
            .route(
                "/sse",
                get(|| async {
                    (
                        [("content-type", "text/event-stream")],
                        "data: ready\n\n".repeat(200),
                    )
                }),
            )
            .route(
                "/image",
                get(|| async { ([("content-type", "image/png")], vec![0_u8; 4096]) }),
            )
            .layer(compression());
        for path in ["/sse", "/image"] {
            let response = app
                .clone()
                .oneshot(
                    axum::http::Request::builder()
                        .uri(path)
                        .header("accept-encoding", "zstd, br, gzip")
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert!(!response.headers().contains_key("content-encoding"));
        }
    }

    #[tokio::test]
    async fn a_panicking_handler_answers_500_with_the_error_body() {
        let app: Router = Router::new()
            .route(
                "/boom",
                get(|| async {
                    panic!("handler bug");
                    #[allow(unreachable_code)]
                    ""
                }),
            )
            .layer(tower_http::catch_panic::CatchPanicLayer::custom(
                panic_response,
            ));
        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/boom")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(res.into_body(), 1 << 16)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["error"]["code"], "INTERNAL_ERROR");
    }
}
