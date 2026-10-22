//! AUDENIQ REST API (`/api/v1`) and developer apps, end to end against real
//! PostgreSQL through the API router (as the edge would call it).
//!
//! An app a Studio user registers connects with authorization code + PKCE
//! for the API resource and calls the same tools as the MCP server over
//! REST. Tokens stay on the resource they were issued for.
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    mcp::{Settings, developer},
};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;

mod support;
use support::{MemStore, ORIGIN, SECRET, User, call, user};

const REDIRECT: &str = "https://app.example.test/callback";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

async fn app_with(pool: PgPool, api: bool) -> Router {
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(MemStore::default());
    let mut s = AppState::new(
        pool,
        Config {
            database_url: String::new(),
            origin: ORIGIN.into(),
            service_secret: SECRET.into(),
            secure_cookie: false,
            bind: "127.0.0.1:0".into(),
            session_seconds: 3600,
            test_only_bypass_dsp_gate: true,
        },
        store,
    )
    .await
    .unwrap();
    s.mcp = Arc::new(Settings {
        enabled: true,
        cimd_hosts: vec!["clients.example.test".into()],
        dynamic_registration: true,
        payout_limit_krw: 300_000,
        public_daily_user: 150,
        api_enabled: api,
    });
    router(s)
}

/// A request as an app's server sends it (service secret from the edge).
async fn raw(
    app: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: String,
) -> (StatusCode, HeaderMap, String) {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("x-audeniq-service", SECRET);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let r = app
        .clone()
        .oneshot(b.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let bytes = axum::body::to_bytes(r.into_body(), 4 << 20).await.unwrap();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// An API call with a bearer token; the body as JSON.
async fn api(
    app: &Router,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let auth = format!("Bearer {token}");
    let mut headers = vec![("authorization", auth.as_str())];
    if body.is_some() {
        headers.push(("content-type", "application/json"));
    }
    let (s, _, b) = raw(
        app,
        method,
        path,
        &headers,
        body.map(|v| v.to_string()).unwrap_or_default(),
    )
    .await;
    (s, serde_json::from_str(&b).unwrap_or(Value::Null))
}

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

fn query_param(url: &str, key: &str) -> Option<String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn basic(client_id: &str, secret: &str) -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:{secret}"))
    )
}

fn authorize_query(client_id: &str, scope: &str, resource: &str) -> String {
    format!(
        "/api/oauth/authorize?{}",
        form(&[
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", REDIRECT),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("state", "st-1"),
            ("scope", scope),
            ("resource", &format!("{ORIGIN}{resource}")),
        ])
    )
}

/// The authorize link for a signed-in user: the consent request and what the
/// Studio's consent page shows, or the redirect/page the server answered.
async fn authorize(
    app: &Router,
    u: &User,
    client_id: &str,
    resource: &str,
) -> Result<(String, Value), (StatusCode, String, String)> {
    let (s, h, body) = raw(
        app,
        "GET",
        &authorize_query(
            client_id,
            "releases:read releases:write settlement:read",
            resource,
        ),
        &[("cookie", &u.cookie)],
        String::new(),
    )
    .await;
    let loc = h
        .get("location")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    if s != StatusCode::SEE_OTHER || !loc.starts_with(&format!("{ORIGIN}/ai/connect?request=")) {
        return Err((s, loc, body));
    }
    let request = query_param(&loc, "request").unwrap();
    let (s, v) = call(
        app,
        "GET",
        &format!(
            "/api/auth/ai-connect?request={}",
            url::form_urlencoded::byte_serialize(request.as_bytes()).collect::<String>()
        ),
        json!(null),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Ok((request, v))
}

/// Approves a consent request (read + write); the code from the redirect.
async fn approve(app: &Router, u: &User, request: &str) -> String {
    let (s, v) = call(
        app,
        "POST",
        "/api/auth/ai-connect",
        json!({"request": request, "decision": "approve", "org": u.org, "scopes": ["releases:read", "releases:write"]}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let to = v["redirect"].as_str().unwrap();
    assert!(to.starts_with(REDIRECT), "{to}");
    query_param(to, "code").unwrap()
}

async fn token(
    app: &Router,
    client_id: &str,
    secret: &str,
    pairs: &[(&str, &str)],
) -> (StatusCode, Value) {
    let (s, _, body) = raw(
        app,
        "POST",
        "/api/oauth/token",
        &[
            ("content-type", "application/x-www-form-urlencoded"),
            ("authorization", &basic(client_id, secret)),
        ],
        form(pairs),
    )
    .await;
    (s, serde_json::from_str(&body).unwrap_or(Value::Null))
}

async fn exchange(
    app: &Router,
    client_id: &str,
    secret: &str,
    code: &str,
    resource: &str,
) -> (StatusCode, Value) {
    token(
        app,
        client_id,
        secret,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", VERIFIER),
            ("redirect_uri", REDIRECT),
            ("resource", &format!("{ORIGIN}{resource}")),
        ],
    )
    .await
}

/// A new developer app of `u`: (row id, client_id, client_secret).
async fn new_app(app: &Router, u: &User) -> (String, String, String) {
    let (s, v) = call(
        app,
        "POST",
        "/api/developer/apps",
        json!({"name": " Release Helper ", "redirect_uris": [REDIRECT], "description": "Prepares releases", "website": "https://app.example.test"}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["name"], "Release Helper");
    assert_eq!(v["visibility"], "PRIVATE");
    assert_eq!(v["public_client"], false);
    assert_eq!(v["connections"], 0);
    (
        v["id"].as_str().unwrap().into(),
        v["client_id"].as_str().unwrap().into(),
        v["client_secret"].as_str().unwrap().into(),
    )
}

/// The owner connects their own app: an API access and refresh token.
async fn connect(app: &Router, u: &User, client_id: &str, secret: &str) -> (String, String) {
    let (request, details) = authorize(app, u, client_id, "/api/v1").await.unwrap();
    assert_eq!(details["audience"], "api");
    assert_eq!(details["client"]["developer"], true);
    assert_eq!(details["client"]["private"], true);
    assert_eq!(details["client"]["website"], "https://app.example.test");
    let code = approve(app, u, &request).await;
    let (s, t) = exchange(app, client_id, secret, &code, "/api/v1").await;
    assert_eq!(s, StatusCode::OK, "{t}");
    (
        t["access_token"].as_str().unwrap().into(),
        t["refresh_token"].as_str().unwrap().into(),
    )
}

#[sqlx::test]
async fn developer_app_connects_its_owner_and_calls_the_api(pool: PgPool) {
    let app = app_with(pool.clone(), true).await;
    let owner = user(&app).await;
    let (_, client_id, secret) = new_app(&app, &owner).await;

    // The list shows the app (no secret) and how to reach the API.
    let (s, v) = call(
        &app,
        "GET",
        "/api/developer/apps",
        json!(null),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["items"][0]["client_id"], client_id);
    assert!(v["items"][0].get("client_secret").is_none());
    assert_eq!(v["api_base"], format!("{ORIGIN}/api/v1"));
    assert_eq!(v["resource"], format!("{ORIGIN}/api/v1"));

    let (access, refresh) = connect(&app, &owner, &client_id, &secret).await;

    // Same tools as MCP, over REST.
    let (s, me) = api(&app, &access, "GET", "/api/v1/me", None).await;
    assert_eq!(s, StatusCode::OK, "{me}");
    assert!(me.get("view").is_none());
    let (s, rel) = api(
        &app,
        &access,
        "POST",
        "/api/v1/releases",
        Some(json!({"title": "Night Drive", "release_type": "SINGLE", "artist_name": "Han River", "genre": "pop"})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{rel}");
    let release = rel["release_id"].as_str().unwrap().to_string();
    let (s, got) = api(
        &app,
        &access,
        "GET",
        &format!("/api/v1/releases/{release}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{got}");
    assert_eq!(got["artist_name"], "Han River");
    let (s, list) = api(&app, &access, "GET", "/api/v1/releases?limit=5", None).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    let (s, v) = api(
        &app,
        &access,
        "GET",
        &format!("/api/v1/releases/{release}/validation"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["ready_to_submit"], false);

    // Errors: schema, conflicting values, unknown path, wrong method.
    let (s, e) = api(
        &app,
        &access,
        "PATCH",
        &format!("/api/v1/releases/{release}"),
        Some(json!({"no_such_field": 1})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    assert_eq!(e["error"]["code"], "INVALID_ARGUMENTS");
    let (s, e) = api(
        &app,
        &access,
        "PATCH",
        &format!("/api/v1/releases/{release}"),
        Some(json!({"release_id": uuid::Uuid::new_v4(), "genre": "rock"})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    let (s, e) = api(&app, &access, "GET", "/api/v1/nope", None).await;
    assert_eq!(
        (s, e["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("NOT_FOUND"))
    );
    let (s, _) = api(&app, &access, "PUT", "/api/v1/releases", None).await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
    let (s, e) = api(&app, &access, "POST", "/api/v1/releases", None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");

    // Scopes: settlement was not granted.
    let (s, h, body) = raw(
        &app,
        "GET",
        "/api/v1/settlement",
        &[("authorization", &format!("Bearer {access}"))],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{body}");
    assert!(body.contains("INSUFFICIENT_SCOPE"));
    assert!(
        h["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("insufficient_scope")
    );

    // No token, a browser page of another site.
    let (s, h, _) = raw(&app, "GET", "/api/v1/me", &[], String::new()).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let challenge = h["www-authenticate"].to_str().unwrap();
    assert!(
        challenge.contains("/.well-known/oauth-protected-resource/api/v1"),
        "{challenge}"
    );
    assert!(!challenge.contains("error="));
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/v1/releases",
        &[
            ("authorization", &format!("Bearer {access}")),
            ("origin", "https://evil.example"),
            ("content-type", "application/json"),
        ],
        json!({"title": "X", "release_type": "SINGLE", "artist_name": "A"}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // The API token is not an MCP token.
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[
            ("authorization", &format!("Bearer {access}")),
            ("content-type", "application/json"),
            ("accept", "application/json, text/event-stream"),
            ("mcp-protocol-version", "2025-06-18"),
        ],
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Refresh: for its own resource only; the form secret works too.
    let (s, e) = token(
        &app,
        &client_id,
        &secret,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("resource", &format!("{ORIGIN}/api/mcp")),
        ],
    )
    .await;
    assert_eq!(
        (s, e["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_target")),
        "{e}"
    );
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/token",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &client_id),
            ("client_secret", &secret),
            ("resource", &format!("{ORIGIN}/api/v1")),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let t: Value = serde_json::from_str(&body).unwrap();
    let access = t["access_token"].as_str().unwrap().to_string();
    assert_eq!(
        api(&app, &access, "GET", "/api/v1/me", None).await.0,
        StatusCode::OK
    );

    // A code for the API cannot become an MCP token.
    let (request, _) = authorize(&app, &owner, &client_id, "/api/v1")
        .await
        .unwrap();
    let code = approve(&app, &owner, &request).await;
    let (s, e) = exchange(&app, &client_id, &secret, &code, "/api/mcp").await;
    assert_eq!(
        (s, e["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_target")),
        "{e}"
    );

    // A developer app is for the API, not the MCP endpoint.
    let Err((s, loc, _)) = authorize(&app, &owner, &client_id, "/api/mcp").await else {
        panic!("a developer app connected to MCP")
    };
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(
        query_param(&loc, "error").as_deref(),
        Some("invalid_target")
    );

    // The connection shows in the user's connected apps and the app's count.
    let (_, v) = call(
        &app,
        "GET",
        "/api/auth/ai-connections",
        json!(null),
        Some(&owner),
    )
    .await;
    assert!(
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["client_id"] == client_id)
    );
    let (_, v) = call(
        &app,
        "GET",
        "/api/developer/apps",
        json!(null),
        Some(&owner),
    )
    .await;
    assert_eq!(v["items"][0]["connections"], 1);

    // The OpenAPI document and resource metadata need no account.
    let (s, _, body) = raw(&app, "GET", "/api/v1/openapi.json", &[], String::new()).await;
    assert_eq!(s, StatusCode::OK);
    let doc: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["servers"][0]["url"], format!("{ORIGIN}/api/v1"));
    assert!(doc["paths"]["/releases/{release_id}"]["get"].is_object());
    let (s, _, body) = raw(
        &app,
        "GET",
        "/.well-known/oauth-protected-resource/api/v1",
        &[],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let meta: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(meta["resource"], format!("{ORIGIN}/api/v1"));
    assert_eq!(meta["authorization_servers"][0], ORIGIN);
}

#[sqlx::test]
async fn private_until_reviewed_and_deleting_ends_connections(pool: PgPool) {
    let app = app_with(pool.clone(), true).await;
    let owner = user(&app).await;
    let other = user(&app).await;
    let (id, client_id, secret) = new_app(&app, &owner).await;

    // Another artist cannot connect a private app.
    let Err((s, _, body)) = authorize(&app, &other, &client_id, "/api/v1").await else {
        panic!("a private app reached another user's consent screen")
    };
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body.contains("만든 사람만"), "{body}");

    // Review: the owner asks, an operator approves; then anyone can connect.
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/developer/apps/{id}/review"),
        json!({}),
        Some(&owner),
    )
    .await;
    assert_eq!(
        (s, v["visibility"].as_str()),
        (StatusCode::OK, Some("REVIEW")),
        "{v}"
    );
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/developer/apps/{id}/review"),
        json!({}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(
        authorize(&app, &other, &client_id, "/api/v1")
            .await
            .is_err()
    );
    assert_eq!(
        developer::review(&pool, "ops", &client_id, true, None)
            .await
            .unwrap(),
        "PUBLIC"
    );
    let (request, details) = authorize(&app, &other, &client_id, "/api/v1")
        .await
        .unwrap();
    assert_eq!(details["client"]["private"], false);
    let code = approve(&app, &other, &request).await;
    let (s, t) = exchange(&app, &client_id, &secret, &code, "/api/v1").await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let other_access = t["access_token"].as_str().unwrap().to_string();
    assert_eq!(
        api(&app, &other_access, "GET", "/api/v1/me", None).await.0,
        StatusCode::OK
    );

    // Taking it back ends other artists' connections.
    developer::review(
        &pool,
        "ops",
        &client_id,
        false,
        Some("Needs a privacy policy"),
    )
    .await
    .unwrap();
    assert_eq!(
        api(&app, &other_access, "GET", "/api/v1/me", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, v) = call(
        &app,
        "GET",
        "/api/developer/apps",
        json!(null),
        Some(&owner),
    )
    .await;
    assert_eq!(v["items"][0]["visibility"], "PRIVATE");
    assert_eq!(v["items"][0]["review_note"], "Needs a privacy policy");

    // Only the owner manages it.
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/developer/apps/{id}/secret"),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &app,
        "PATCH",
        &format!("/api/developer/apps/{id}"),
        json!({"redirect_uris": ["http://bad.example/cb"]}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A new secret: the old one stops at once.
    let (access, refresh) = connect(&app, &owner, &client_id, &secret).await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/developer/apps/{id}/secret"),
        json!({}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let new_secret = v["client_secret"].as_str().unwrap().to_string();
    let (s, _) = token(
        &app,
        &client_id,
        &secret,
        &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, t) = token(
        &app,
        &client_id,
        &new_secret,
        &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let access2 = t["access_token"].as_str().unwrap().to_string();

    // Deleting the app ends every connection to it.
    let (s, _) = call(
        &app,
        "DELETE",
        &format!("/api/developer/apps/{id}"),
        json!(null),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    for t in [&access, &access2] {
        assert_eq!(
            api(&app, t, "GET", "/api/v1/me", None).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    let (_, v) = call(
        &app,
        "GET",
        "/api/developer/apps",
        json!(null),
        Some(&owner),
    )
    .await;
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
}

#[sqlx::test]
async fn mcp_clients_and_tokens_stay_off_the_api(pool: PgPool) {
    let app = app_with(pool.clone(), true).await;
    let u = user(&app).await;
    // An AI assistant registered itself (DCR): MCP only.
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/register",
        &[("content-type", "application/json")],
        json!({"client_name": "Assistant", "redirect_uris": [REDIRECT], "token_endpoint_auth_method": "none"}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");
    let dcr: Value = serde_json::from_str(&body).unwrap();
    let dcr = dcr["client_id"].as_str().unwrap();
    let Err((s, loc, _)) = authorize(&app, &u, dcr, "/api/v1").await else {
        panic!("an anonymous client reached the API consent")
    };
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(
        query_param(&loc, "error").as_deref(),
        Some("invalid_target")
    );

    // Its MCP token does not open the API.
    let (request, details) = authorize(&app, &u, dcr, "/api/mcp").await.unwrap();
    assert_eq!(details["audience"], "mcp");
    let code = approve(&app, &u, &request).await;
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/token",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", VERIFIER),
            ("client_id", dcr),
            ("redirect_uri", REDIRECT),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let t: Value = serde_json::from_str(&body).unwrap();
    let mcp_access = t["access_token"].as_str().unwrap();
    let (s, _) = api(&app, mcp_access, "GET", "/api/v1/me", None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Ten apps per user.
    for _ in 0..developer::MAX_APPS_PER_USER {
        let (s, v) = call(
            &app,
            "POST",
            "/api/developer/apps",
            json!({"name": "A", "redirect_uris": [REDIRECT], "public_client": true}),
            Some(&u),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert!(v.get("client_secret").is_none());
    }
    let (s, v) = call(
        &app,
        "POST",
        "/api/developer/apps",
        json!({"name": "A", "redirect_uris": [REDIRECT]}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("APP_LIMIT_REACHED")),
        "{v}"
    );
}

#[sqlx::test]
async fn api_and_developer_apps_are_off_unless_enabled(pool: PgPool) {
    let app = app_with(pool.clone(), false).await;
    let u = user(&app).await;
    for path in [
        "/api/v1/me",
        "/api/v1/openapi.json",
        "/.well-known/oauth-protected-resource/api/v1",
    ] {
        let (s, _, _) = raw(&app, "GET", path, &[], String::new()).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (s, _) = call(&app, "GET", "/api/developer/apps", json!(null), Some(&u)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // The API resource is unknown while the API is off.
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/register",
        &[("content-type", "application/json")],
        json!({"client_name": "Assistant", "redirect_uris": [REDIRECT], "token_endpoint_auth_method": "none"}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let dcr: Value = serde_json::from_str(&body).unwrap();
    let Err((_, loc, _)) = authorize(&app, &u, dcr["client_id"].as_str().unwrap(), "/api/v1").await
    else {
        panic!("API resource accepted while off")
    };
    assert_eq!(
        query_param(&loc, "error").as_deref(),
        Some("invalid_target")
    );
}
