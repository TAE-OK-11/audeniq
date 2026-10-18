//! AUDENIQ MCP server and its OAuth authorization server, end to end against
//! real PostgreSQL through the API router (as the edge would call it).
//!
//! Simulated, not real: the AI client (requests are built here), audio
//! analysis (assets are registered directly, QC stays PENDING) and DSPs
//! (nothing is delivered; submission only starts AUDENIQ review). Platform
//! routing is bypassed (`test_only_bypass_dsp_gate`) like the other pipeline
//! suites, except in the test that checks the platform rule itself.
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    mcp::Settings,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

mod support;
use support::{MemStore, ORIGIN, SECRET, User, call, register_asset, user};

const REDIRECT: &str = "https://client.example.test/callback";
const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
/// Stands in for an audio file uploaded in Studio: these tests never run QC
/// on it (no ffmpeg or analyzer needed), only its registration and hash.
const AUDIO: &[u8] = b"RIFF\x24\x00\x00\x00WAVEfmt mcp-test-audio";

async fn app_with(pool: PgPool, enabled: bool, bypass_dsp_gate: bool) -> (Router, Arc<MemStore>) {
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
            test_only_bypass_dsp_gate: bypass_dsp_gate,
        },
        store.clone(),
    )
    .await
    .unwrap();
    s.mcp = Arc::new(Settings {
        enabled,
        cimd_hosts: vec!["clients.example.test".into()],
        dynamic_registration: true,
        payout_limit_krw: 300_000,
        public_daily_user: 150,
    });
    (router(s), store)
}
async fn app(pool: PgPool) -> (Router, Arc<MemStore>) {
    app_with(pool, true, true).await
}

/// A raw request as an AI client sends it: service secret (added by the
/// edge), no browser Origin unless given.
async fn raw(
    app: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: String,
) -> (StatusCode, axum::http::HeaderMap, String) {
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

fn form(pairs: &[(&str, &str)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(pairs)
        .finish()
}

async fn register_client(app: &Router) -> String {
    let (s, _, body) = raw(
        app,
        "POST",
        "/api/oauth/register",
        &[("content-type", "application/json")],
        json!({"client_name":"Test <b>Assistant</b>","redirect_uris":[REDIRECT],"token_endpoint_auth_method":"none",
               "grant_types":["authorization_code","refresh_token"],"response_types":["code"],"unknown_field":1}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert!(v.get("client_secret").is_none());
    v["client_id"].as_str().unwrap().to_string()
}

fn authorize_query(client_id: &str, scope: &str) -> String {
    format!(
        "/api/oauth/authorize?{}",
        form(&[
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", REDIRECT),
            ("code_challenge", CHALLENGE),
            ("code_challenge_method", "S256"),
            ("state", "st-123"),
            ("scope", scope),
            ("resource", &format!("{ORIGIN}/api/mcp")),
        ])
    )
}

/// The signed consent request embedded in the consent page.
async fn consent_page(app: &Router, u: &User, client_id: &str, scope: &str) -> String {
    let (s, h, body) = raw(
        app,
        "GET",
        &authorize_query(client_id, scope),
        &[("cookie", &u.cookie)],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert!(
        body.contains("Test &lt;b&gt;Assistant&lt;/b&gt;"),
        "client name is escaped"
    );
    assert!(!body.contains("<b>Assistant"));
    let start = body.find("name=\"request\" value=\"").unwrap() + "name=\"request\" value=\"".len();
    let end = body[start..].find('"').unwrap() + start;
    body[start..end].to_string()
}

async fn approve(app: &Router, u: &User, request: &str, scopes: &[&str]) -> (StatusCode, String) {
    let org = u.org.to_string();
    let mut pairs = vec![
        ("request", request),
        ("org", org.as_str()),
        ("decision", "approve"),
    ];
    for s in scopes {
        pairs.push((s, "on"));
    }
    let (s, h, _) = raw(
        app,
        "POST",
        "/api/oauth/authorize",
        &[
            ("cookie", &u.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&pairs),
    )
    .await;
    (
        s,
        h.get("location")
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default(),
    )
}

fn query_param(url: &str, key: &str) -> Option<String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

async fn exchange(
    app: &Router,
    client_id: &str,
    code: &str,
    verifier: &str,
) -> (StatusCode, Value) {
    let (s, _, body) = raw(
        app,
        "POST",
        "/api/oauth/token",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("client_id", client_id),
            ("redirect_uri", REDIRECT),
            ("resource", &format!("{ORIGIN}/api/mcp")),
        ]),
    )
    .await;
    (s, serde_json::from_str(&body).unwrap_or(Value::Null))
}

struct Conn {
    client_id: String,
    access: String,
    refresh: String,
}

/// Full authorization: registration, consent, code exchange.
async fn connect(app: &Router, u: &User, scopes: &[&str]) -> Conn {
    let client_id = register_client(app).await;
    let request = consent_page(
        app,
        u,
        &client_id,
        "releases:read releases:write releases:submit settlement:read payouts:request",
    )
    .await;
    let (s, location) = approve(app, u, &request, scopes).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert!(location.starts_with(REDIRECT));
    assert_eq!(query_param(&location, "state").as_deref(), Some("st-123"));
    assert_eq!(query_param(&location, "iss").as_deref(), Some(ORIGIN));
    let code = query_param(&location, "code").unwrap();
    let (s, t) = exchange(app, &client_id, &code, VERIFIER).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["token_type"], "Bearer");
    Conn {
        client_id,
        access: t["access_token"].as_str().unwrap().into(),
        refresh: t["refresh_token"].as_str().unwrap().into(),
    }
}

/// One MCP JSON-RPC call (legacy era: no per-request metadata).
async fn rpc(
    app: &Router,
    token: Option<&str>,
    method: &str,
    params: Value,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![
        ("content-type", "application/json"),
        ("accept", "application/json, text/event-stream"),
        ("mcp-protocol-version", "2025-06-18"),
    ];
    if let Some(a) = &auth {
        headers.push(("authorization", a.as_str()));
    }
    let (s, h, body) = raw(
        app,
        "POST",
        "/api/mcp",
        &headers,
        json!({"jsonrpc":"2.0","id":7,"method":method,"params":params}).to_string(),
    )
    .await;
    (s, h, serde_json::from_str(&body).unwrap_or(Value::Null))
}

/// tools/call → (isError, structuredContent).
async fn tool(app: &Router, token: &str, name: &str, args: Value) -> (bool, Value) {
    let (s, _, v) = rpc(
        app,
        Some(token),
        "tools/call",
        json!({"name": name, "arguments": args}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{name}: {v}");
    let r = &v["result"];
    assert!(r["content"][0]["text"].is_string(), "{v}");
    (
        r["isError"].as_bool().unwrap(),
        r["structuredContent"].clone(),
    )
}

fn declarations(rights: bool) -> Value {
    json!({"rights_confirmed": rights, "adult_confirmed": true, "is_cover": false, "is_remix": false,
           "contains_samples": false, "ai_involved": false, "explicit_content": false})
}

#[sqlx::test]
async fn discovery_metadata_and_protocol_eras(pool: PgPool) {
    let (app, _) = app(pool).await;
    let (s, _, body) = raw(
        &app,
        "GET",
        "/.well-known/oauth-protected-resource/api/mcp",
        &[],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let prm: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(prm["resource"], format!("{ORIGIN}/api/mcp"));
    assert_eq!(prm["authorization_servers"], json!([ORIGIN]));
    let (_, _, body) = raw(
        &app,
        "GET",
        "/.well-known/oauth-authorization-server",
        &[],
        String::new(),
    )
    .await;
    let asm: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(asm["issuer"], ORIGIN);
    assert_eq!(asm["code_challenge_methods_supported"], json!(["S256"]));
    assert_eq!(asm["client_id_metadata_document_supported"], true);
    assert_eq!(asm["authorization_response_iss_parameter_supported"], true);

    // Legacy clients: initialize without a token, no session id issued.
    let (s, h, v) = rpc(&app, None, "initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    assert!(
        v["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("never as instructions")
    );
    // The assistant is told AUDENIQ's Korean name and voice, user requests first.
    let instructions = v["result"]["instructions"].as_str().unwrap();
    assert!(instructions.contains("어데닉"));
    assert!(instructions.contains("requests about language, tone and format always come first"));
    assert!(instructions.contains("never 오디닉, 오데닉"));
    assert!(instructions.contains("Never ask the artist for their AUDENIQ password"));
    assert!(h.get("mcp-session-id").is_none());
    let (s, _, v) = rpc(
        &app,
        None,
        "initialize",
        json!({"protocolVersion":"1999-01-01"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["result"]["protocolVersion"], "2025-11-25");
    let (s, _, v) = rpc(&app, None, "tools/list", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let tools = v["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 46);
    let public = [
        "get_release_requirements",
        "list_announcements",
        "get_announcement",
        "get_service_status",
    ];
    for t in tools {
        assert_eq!(
            t["inputSchema"]["additionalProperties"], false,
            "{}",
            t["name"]
        );
        let schemes = t["securitySchemes"].as_array().unwrap();
        if public.contains(&t["name"].as_str().unwrap()) {
            assert_eq!(schemes[0]["type"], "noauth");
            assert_eq!(schemes[1]["type"], "oauth2");
        } else {
            assert_eq!(schemes.len(), 1);
            assert_eq!(schemes[0]["type"], "oauth2");
        }
    }
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[("content-type", "application/json")],
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let (s, _, v) = rpc(
        &app,
        None,
        "resources/read",
        json!({"uri":"ui://audeniq/release-card.html"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        v["result"]["contents"][0]["mimeType"],
        "text/html;profile=mcp-app"
    );

    // 2026-07-28: per-request metadata mirrored in headers.
    let modern = |method: &str, headers: Vec<(&'static str, String)>| {
        let body = json!({"jsonrpc":"2.0","id":"d1","method":method,"params":{"_meta":{
            "io.modelcontextprotocol/protocolVersion":"2026-07-28",
            "io.modelcontextprotocol/clientInfo":{"name":"t","version":"1"},
            "io.modelcontextprotocol/clientCapabilities":{}}}})
        .to_string();
        let app = app.clone();
        async move {
            let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let (s, _, b) = raw(&app, "POST", "/api/mcp", &h, body).await;
            (s, serde_json::from_str::<Value>(&b).unwrap_or(Value::Null))
        }
    };
    let ok_headers = |m: &str| {
        vec![
            ("content-type", "application/json".to_string()),
            ("mcp-protocol-version", "2026-07-28".to_string()),
            ("mcp-method", m.to_string()),
        ]
    };
    let (s, v) = modern("server/discover", ok_headers("server/discover")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["result"]["supportedVersions"][0], "2026-07-28");
    let (s, v) = modern("tools/list", ok_headers("tools/list")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["result"]["resultType"], "complete");
    let (s, v) = modern("tools/list", ok_headers("server/discover")).await;
    assert_eq!(
        (s, v["error"]["code"].as_i64()),
        (StatusCode::BAD_REQUEST, Some(-32020))
    );
    let mut wrong_version = ok_headers("tools/list");
    wrong_version[1].1 = "2025-11-25".into();
    let (s, v) = modern("tools/list", wrong_version).await;
    assert_eq!(
        (s, v["error"]["code"].as_i64()),
        (StatusCode::BAD_REQUEST, Some(-32020))
    );
    let (s, v) = modern("no/such", ok_headers("no/such")).await;
    assert_eq!(
        (s, v["error"]["code"].as_i64()),
        (StatusCode::NOT_FOUND, Some(-32601))
    );
    let (s, _, b) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[("mcp-protocol-version", "1900-01-01")],
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(
        serde_json::from_str::<Value>(&b).unwrap()["error"]["code"],
        -32022
    );

    // Transport rules: GET → 405, foreign browser Origin → 403, garbage → 400.
    let (s, _, _) = raw(&app, "GET", "/api/mcp", &[], String::new()).await;
    assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[("origin", "https://evil.example")],
        json!({"jsonrpc":"2.0","id":1,"method":"ping"}).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = raw(&app, "POST", "/api/mcp", &[], "{not json".into()).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[],
        json!([{"jsonrpc":"2.0","id":1,"method":"ping"}]).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Still behind the edge's service secret.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/mcp")
                .body(Body::from(
                    json!({"jsonrpc":"2.0","id":1,"method":"ping"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn disabled_by_default_everything_is_not_found(pool: PgPool) {
    let (app, _) = app_with(pool, false, true).await;
    for (m, p) in [
        ("POST", "/api/mcp"),
        ("GET", "/.well-known/oauth-authorization-server"),
        ("POST", "/api/oauth/register"),
        ("POST", "/api/oauth/token"),
    ] {
        let (s, _, _) = raw(
            &app,
            m,
            p,
            &[("content-type", "application/json")],
            "{}".into(),
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {p}");
    }
}

#[sqlx::test]
async fn tools_require_a_valid_token_and_logout_everywhere_revokes_it(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write"]).await;
    let (s, h, v) = rpc(
        &app,
        None,
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let challenge = h["www-authenticate"].to_str().unwrap();
    assert!(
        challenge.contains(&format!(
            "resource_metadata=\"{ORIGIN}/.well-known/oauth-protected-resource/api/mcp\""
        )),
        "{challenge}"
    );
    assert!(v["error"].is_object());
    let (s, _, _) = rpc(
        &app,
        Some(&"0".repeat(64)),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // The browser session cookie is not a bearer token.
    let (s, _, _) = rpc(
        &app,
        Some(u.cookie.split('=').nth(1).unwrap()),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (err, v) = tool(&app, &c.access, "get_account_context", json!({})).await;
    assert!(!err, "{v}");
    assert_eq!(v["workspace"]["id"], u.org.to_string());
    assert_eq!(v["workspace"]["role"], "OWNER");
    assert_eq!(
        v["connection"]["scopes"],
        json!(["releases:read", "releases:write"])
    );
    assert_eq!(v["can_submit"], false);
    assert_eq!(v["brand"]["name_ko"], "어데닉");

    // Expired access token → 401 (client refreshes).
    sqlx::query(
        "UPDATE identity.mcp_tokens SET expires_at=now()-interval '1 second' WHERE kind='ACCESS'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let (s, _, _) = rpc(
        &app,
        Some(&c.access),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/token",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &c.refresh),
            ("client_id", &c.client_id),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let fresh: Value = serde_json::from_str(&body).unwrap();
    let access = fresh["access_token"].as_str().unwrap();
    assert_ne!(
        fresh["refresh_token"],
        c.refresh.as_str(),
        "refresh tokens rotate"
    );
    assert!(!tool(&app, access, "get_account_context", json!({})).await.0);

    // "Log out everywhere" also disconnects AI assistants.
    let (s, _) = call(&app, "POST", "/api/auth/logout-all", json!({}), Some(&u)).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = rpc(
        &app,
        Some(access),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/token",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", fresh["refresh_token"].as_str().unwrap()),
            ("client_id", &c.client_id),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid_grant"));
    let reason: String =
        sqlx::query_scalar("SELECT revoke_reason FROM identity.mcp_grants WHERE user_id=$1")
            .bind(u.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(reason, "LOGOUT_ALL");
}

#[sqlx::test]
async fn authorization_code_and_refresh_token_attacks_are_refused(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let client_id = register_client(&app).await;

    // Unregistered redirect URI: an error page, never a redirect.
    let bad = authorize_query(&client_id, "releases:read")
        .replace("client.example.test", "evil.example.test");
    let (s, h, _) = raw(&app, "GET", &bad, &[("cookie", &u.cookie)], String::new()).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(h.get("location").is_none());
    // Missing PKCE / unknown resource / unknown scope: error redirect with iss.
    for (from, to, err) in [
        (
            "code_challenge_method=S256",
            "code_challenge_method=plain",
            "invalid_request",
        ),
        (
            "resource=http",
            "resource=https%3A%2F%2Fother.example%2Fmcp&x=http",
            "invalid_target",
        ),
        (
            "scope=releases",
            "scope=payouts%3Awrite+releases",
            "invalid_scope",
        ),
    ] {
        let q = authorize_query(&client_id, "releases:read").replacen(from, to, 1);
        let (s, h, _) = raw(&app, "GET", &q, &[("cookie", &u.cookie)], String::new()).await;
        assert_eq!(s, StatusCode::SEE_OTHER, "{q}");
        let loc = h["location"].to_str().unwrap();
        assert_eq!(query_param(loc, "error").as_deref(), Some(err), "{loc}");
        assert_eq!(query_param(loc, "iss").as_deref(), Some(ORIGIN));
    }
    // Not signed in: the sign-in page, not a consent screen.
    let (s, _, body) = raw(
        &app,
        "GET",
        &authorize_query(&client_id, "releases:read"),
        &[],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("/login") && !body.contains("name=\"request\""));

    // The consent request is bound to the session that saw it.
    let request = consent_page(&app, &u, &client_id, "releases:read").await;
    let other = user(&app).await;
    let (s, _) = approve(&app, &other, &request, &[]).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Cross-site form posts are refused (no Studio Origin).
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/oauth/authorize",
        &[
            ("cookie", &u.cookie),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&[
            ("request", &request),
            ("org", &u.org.to_string()),
            ("decision", "approve"),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // A tampered request is refused.
    let (s, _) = approve(&app, &u, &format!("{request}x"), &[]).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Another user's workspace cannot be chosen.
    let org = other.org.to_string();
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/oauth/authorize",
        &[
            ("cookie", &u.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&[
            ("request", &request),
            ("org", &org),
            ("decision", "approve"),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Deny → access_denied.
    let (s, h, _) = raw(
        &app,
        "POST",
        "/api/oauth/authorize",
        &[
            ("cookie", &u.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&[("request", &request), ("decision", "deny")]),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(
        query_param(h["location"].to_str().unwrap(), "error").as_deref(),
        Some("access_denied")
    );

    // Wrong PKCE verifier.
    let (_, location) = approve(&app, &u, &request, &[]).await;
    let code = query_param(&location, "code").unwrap();
    let (s, v) = exchange(&app, &client_id, &code, &"a".repeat(43)).await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    // Another client cannot redeem the code.
    let other_client = register_client(&app).await;
    let (s, v) = exchange(&app, &other_client, &code, VERIFIER).await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    // Correct exchange, then a replay revokes the grant and its tokens.
    let (s, t) = exchange(&app, &client_id, &code, VERIFIER).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let access = t["access_token"].as_str().unwrap().to_string();
    assert!(
        !tool(&app, &access, "get_account_context", json!({}))
            .await
            .0
    );
    let (s, _) = exchange(&app, &client_id, &code, VERIFIER).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = rpc(
        &app,
        Some(&access),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Refresh token reuse revokes the grant.
    let c = connect(&app, &u, &[]).await;
    let refresh = |rt: String| {
        let app = app.clone();
        let cid = c.client_id.clone();
        async move {
            let (s, _, b) = raw(
                &app,
                "POST",
                "/api/oauth/token",
                &[("content-type", "application/x-www-form-urlencoded")],
                form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &rt),
                    ("client_id", &cid),
                ]),
            )
            .await;
            (s, serde_json::from_str::<Value>(&b).unwrap())
        }
    };
    let (s, next) = refresh(c.refresh.clone()).await;
    assert_eq!(s, StatusCode::OK);
    // The same token again within seconds is the client racing itself
    // (parallel calls, a retry): refused, but the first answer stays valid.
    let (s, v) = refresh(c.refresh.clone()).await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let (err, _) = tool(
        &app,
        next["access_token"].as_str().unwrap(),
        "get_account_context",
        json!({}),
    )
    .await;
    assert!(!err, "a racing refresh does not disconnect the user");
    // Later reuse is theft: the whole grant ends.
    sqlx::query("UPDATE identity.mcp_tokens SET used_at=now()-interval '1 minute' WHERE used_at IS NOT NULL")
        .execute(&pool)
        .await
        .unwrap();
    let (s, v) = refresh(c.refresh.clone()).await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let (s, _) = refresh(next["refresh_token"].as_str().unwrap().into()).await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "the rotated token died with the grant"
    );
    let reason: String = sqlx::query_scalar("SELECT revoke_reason FROM identity.mcp_grants WHERE user_id=$1 AND revoke_reason='REFRESH_REUSED'")
        .bind(u.user).fetch_one(&pool).await.unwrap();
    assert_eq!(reason, "REFRESH_REUSED");

    // RFC 7009 revocation by the client.
    let c = connect(&app, &u, &[]).await;
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/oauth/revoke",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[("token", &c.access), ("client_id", &c.client_id)]),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = rpc(
        &app,
        Some(&c.access),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // The user sees and revokes connections from the Studio session.
    let c = connect(&app, &u, &[]).await;
    let (s, v) = call(
        &app,
        "GET",
        "/api/auth/ai-connections",
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let items = v["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        1,
        "older grants of the same client were replaced or revoked: {v}"
    );
    let id = items[0]["id"].as_str().unwrap();
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-connections/{id}/revoke"),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "another user cannot revoke it");
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-connections/{id}/revoke"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = rpc(
        &app,
        Some(&c.access),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn client_id_metadata_documents_are_cached_and_allowlisted(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    // A fresh cached document (no network in tests).
    let cid = "https://clients.example.test/oauth/client.json";
    sqlx::query("INSERT INTO identity.mcp_clients(id,client_id,kind,client_name,redirect_uris,token_endpoint_auth_method,fetched_at) VALUES($1,$2,'CIMD','Cached Client',$3,'none',now())")
        .bind(Uuid::new_v4()).bind(cid).bind(vec![REDIRECT.to_string()]).execute(&pool).await.unwrap();
    let request = consent_page_named(&app, &u, cid).await;
    assert!(request.contains("Cached Client") && request.contains(cid));
    // A URL client on a host outside the allowlist is never fetched.
    let (s, _, body) = raw(
        &app,
        "GET",
        &authorize_query("https://169.254.169.254/latest/meta-data", "releases:read"),
        &[("cookie", &u.cookie)],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(!body.contains("name=\"request\""));
}
async fn consent_page_named(app: &Router, u: &User, client_id: &str) -> String {
    let (s, _, body) = raw(
        app,
        "GET",
        &authorize_query(client_id, "releases:read"),
        &[("cookie", &u.cookie)],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    body
}

#[sqlx::test]
async fn scopes_and_tenants_bound_every_tool(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let a = user(&app).await;
    let b = user(&app).await;
    let read_only = connect(&app, &a, &[]).await;
    let writer = connect(&app, &a, &["scope_write"]).await;
    let other = connect(&app, &b, &["scope_write", "scope_submit"]).await;

    let (s, h, v) = rpc(&app, Some(&read_only.access), "tools/call",
        json!({"name":"create_release_draft","arguments":{"title":"X","release_type":"SINGLE","artist_name":"A"}})).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert!(
        h["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("insufficient_scope")
    );
    assert!(
        h["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("scope=\"releases:write\"")
    );

    let (err, rel) = tool(
        &app,
        &writer.access,
        "create_release_draft",
        json!({"title":"Night Drive","release_type":"SINGLE","artist_name":"Han River"}),
    )
    .await;
    assert!(!err, "{rel}");
    let release = rel["release_id"].as_str().unwrap().to_string();

    // Another account's token cannot see or touch it, and cannot name an org.
    let (err, v) = tool(
        &app,
        &other.access,
        "get_release",
        json!({"release_id": release}),
    )
    .await;
    assert!(err);
    assert_eq!(v["error"]["code"], "FORBIDDEN");
    let (err, v) = tool(
        &app,
        &other.access,
        "update_release_draft",
        json!({"release_id": release, "title": "Hijack"}),
    )
    .await;
    assert!(err);
    assert_eq!(v["error"]["code"], "FORBIDDEN");
    let (err, v) = tool(
        &app,
        &other.access,
        "get_release",
        json!({"release_id": release, "org_id": a.org}),
    )
    .await;
    assert!(err);
    assert_eq!(v["error"]["code"], "INVALID_ARGUMENTS");
    let (err, v) = tool(&app, &other.access, "list_releases", json!({})).await;
    assert!(!err);
    assert_eq!(v["releases"], json!([]));
    let (err, v) = tool(
        &app,
        &other.access,
        "preview_submission",
        json!({"release_id": release, "declarations": declarations(true)}),
    )
    .await;
    assert!(err);
    assert_eq!(v["error"]["code"], "FORBIDDEN");

    // A viewer membership limits the token even with the write scope.
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role) VALUES($1,$2,'VIEWER')")
        .bind(a.org)
        .bind(b.user)
        .execute(&pool)
        .await
        .unwrap();
    let (err, _) = tool(
        &app,
        &other.access,
        "get_release",
        json!({"release_id": release}),
    )
    .await;
    assert!(
        err,
        "the token stays bound to the workspace chosen at consent"
    );

    // Malformed arguments are refused before any service call.
    let (err, v) = tool(
        &app,
        &writer.access,
        "update_release_draft",
        json!({"release_id": release, "release_date": "soon"}),
    )
    .await;
    assert!(err);
    assert_eq!(v["error"]["code"], "INVALID_ARGUMENTS");
    assert!(v["error"]["details"]["problems"].as_array().unwrap().len() == 1);
    let (s, _, v) = rpc(
        &app,
        Some(&writer.access),
        "tools/call",
        json!({"name":"drop_tables","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["error"]["code"], -32602);

    // Write tools are audited with the client; nothing logs the token.
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action='mcp.create_release_draft' AND actor_user_id=$1 AND reason_code='MCP_OK'")
        .bind(a.user).fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 1);
    let rejected: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action='mcp.update_release_draft' AND actor_user_id=$1 AND reason_code='MCP_REJECTED'")
        .bind(b.user).fetch_one(&pool).await.unwrap();
    assert_eq!(rejected, 1);
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identity.mcp_tokens WHERE token_hash=$1")
            .bind(writer.access.as_bytes())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 0, "tokens are stored hashed only");
}

#[sqlx::test]
async fn conversational_release_preparation_to_confirmed_submission(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write", "scope_submit"]).await;

    let (err, acct) = tool(&app, &c.access, "get_account_context", json!({})).await;
    assert!(!err && acct["can_submit"] == true, "{acct}");
    let (err, req) = tool(&app, &c.access, "get_release_requirements", json!({})).await;
    assert!(!err);
    assert_eq!(
        req["release_types"][1],
        json!({"release_type":"EP","min_tracks":2,"max_tracks":6})
    );
    let (err, p) = tool(&app, &c.access, "list_distribution_platforms", json!({})).await;
    assert!(!err);
    assert!(
        p["platforms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["requirements"]["artwork_min_px"].is_number())
    );

    // 1. Draft with the basics.
    let (err, rel) = tool(&app, &c.access, "create_release_draft",
        json!({"title":"Night Drive","release_type":"SINGLE","artist_name":"Han River","genre":"pop"})).await;
    assert!(!err, "{rel}");
    let release = rel["release_id"].as_str().unwrap().to_string();
    assert_eq!(rel["artist_name"], "Han River");
    assert_eq!(rel["language"], "ko");

    // 2. What is missing? Field-level findings, no confirmation possible.
    let (err, v) = tool(
        &app,
        &c.access,
        "validate_release",
        json!({"release_id": release}),
    )
    .await;
    assert!(!err);
    assert_eq!(v["ready_to_submit"], false);
    let codes: Vec<&str> = v["blocking"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["code"].as_str())
        .collect();
    for code in [
        "TRACK_REQUIRED",
        "FIELD_RELEASE_DATE_MISSING",
        "PLINE_MISSING",
        "CLINE_MISSING",
    ] {
        assert!(codes.contains(&code), "{code} in {codes:?}");
    }
    assert!(
        v["blocking"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["message"].is_string()),
        "every finding is explained: {v}"
    );

    // 3. Fill in metadata, a track and its credits.
    let (err, v) = tool(&app, &c.access, "update_release_draft", json!({"release_id": release,
        "release_date":"2027-03-01","p_line":"2026 Han River","c_line":"2026 Han River","platforms":["spotify"],"territories":["WORLD","-JP"]})).await;
    assert!(!err, "{v}");
    let (err, v) = tool(
        &app,
        &c.access,
        "upsert_track",
        json!({"release_id": release, "title":"Night Drive", "track_number":1}),
    )
    .await;
    assert!(!err, "{v}");
    let track = v["track_id"].as_str().unwrap().to_string();
    let (err, v) = tool(
        &app,
        &c.access,
        "set_track_credits",
        json!({"release_id": release, "track_id": track,
        "composers":["Kim Minji"], "lyricists":["Kim Minji","Lee Jun"]}),
    )
    .await;
    assert!(!err, "{v}");
    assert_eq!(
        v["tracks"][0]["credits"]["lyricists"],
        json!(["Kim Minji", "Lee Jun"])
    );
    // The Studio editor's copy matches, so reopening it in Studio keeps the credits.
    let draft: Value = sqlx::query_scalar("SELECT draft FROM catalog.releases WHERE id=$1")
        .bind(Uuid::parse_str(&release).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(draft["draftTracks"][0]["serverId"], track.as_str());
    assert_eq!(draft["draftTracks"][0]["lyricists"], "Kim Minji, Lee Jun");
    assert_eq!(draft["phonogram"], "2026 Han River");

    // 4. Audio is still missing: reported with the Studio upload link.
    let (_, v) = tool(
        &app,
        &c.access,
        "validate_release",
        json!({"release_id": release}),
    )
    .await;
    let codes: Vec<&str> = v["blocking"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["code"].as_str())
        .collect();
    assert!(codes.contains(&"AUDIO_REQUIRED"), "{codes:?}");
    assert!(
        v["missing_files"]["upload_url"]
            .as_str()
            .unwrap()
            .contains("/upload?edit=")
    );
    let (err, v) = tool(
        &app,
        &c.access,
        "preview_submission",
        json!({"release_id": release, "declarations": declarations(true)}),
    )
    .await;
    assert!(!err);
    assert!(
        v.get("confirmation_token").is_none(),
        "no confirmation while blocked"
    );

    // The artist uploads audio in Studio (simulated: a registered asset).
    let asset = register_asset(&pool, &store, &u, "night.wav", AUDIO).await;
    let (err, v) = tool(
        &app,
        &c.access,
        "upsert_track",
        json!({"release_id": release, "track_id": track,
        "title":"Night Drive", "track_number":1, "asset_id": asset}),
    )
    .await;
    assert!(!err, "{v}");
    assert_eq!(v["tracks"][0]["audio_attached"], true);
    assert_eq!(
        v["tracks"][0]["credits"]["composers"],
        json!(["Kim Minji"]),
        "editing a track keeps its credits"
    );

    // 5. Ready. Declarations are required; the preview is bound to them.
    let (_, v) = tool(
        &app,
        &c.access,
        "validate_release",
        json!({"release_id": release}),
    )
    .await;
    assert_eq!(v["ready_to_submit"], true, "{v}");
    assert!(
        v["will_be_reviewed"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "AUDIO_QC_PENDING")
    );
    let (_, v) = tool(
        &app,
        &c.access,
        "preview_submission",
        json!({"release_id": release, "declarations": declarations(false)}),
    )
    .await;
    assert_eq!(v["ready_to_submit"], false);
    assert!(v.get("confirmation_token").is_none());
    let (err, preview) = tool(
        &app,
        &c.access,
        "preview_submission",
        json!({"release_id": release, "declarations": declarations(true)}),
    )
    .await;
    assert!(!err);
    assert_eq!(preview["view"], "preview");
    let token = preview["confirmation_token"].as_str().unwrap().to_string();

    // Not confirmed by the artist / different declarations / tampered token.
    let submit = |token: String, decl: Value, confirmed: bool| {
        let app = app.clone();
        let access = c.access.clone();
        let release = release.clone();
        async move {
            tool(
                &app,
                &access,
                "submit_release",
                json!({"release_id": release, "confirmation_token": token,
                "declarations": decl, "artist_confirmed": confirmed}),
            )
            .await
        }
    };
    let (err, v) = submit(token.clone(), declarations(true), false).await;
    assert!(err);
    assert_eq!(v["error"]["code"], "INVALID_ARGUMENTS");
    let mut other_decl = declarations(true);
    other_decl["ai_involved"] = json!(true);
    let (err, v) = submit(token.clone(), other_decl, true).await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("CONFIRMATION_INVALID"))
    );
    let (err, v) = submit(format!("{token}A"), declarations(true), true).await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("CONFIRMATION_INVALID"))
    );
    assert_eq!(
        support::release_status(&pool, Uuid::parse_str(&release).unwrap()).await,
        "DRAFT"
    );

    // A change after the preview invalidates it.
    let (err, _) = tool(
        &app,
        &c.access,
        "update_release_draft",
        json!({"release_id": release, "genre":"rock"}),
    )
    .await;
    assert!(!err);
    let (err, v) = submit(token.clone(), declarations(true), true).await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("RELEASE_CHANGED"))
    );

    // 6. Confirmed submission, then a retry: one application.
    let (_, preview) = tool(
        &app,
        &c.access,
        "preview_submission",
        json!({"release_id": release, "declarations": declarations(true)}),
    )
    .await;
    let token = preview["confirmation_token"].as_str().unwrap().to_string();
    let (err, first) = submit(token.clone(), declarations(true), true).await;
    assert!(!err, "{first}");
    assert_eq!(first["status"], "SUBMITTED");
    assert_eq!(first["duplicate_request"], false);
    let (err, again) = submit(token.clone(), declarations(true), true).await;
    assert!(!err, "{again}");
    assert_eq!(again["application_id"], first["application_id"]);
    assert_eq!(again["duplicate_request"], true);
    let revisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog.application_revisions WHERE release_id=$1",
    )
    .bind(Uuid::parse_str(&release).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revisions, 1);
    let body: Value =
        sqlx::query_scalar("SELECT body FROM catalog.application_revisions WHERE release_id=$1")
            .bind(Uuid::parse_str(&release).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(body["declarations"]["rights_confirmed"], true);
    assert_eq!(
        body["release"]["draft"]["territories"],
        json!(["WORLD", "-JP"])
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action='submission.created' AND resource_id=$1")
        .bind(Uuid::parse_str(&release).unwrap()).fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 1);

    // 7. Under review: no more edits; status explains the follow-up.
    let (err, v) = tool(
        &app,
        &c.access,
        "update_release_draft",
        json!({"release_id": release, "genre":"jazz"}),
    )
    .await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("RELEASE_NOT_EDITABLE"))
    );
    let (err, st) = tool(
        &app,
        &c.access,
        "get_submission_status",
        json!({"release_id": release}),
    )
    .await;
    assert!(!err);
    assert_eq!(st["status"], "SUBMITTED");
    assert!(
        st["follow_up"][0]
            .as_str()
            .unwrap()
            .contains("Sign the release application")
    );
    let (err, v) = tool(&app, &c.access, "list_releases", json!({})).await;
    assert!(!err);
    assert_eq!(v["releases"][0]["status"], "SUBMITTED");
}

#[sqlx::test]
async fn submit_scope_is_opt_in_and_platform_rules_are_enforced(pool: PgPool) {
    let (app, store) = app_with(pool.clone(), true, false).await;
    let u = user(&app).await;
    // The client asked for submit, the user did not tick it.
    let c = connect(&app, &u, &["scope_write"]).await;
    let (_, rel) = tool(
        &app,
        &c.access,
        "create_release_draft",
        json!({"title":"T","release_type":"SINGLE","artist_name":"A",
        "release_date":"2027-03-01","p_line":"2026 A","c_line":"2026 A"}),
    )
    .await;
    let release = rel["release_id"].as_str().unwrap().to_string();
    let asset = register_asset(&pool, &store, &u, "t.wav", AUDIO).await;
    let (_, v) = tool(
        &app,
        &c.access,
        "upsert_track",
        json!({"release_id": release, "title":"T", "track_number":1, "asset_id": asset}),
    )
    .await;
    let track = v["track_id"].as_str().unwrap().to_string();
    tool(
        &app,
        &c.access,
        "set_track_credits",
        json!({"release_id": release, "track_id": track, "composers":["A"]}),
    )
    .await;
    // No platform chosen: the same rule submit applies.
    let (_, v) = tool(
        &app,
        &c.access,
        "validate_release",
        json!({"release_id": release}),
    )
    .await;
    assert!(
        v["blocking"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "DSP_SELECTION_REQUIRED"),
        "{v}"
    );
    // A platform without a delivery route in this deployment is unavailable.
    tool(
        &app,
        &c.access,
        "update_release_draft",
        json!({"release_id": release, "platforms":["spotify"]}),
    )
    .await;
    let (_, v) = tool(
        &app,
        &c.access,
        "validate_release",
        json!({"release_id": release}),
    )
    .await;
    assert!(
        v["blocking"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["code"] == "DSP_UNAVAILABLE"),
        "{v}"
    );
    let (s, _, v) = rpc(&app, Some(&c.access), "tools/call", json!({"name":"submit_release","arguments":{
        "release_id": release, "confirmation_token": "x".repeat(40), "declarations": declarations(true), "artist_confirmed": true}})).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"]["data"]["required_scope"], "releases:submit");
}

async fn fund(pool: &PgPool, u: &User, amount: &str) {
    use audeniq_core::finance::{self, EntrySide, LedgerEntryInput, PostTransaction};
    let entry = |account, side| LedgerEntryInput {
        account,
        side,
        amount: amount.parse().unwrap(),
        currency: None,
        party_id: Some(u.party),
        isrc: Some("KRA402600001"),
        split_snapshot_id: None,
    };
    finance::post_transaction(
        pool,
        u.org,
        PostTransaction {
            transaction_code: "ROYALTY_ACCRUAL",
            currency: "KRW",
            entries: vec![
                entry("ROYALTY_RECEIVABLE", EntrySide::Debit),
                entry("ROYALTY_PAYABLE", EntrySide::Credit),
            ],
            match_status: Some("AUTO"),
            fx_rate: None,
            fee_policy_version: None,
            contract_version: None,
            tax_rule_version: None,
            source_ref: json!({"isrc": "KRA402600001"}),
            description: "accrual",
            created_by: None,
        },
    )
    .await
    .unwrap();
}

async fn register_payout_account(app: &Router, u: &User, number: &str) {
    let (s, v) = call(
        app,
        "PUT",
        &format!("/api/orgs/{}/payout-account", u.org),
        json!({"payee_type":"INDIVIDUAL","holder_name":"서린","bank_name":"국민은행","account_number":number,
               "consents":["settlement_terms","privacy","financial_info","payee_accuracy"]}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

#[sqlx::test]
async fn settlement_view_and_small_payouts_large_ones_go_to_studio(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_settlement", "scope_payout"]).await;
    let preview = |amount: i64| {
        let app = app.clone();
        let access = c.access.clone();
        async move { tool(&app, &access, "preview_payout", json!({"amount": amount})).await }
    };
    let request = |amount: i64, token: String| {
        let app = app.clone();
        let access = c.access.clone();
        async move {
            tool(
                &app,
                &access,
                "request_payout",
                json!({"amount": amount, "confirmation_token": token, "artist_confirmed": true}),
            )
            .await
        }
    };

    let (err, v) = tool(&app, &c.access, "get_settlement_summary", json!({})).await;
    assert!(!err, "{v}");
    assert_eq!(v["available"], "0");
    assert_eq!(v["account"]["registered"], false);
    assert_eq!(v["assistant_payouts"]["limit_krw_per_24h"], 300_000);
    // No account yet: no confirmation.
    let (_, v) = preview(10_000).await;
    assert_eq!(v["allowed"], false);
    assert_eq!(v["blocked_by"]["code"], "PAYOUT_ACCOUNT_REQUIRED");

    register_payout_account(&app, &u, "123456-78-901234").await;
    fund(&pool, &u, "1000000").await;
    let (_, v) = tool(&app, &c.access, "get_settlement_summary", json!({})).await;
    assert_eq!(v["available"], "1000000");
    assert_eq!(v["account"]["account_last4"], "1234");
    assert!(
        v["account"].get("holder_name").is_none(),
        "the holder name stays in Studio"
    );
    let (err, v) = tool(
        &app,
        &c.access,
        "list_settlement_statements",
        json!({"limit": 5}),
    )
    .await;
    assert!(!err && v["items"].as_array().unwrap().len() == 1, "{v}");
    let (err, _) = tool(
        &app,
        &c.access,
        "get_royalty_reports",
        json!({"from": "2026-01", "to": "2026-10"}),
    )
    .await;
    assert!(!err);

    // Above the assistant limit: Studio only.
    let (_, v) = preview(400_000).await;
    assert_eq!(v["allowed"], false);
    assert_eq!(v["blocked_by"]["code"], "PAYOUT_USE_STUDIO");
    assert!(
        v["blocked_by"]["message"]
            .as_str()
            .unwrap()
            .contains("/settlement")
    );
    assert!(v.get("confirmation_token").is_none());
    // A small fee applies to small amounts (settlement terms).
    let (_, v) = preview(20_000).await;
    assert_eq!(
        (v["fee"].as_str(), v["net"].as_str()),
        (Some("500"), Some("19500"))
    );

    // Two previews issued before either is used: the cap is rechecked under
    // the payout lock, so together they cannot pass the 24-hour limit.
    let (_, p1) = preview(200_000).await;
    let (_, p2) = preview(200_000).await;
    let t1 = p1["confirmation_token"].as_str().unwrap().to_string();
    let t2 = p2["confirmation_token"].as_str().unwrap().to_string();
    let (err, v) = request(100_000, t1.clone()).await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("CONFIRMATION_INVALID"))
    );
    let (err, first) = request(200_000, t1.clone()).await;
    assert!(!err, "{first}");
    assert_eq!(first["status"], "REQUESTED");
    assert_eq!(first["net"], "200000");
    let (err, again) = request(200_000, t1).await;
    assert!(!err);
    assert_eq!(
        again["payout_id"], first["payout_id"],
        "a retry is the same request"
    );
    let (err, v) = request(200_000, t2).await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("PAYOUT_USE_STUDIO"))
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM portal.payout_requests WHERE org_id=$1")
        .bind(u.org)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    // The account changed after the preview: refused.
    let (_, p3) = preview(50_000).await;
    register_payout_account(&app, &u, "999999-11-222222").await;
    let (err, v) = request(50_000, p3["confirmation_token"].as_str().unwrap().into()).await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("PAYOUT_ACCOUNT_CHANGED"))
    );

    // Listing hides the holder name; cancelling frees the assistant limit again.
    let (_, v) = tool(&app, &c.access, "list_payouts", json!({})).await;
    let item = &v["items"][0];
    assert!(item["account"].get("holder_name").is_none(), "{item}");
    assert_eq!(item["cancellable"], true);
    let (err, v) = tool(
        &app,
        &c.access,
        "cancel_payout",
        json!({"payout_id": first["payout_id"]}),
    )
    .await;
    assert!(!err && v["status"] == "CANCELLED", "{v}");
    let (_, v) = preview(300_000).await;
    assert_eq!(v["allowed"], true, "{v}");
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action='mcp.request_payout' AND actor_user_id=$1")
        .bind(u.user).fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 5, "every payout attempt is audited");
}

#[sqlx::test]
async fn settlement_and_payouts_need_their_own_opt_in_scopes_and_the_owner(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write"]).await;
    for (name, args, scope) in [
        ("get_settlement_summary", json!({}), "settlement:read"),
        (
            "preview_payout",
            json!({"amount": 10000}),
            "payouts:request",
        ),
    ] {
        let (s, _, v) = rpc(
            &app,
            Some(&c.access),
            "tools/call",
            json!({"name": name, "arguments": args}),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{name}: {v}");
        assert_eq!(v["error"]["data"]["required_scope"], scope);
    }
    // An editor (not the owner) cannot get a payout confirmation.
    let editor = user(&app).await;
    sqlx::query("UPDATE identity.memberships SET status='REVOKED' WHERE user_id=$1")
        .bind(editor.user)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role) VALUES($1,$2,'EDITOR')")
        .bind(u.org)
        .bind(editor.user)
        .execute(&pool)
        .await
        .unwrap();
    let client_id = register_client(&app).await;
    let request = consent_page(
        &app,
        &editor,
        &client_id,
        "releases:read settlement:read payouts:request",
    )
    .await;
    let org = u.org.to_string();
    let (_, h, _) = raw(
        &app,
        "POST",
        "/api/oauth/authorize",
        &[
            ("cookie", &editor.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&[
            ("request", &request),
            ("org", &org),
            ("decision", "approve"),
            ("scope_settlement", "on"),
            ("scope_payout", "on"),
        ]),
    )
    .await;
    let code = query_param(h["location"].to_str().unwrap(), "code").unwrap();
    let (_, t) = exchange(&app, &client_id, &code, VERIFIER).await;
    let access = t["access_token"].as_str().unwrap();
    let (_, v) = tool(&app, access, "preview_payout", json!({"amount": 10000})).await;
    assert_eq!(v["blocked_by"]["code"], "PAYOUT_OWNER_ONLY", "{v}");
}

#[sqlx::test]
async fn corrections_are_only_for_delivered_releases(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write"]).await;
    let (_, rel) = tool(
        &app,
        &c.access,
        "create_release_draft",
        json!({"title":"T","release_type":"SINGLE","artist_name":"A"}),
    )
    .await;
    let release = rel["release_id"].as_str().unwrap().to_string();
    let (err, v) = tool(
        &app,
        &c.access,
        "get_release_corrections",
        json!({"release_id": release}),
    )
    .await;
    assert!(!err, "{v}");
    assert_eq!(v["can_request"], false);
    assert_eq!(v["review"], "No correction requested yet.");
    let (err, v) = tool(
        &app,
        &c.access,
        "request_release_correction",
        json!({"release_id": release, "reason": "typo", "genre": "rock"}),
    )
    .await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("CORRECTION_NOTHING_SENT"))
    );
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("edit the draft")
    );
    let (err, v) = tool(
        &app,
        &c.access,
        "cancel_release_correction",
        json!({"release_id": release}),
    )
    .await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("NOT_FOUND"))
    );
    // Another account cannot read it.
    let other = user(&app).await;
    let oc = connect(&app, &other, &[]).await;
    let (err, v) = tool(
        &app,
        &oc.access,
        "get_release_corrections",
        json!({"release_id": release}),
    )
    .await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("FORBIDDEN"))
    );
}

/// A release taken through the MCP flow up to submission.
async fn submitted_release(
    app: &Router,
    pool: &PgPool,
    store: &MemStore,
    u: &User,
    access: &str,
    title: &str,
) -> (String, String) {
    let (_, rel) = tool(
        app,
        access,
        "create_release_draft",
        json!({"title": title, "release_type":"SINGLE","artist_name":"Han River",
        "release_date":"2027-04-01","p_line":"2026 Han River","c_line":"2026 Han River"}),
    )
    .await;
    let release = rel["release_id"].as_str().unwrap().to_string();
    let asset = register_asset(pool, store, u, "a.wav", AUDIO).await;
    let (_, v) = tool(
        app,
        access,
        "upsert_track",
        json!({"release_id": release, "title": title, "track_number": 1, "asset_id": asset}),
    )
    .await;
    let track = v["track_id"].as_str().unwrap().to_string();
    tool(
        app,
        access,
        "set_track_credits",
        json!({"release_id": release, "track_id": track, "composers": ["Kim"]}),
    )
    .await;
    let (_, p) = tool(
        app,
        access,
        "preview_submission",
        json!({"release_id": release, "declarations": declarations(true)}),
    )
    .await;
    let token = p["confirmation_token"]
        .as_str()
        .unwrap_or_else(|| panic!("{p}"))
        .to_string();
    let (err, v) = tool(
        app,
        access,
        "submit_release",
        json!({"release_id": release, "confirmation_token": token,
        "declarations": declarations(true), "artist_confirmed": true}),
    )
    .await;
    assert!(!err, "{v}");
    (release, track)
}

#[sqlx::test]
async fn promotion_codes_notifications_documents_and_support(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write", "scope_submit"]).await;

    // Pitch: not before submission, then within the lead time.
    let (_, draft) = tool(
        &app,
        &c.access,
        "create_release_draft",
        json!({"title":"Draft","release_type":"SINGLE","artist_name":"Han River"}),
    )
    .await;
    let (_, v) = tool(
        &app,
        &c.access,
        "upsert_track",
        json!({"release_id": draft["release_id"], "title": "Draft", "track_number": 1}),
    )
    .await;
    let (err, v) = tool(
        &app,
        &c.access,
        "submit_playlist_pitch",
        json!({"release_id": draft["release_id"], "track_id": v["track_id"],
        "description": "A slow night-drive song about the Han river."}),
    )
    .await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("PITCH_RELEASE_NOT_SUBMITTED")),
        "{v}"
    );

    let (release, track) =
        submitted_release(&app, &pool, &store, &u, &c.access, "Night Drive").await;
    let (err, v) = tool(
        &app,
        &c.access,
        "submit_playlist_pitch",
        json!({"release_id": release, "track_id": track,
        "description": "too short"}),
    )
    .await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("INVALID_ARGUMENTS"))
    );
    let (err, v) = tool(&app, &c.access, "submit_playlist_pitch", json!({"release_id": release, "track_id": track,
        "description": "A slow night-drive song about the Han river, built on warm synths.", "moods": "calm, nostalgic"})).await;
    assert!(!err, "{v}");
    assert_eq!(v["view"], "pitch");
    let (err, v) = tool(
        &app,
        &c.access,
        "submit_playlist_pitch",
        json!({"release_id": release, "track_id": track,
        "description": "A second pitch for the same release, which is not allowed."}),
    )
    .await;
    assert!(err, "one active pitch per release: {v}");
    let (err, _) = tool(
        &app,
        &c.access,
        "withdraw_playlist_pitch",
        json!({"release_id": release}),
    )
    .await;
    assert!(!err);
    let (err, v) = tool(
        &app,
        &c.access,
        "get_playlist_pitch",
        json!({"release_id": release}),
    )
    .await;
    assert!(!err, "{v}");

    // Smart link: saved links, public URL, fan alert counts.
    let (err, v) = tool(
        &app,
        &c.access,
        "save_smart_link",
        json!({"release_id": release, "links": {"spotify": "https://open.spotify.com/album/abc"}}),
    )
    .await;
    assert!(!err, "{v}");
    assert!(
        v["url"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{ORIGIN}/l/")),
        "{v}"
    );
    assert_eq!(
        v["link"]["links"]["spotify"],
        "https://open.spotify.com/album/abc"
    );
    assert_eq!(v["stats"]["alerts"], 0);
    let (err, v) = tool(
        &app,
        &c.access,
        "save_smart_link",
        json!({"release_id": release, "links": {"myspace": "https://myspace.com/x"}}),
    )
    .await;
    assert_eq!(
        (err, v["error"]["code"].as_str()),
        (true, Some("INVALID_ARGUMENTS"))
    );

    // Codes, notifications.
    let (err, v) = tool(&app, &c.access, "list_release_codes", json!({})).await;
    assert!(!err, "{v}");
    assert!(
        v["releases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r.to_string().contains(&release)),
        "{v}"
    );
    let (err, v) = tool(&app, &c.access, "list_notifications", json!({})).await;
    assert!(!err && v["unread"].is_number(), "{v}");

    // Documents: status and the Studio link, never the signature or legal text.
    support::set_agreement(&pool, u.org, uuid::Uuid::parse_str(&release).unwrap(), true).await;
    let (err, v) = tool(&app, &c.access, "list_documents", json!({})).await;
    assert!(!err, "{v}");
    let d = &v["documents"][0];
    assert_eq!(d["status"], "SIGNED");
    assert!(d["studio_url"].as_str().unwrap().contains("/contracts/"));
    assert!(
        !v.to_string().contains("data:image"),
        "no signature images: {v}"
    );

    // Support inquiries.
    let (err, v) = tool(
        &app,
        &c.access,
        "create_inquiry",
        json!({"category": "RELEASE", "release_id": release,
        "subject": "Release time", "body": "Can the release go out at 18:00 KST?"}),
    )
    .await;
    assert!(!err, "{v}");
    let inquiry = v["id"].as_str().unwrap().to_string();
    let (err, v) = tool(
        &app,
        &c.access,
        "reply_inquiry",
        json!({"inquiry_id": inquiry, "body": "Also on Spotify, please."}),
    )
    .await;
    assert!(!err, "{v}");
    assert_eq!(v["messages"].as_array().unwrap().len(), 2);
    let (err, v) = tool(&app, &c.access, "list_inquiries", json!({})).await;
    assert!(!err && v["items"].as_array().unwrap().len() == 1, "{v}");

    // Other accounts see none of it.
    let other = user(&app).await;
    let oc = connect(&app, &other, &["scope_write"]).await;
    for (name, args) in [
        ("get_inquiry", json!({"inquiry_id": inquiry})),
        ("get_smart_link", json!({"release_id": release})),
        ("get_playlist_pitch", json!({"release_id": release})),
    ] {
        let (err, v) = tool(&app, &oc.access, name, args).await;
        assert!(err, "{name}: {v}");
        assert!(
            matches!(v["error"]["code"].as_str(), Some("FORBIDDEN" | "NOT_FOUND")),
            "{name}: {v}"
        );
    }
    let (_, v) = tool(&app, &oc.access, "list_documents", json!({})).await;
    assert_eq!(v["documents"], json!([]));
}

// ---------------------------------------------------------------------------
// Other ways to connect: device authorization, password grant for trusted
// agents, Studio connection tokens.
// ---------------------------------------------------------------------------

/// HTTP Basic client authentication for `client_id:secret`.
fn basic(credentials: &str) -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(credentials)
    )
}

/// An operator-registered agent (the link-and-code flow is only for these):
/// `client_id:secret`.
async fn agent(pool: &PgPool) -> String {
    let (id, secret) = audeniq_core::mcp::login::admin_add_client(
        pool,
        "ops@audeniq",
        "Test <b>Assistant</b>",
        false,
        false,
        &[],
    )
    .await
    .unwrap();
    format!("{id}:{secret}")
}

async fn device_poll(app: &Router, client: &str, device_code: &str) -> (StatusCode, Value) {
    let auth = basic(client);
    let (s, _, body) = raw(
        app,
        "POST",
        "/api/oauth/token",
        &[
            ("content-type", "application/x-www-form-urlencoded"),
            ("authorization", &auth),
        ],
        form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ]),
    )
    .await;
    (s, serde_json::from_str(&body).unwrap_or(Value::Null))
}

async fn device_start(app: &Router, client: &str, scope: &str) -> Value {
    let auth = basic(client);
    let (s, h, body) = raw(
        app,
        "POST",
        "/api/oauth/device_authorization",
        &[
            ("content-type", "application/x-www-form-urlencoded"),
            ("authorization", &auth),
        ],
        form(&[("scope", scope)]),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(h["cache-control"], "no-store");
    serde_json::from_str(&body).unwrap()
}

fn request_field(body: &str) -> String {
    let start = body.find("name=\"request\" value=\"").unwrap() + "name=\"request\" value=\"".len();
    let end = body[start..].find('"').unwrap() + start;
    body[start..end].to_string()
}

#[sqlx::test]
async fn device_authorization_links_with_a_code(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let client_id = agent(&pool).await;
    // Public clients (DCR, or anyone naming ChatGPT's CIMD URL) cannot start
    // it: with no redirect to bind them, the link would be a phishing kit.
    let dcr = register_client(&app).await;
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/device_authorization",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[("client_id", &dcr)]),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body.contains("unauthorized_client"));

    let (_, _, meta) = raw(
        &app,
        "GET",
        "/.well-known/oauth-authorization-server",
        &[],
        String::new(),
    )
    .await;
    let meta: Value = serde_json::from_str(&meta).unwrap();
    assert_eq!(
        meta["device_authorization_endpoint"],
        format!("{ORIGIN}/api/oauth/device_authorization")
    );
    assert!(
        meta["grant_types_supported"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "urn:ietf:params:oauth:grant-type:device_code")
    );

    let d = device_start(
        &app,
        &client_id,
        "releases:read releases:write releases:submit",
    )
    .await;
    let device_code = d["device_code"].as_str().unwrap();
    let user_code = d["user_code"].as_str().unwrap();
    assert_eq!(user_code.len(), 9);
    assert_eq!(d["verification_uri"], format!("{ORIGIN}/api/oauth/device"));
    assert_eq!(d["interval"], 5);

    // Not approved yet; polling again at once is too fast.
    let (s, v) = device_poll(&app, &client_id, device_code).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"], "authorization_pending");
    let (_, v) = device_poll(&app, &client_id, device_code).await;
    assert_eq!(v["error"], "slow_down");
    // Another client cannot use the code.
    let other = agent(&pool).await;
    let (_, v) = device_poll(&app, &other, device_code).await;
    assert_eq!(v["error"], "invalid_grant");

    // The link: sign in first, then a wrong code, then the real one (any case).
    let page = format!("/api/oauth/device?user_code={}", user_code.to_lowercase());
    let (s, _, body) = raw(&app, "GET", &page, &[], String::new()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("먼저 어데닉에 로그인해 주세요"), "{body}");
    let (_, _, body) = raw(
        &app,
        "GET",
        "/api/oauth/device?user_code=BCDF-BCDF",
        &[("cookie", &u.cookie)],
        String::new(),
    )
    .await;
    assert!(body.contains("코드가 맞지 않거나 만료됐어요"), "{body}");
    let (s, _, body) = raw(&app, "GET", &page, &[("cookie", &u.cookie)], String::new()).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        body.contains(user_code),
        "the page shows the code to compare"
    );
    assert!(body.contains("Test &lt;b&gt;Assistant&lt;/b&gt;"));
    let request = request_field(&body);

    // The form needs the Studio Origin and the session that saw it.
    let org = u.org.to_string();
    let approve_form = form(&[
        ("request", &request),
        ("org", &org),
        ("decision", "approve"),
        ("scope_write", "on"),
        ("scope_submit", "on"),
    ]);
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/oauth/device",
        &[
            ("cookie", &u.cookie),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        approve_form.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let other_user = user(&app).await;
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/oauth/device",
        &[
            ("cookie", &other_user.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        approve_form.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/device",
        &[
            ("cookie", &u.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        approve_form.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("연결했어요"), "{body}");
    // The same code cannot be approved twice.
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/device",
        &[
            ("cookie", &u.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        approve_form,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body.contains("이미 처리됐어요"));

    let (s, v) = device_poll(&app, &client_id, device_code).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["scope"], "releases:read releases:write releases:submit");
    let access = v["access_token"].as_str().unwrap().to_string();
    let (err, acct) = tool(&app, &access, "get_account_context", json!({})).await;
    assert!(!err, "{acct}");
    assert_eq!(acct["workspace"]["id"], org);
    assert_eq!(acct["can_submit"], true);
    // A device code is used once.
    let (_, v) = device_poll(&app, &client_id, device_code).await;
    assert_eq!(v["error"], "invalid_grant");
    let (_, list) = call(&app, "GET", "/api/auth/ai-connections", json!({}), Some(&u)).await;
    assert_eq!(list["items"][0]["method"], "DEVICE");

    // Denied on the page → access_denied for the client.
    let d = device_start(&app, &client_id, "").await;
    let page = format!(
        "/api/oauth/device?user_code={}",
        d["user_code"].as_str().unwrap()
    );
    let (_, _, body) = raw(&app, "GET", &page, &[("cookie", &u.cookie)], String::new()).await;
    let request = request_field(&body);
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/device",
        &[
            ("cookie", &u.cookie),
            ("origin", ORIGIN),
            ("content-type", "application/x-www-form-urlencoded"),
        ],
        form(&[("request", &request), ("decision", "deny")]),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("연결을 거부했어요"));
    let (_, v) = device_poll(&app, &client_id, d["device_code"].as_str().unwrap()).await;
    assert_eq!(v["error"], "access_denied");

    // Expired codes.
    let d = device_start(&app, &client_id, "").await;
    sqlx::query("UPDATE identity.mcp_device_codes SET expires_at=now()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let (_, v) = device_poll(&app, &client_id, d["device_code"].as_str().unwrap()).await;
    assert_eq!(v["error"], "expired_token");
}

async fn password_token(
    app: &Router,
    client_id: &str,
    secret: &str,
    email: &str,
    password: &str,
    scope: &str,
) -> (StatusCode, Value) {
    use base64::Engine as _;
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{client_id}:{secret}"))
    );
    let (s, _, body) = raw(
        app,
        "POST",
        "/api/oauth/token",
        &[
            ("content-type", "application/x-www-form-urlencoded"),
            ("authorization", &basic),
        ],
        form(&[
            ("grant_type", "password"),
            ("username", email),
            ("password", password),
            ("scope", scope),
        ]),
    )
    .await;
    (s, serde_json::from_str(&body).unwrap_or(Value::Null))
}

#[sqlx::test]
async fn password_grant_only_for_registered_trusted_agents(pool: PgPool) {
    use audeniq_core::mcp::login;
    let (app, _) = app(pool.clone()).await;
    let email = format!("{}@example.test", Uuid::new_v4());
    let password = "Long-test-password-123!";
    let (s, r) = call(
        &app,
        "POST",
        "/api/auth/register",
        json!({"email": email, "password": password, "terms_version": "AUD-TERMS 2026.10.1"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");

    assert!(
        login::admin_add_client(&pool, "", "Muse", true, false, &[])
            .await
            .is_err(),
        "operator required"
    );
    let (muse, secret) = login::admin_add_client(&pool, "ops@audeniq", "Muse", true, false, &[])
        .await
        .unwrap();
    let (plain, plain_secret) =
        login::admin_add_client(&pool, "ops@audeniq", "Plain agent", false, false, &[])
            .await
            .unwrap();

    // A public client, a client without the flag, a wrong secret: refused.
    let dcr = register_client(&app).await;
    let (s, _, body) = raw(
        &app,
        "POST",
        "/api/oauth/token",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[
            ("grant_type", "password"),
            ("username", &email),
            ("password", password),
            ("client_id", &dcr),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{body}");
    assert!(body.contains("unauthorized_client"));
    let (s, v) = password_token(&app, &plain, &plain_secret, &email, password, "").await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("unauthorized_client"))
    );
    let (s, v) = password_token(&app, &muse, &"0".repeat(64), &email, password, "").await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("invalid_client"))
    );

    // Wrong password, payouts, then a real login.
    let (s, v) = password_token(&app, &muse, &secret, &email, "Wrong-password-12345", "").await;
    assert_eq!(
        (s, v["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let (_, v) = password_token(
        &app,
        &muse,
        &secret,
        &email,
        password,
        "releases:read payouts:request",
    )
    .await;
    assert_eq!(v["error"], "invalid_scope");
    let (s, v) = password_token(
        &app,
        &muse,
        &secret,
        &email.to_uppercase(),
        password,
        "releases:read releases:write settlement:read",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["scope"], "releases:read releases:write settlement:read");
    let access = v["access_token"].as_str().unwrap().to_string();
    let refresh = v["refresh_token"].as_str().unwrap().to_string();
    let (err, acct) = tool(&app, &access, "get_account_context", json!({})).await;
    assert!(!err, "{acct}");
    assert_eq!(acct["workspace"]["id"], r["org_id"]);
    assert_eq!(acct["connection"]["client"], "Muse");

    // The agent has no browser redirect.
    let (s, _, body) = raw(
        &app,
        "GET",
        &format!("/api/oauth/authorize?client_id={muse}&response_type=code"),
        &[],
        String::new(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(body.contains("브라우저 연결을 쓰지 않아요"), "{body}");

    let grants: Vec<String> =
        sqlx::query_scalar("SELECT method FROM identity.mcp_grants WHERE revoked_at IS NULL")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(grants, vec!["PASSWORD".to_string()]);

    // Disabling the agent ends its connections.
    login::admin_disable_client(&pool, "ops@audeniq", &muse)
        .await
        .unwrap();
    let (s, _, _) = rpc(
        &app,
        Some(&access),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    use base64::Engine as _;
    let basic = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{muse}:{secret}"))
    );
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/oauth/token",
        &[
            ("content-type", "application/x-www-form-urlencoded"),
            ("authorization", &basic),
        ],
        form(&[("grant_type", "refresh_token"), ("refresh_token", &refresh)]),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let listed = login::admin_list_clients(&pool).await.unwrap();
    assert_eq!(listed["items"].as_array().unwrap().len(), 2);
    assert!(listed["items"][0]["disabled_at"].is_string());
}

#[sqlx::test]
async fn connection_tokens_are_made_in_the_studio(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let org = u.org.to_string();
    for bad in [
        json!({"label": "", "org_id": org, "scopes": []}),
        json!({"label": "Agent", "org_id": org, "scopes": ["releases:delete"]}),
        json!({"label": "Agent", "org_id": org, "scopes": [], "days": 91}),
    ] {
        let (s, v) = call(
            &app,
            "POST",
            "/api/auth/ai-connections/tokens",
            bad,
            Some(&u),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    }
    let stranger = user(&app).await;
    let (s, _) = call(
        &app,
        "POST",
        "/api/auth/ai-connections/tokens",
        json!({"label": "Agent", "org_id": stranger.org, "scopes": []}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (s, v) = call(&app, "POST", "/api/auth/ai-connections/tokens",
        json!({"label": "My agent", "org_id": org, "scopes": ["releases:write", "settlement:read"], "days": 7}), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["scopes"],
        json!(["releases:read", "releases:write", "settlement:read"])
    );
    assert_eq!(v["mcp_url"], format!("{ORIGIN}/api/mcp"));
    let token = v["token"].as_str().unwrap().to_string();
    let id = v["id"].as_str().unwrap().to_string();
    let (err, acct) = tool(&app, &token, "get_account_context", json!({})).await;
    assert!(!err, "{acct}");
    assert_eq!(acct["can_view_settlement"], true);
    assert_eq!(acct["can_submit"], false);

    // A second token is a separate connection.
    let (s, _) = call(
        &app,
        "POST",
        "/api/auth/ai-connections/tokens",
        json!({"label": "Other", "org_id": org, "scopes": []}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = call(&app, "GET", "/api/auth/ai-connections", json!({}), Some(&u)).await;
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let mine = items.iter().find(|i| i["id"] == id.as_str()).unwrap();
    assert_eq!(mine["method"], "TOKEN");
    assert_eq!(mine["label"], "My agent");
    assert!(mine["expires_at"].is_string());

    // No end date, every permission: works until revoked.
    let (s, v) = call(&app, "POST", "/api/auth/ai-connections/tokens",
        json!({"label": "Forever", "org_id": org, "scopes": ["releases:write", "releases:submit", "settlement:read", "payouts:request"], "days": 30, "no_expiry": true}), Some(&u)).await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "days and no_expiry together: {v}"
    );
    let (s, v) = call(&app, "POST", "/api/auth/ai-connections/tokens",
        json!({"label": "Forever", "org_id": org, "scopes": ["releases:write", "releases:submit", "settlement:read", "payouts:request"], "no_expiry": true}), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["expires_at"], Value::Null);
    assert_eq!(v["no_expiry"], true);
    assert_eq!(v["scopes"].as_array().unwrap().len(), 5);
    let forever = v["token"].as_str().unwrap().to_string();
    let (err, acct) = tool(&app, &forever, "get_account_context", json!({})).await;
    assert!(!err, "{acct}");
    assert_eq!(acct["can_request_payouts"], true);
    let (_, list) = call(&app, "GET", "/api/auth/ai-connections", json!({}), Some(&u)).await;
    let f = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "Forever")
        .unwrap()
        .clone();
    assert_eq!(f["no_expiry"], true);
    assert_eq!(f["expires_at"], Value::Null);
    // Logging out everywhere ends it like any other connection.
    let (s, _) = call(&app, "POST", "/api/auth/logout-all", json!({}), Some(&u)).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = rpc(
        &app,
        Some(&forever),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let u = user(&app).await;
    let (s, v) = call(
        &app,
        "POST",
        "/api/auth/ai-connections/tokens",
        json!({"label": "My agent", "org_id": u.org, "scopes": [], "days": 7}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let token = v["token"].as_str().unwrap().to_string();
    let id = v["id"].as_str().unwrap().to_string();

    // Revoking it in the Studio stops it at once.
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-connections/{id}/revoke"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = rpc(
        &app,
        Some(&token),
        "tools/call",
        json!({"name":"get_account_context","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // The built-in token client cannot start a browser or device flow.
    let (s, _, _) = raw(
        &app,
        "POST",
        "/api/oauth/device_authorization",
        &[("content-type", "application/x-www-form-urlencoded")],
        form(&[("client_id", "audeniq-connection-token")]),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// Public tools: no account needed, daily limits per IP / per user.
// ---------------------------------------------------------------------------

async fn anon_tool(
    app: &Router,
    ip: &str,
    name: &str,
    args: Value,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let (s, h, body) = raw(
        app,
        "POST",
        "/api/mcp",
        &[
            ("content-type", "application/json"),
            ("accept", "application/json, text/event-stream"),
            ("mcp-protocol-version", "2025-06-18"),
            ("x-audeniq-client-ip", ip),
        ],
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string(),
    )
    .await;
    (s, h, serde_json::from_str(&body).unwrap_or(Value::Null))
}

#[sqlx::test]
async fn public_tools_need_no_account_within_daily_limits(pool: PgPool) {
    use audeniq_core::mcp::public;
    let (app, _) = app(pool.clone()).await;
    sqlx::query("INSERT INTO content.notices(id,title,body,pinned,published_at) VALUES
        ('n-holiday','추석 연휴 안내','연휴 동안\n\n검토가 늦어질 수 있어요.',true,now()-interval '1 day'),
        ('n-later','예약 공지','아직',false,now()+interval '1 day')")
        .execute(&pool).await.unwrap();
    let ip = "203.0.113.5";

    let (s, _, v) = anon_tool(&app, ip, "list_announcements", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let r = &v["result"]["structuredContent"];
    assert_eq!(v["result"]["isError"], false);
    assert_eq!(r["total"], 1, "scheduled notices stay hidden");
    assert_eq!(r["items"][0]["title"], "추석 연휴 안내");
    assert_eq!(
        r["items"][0]["excerpt"],
        "연휴 동안 검토가 늦어질 수 있어요."
    );
    assert_eq!(r["items"][0]["url"], format!("{ORIGIN}/notices/n-holiday"));
    assert!(r["items"][0].get("body").is_none());
    assert_eq!(r["usage"], json!({"signed_in": false}));

    let (_, _, v) = anon_tool(
        &app,
        ip,
        "get_announcement",
        json!({"kind":"notices","id":"n-holiday"}),
    )
    .await;
    assert_eq!(
        v["result"]["structuredContent"]["body"],
        "연휴 동안\n\n검토가 늦어질 수 있어요."
    );
    let (_, _, v) = anon_tool(
        &app,
        ip,
        "get_announcement",
        json!({"kind":"notices","id":"n-later"}),
    )
    .await;
    assert_eq!(
        v["result"]["structuredContent"]["error"]["code"],
        "NOT_FOUND"
    );
    let (_, _, v) = anon_tool(&app, ip, "get_service_status", json!({})).await;
    assert_eq!(v["result"]["isError"], false, "{v}");
    let (_, _, v) = anon_tool(&app, ip, "list_announcements", json!({"kind":"events"})).await;
    assert_eq!(v["result"]["structuredContent"]["items"], json!([]));
    let (_, _, v) = anon_tool(&app, ip, "get_release_requirements", json!({})).await;
    assert_eq!(v["result"]["isError"], false);
    // Other tools still need an account.
    let (s, _, _) = anon_tool(&app, ip, "list_releases", json!({})).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Without an account nothing is counted: no daily cap.
    for _ in 0..30 {
        let (s, _, v) = anon_tool(&app, ip, "list_announcements", json!({})).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM identity.mcp_daily_usage")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);

    // Signed in: the user's own daily count, then support.
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write"]).await;
    let (err, v) = tool(&app, &c.access, "list_announcements", json!({})).await;
    assert!(!err, "{v}");
    assert_eq!(v["usage"]["signed_in"], true);
    assert_eq!(v["usage"]["limit"], 150);
    public::admin_set_limit(&pool, "ops@audeniq", u.user, Some(2), Some("test"))
        .await
        .unwrap();
    let (err, _) = tool(&app, &c.access, "get_service_status", json!({})).await;
    assert!(!err);
    let (err, v) = tool(&app, &c.access, "get_service_status", json!({})).await;
    assert!(err);
    assert_eq!(v["error"]["code"], "DAILY_LIMIT_REACHED");
    assert_eq!(
        v["error"]["details"]["support_url"],
        format!("{ORIGIN}/inquiries")
    );
    // Account tools are not counted against it.
    let (err, _) = tool(&app, &c.access, "get_account_context", json!({})).await;
    assert!(!err);
    // Support raises it.
    public::admin_set_limit(&pool, "ops@audeniq", u.user, Some(500), None)
        .await
        .unwrap();
    let (err, v) = tool(&app, &c.access, "get_service_status", json!({})).await;
    assert!(!err, "{v}");
    assert_eq!(v["usage"]["limit"], 500);
    assert_eq!(
        public::admin_list_limits(&pool).await.unwrap()["items"][0]["daily_limit"],
        500
    );
    assert!(
        public::admin_set_limit(&pool, "", u.user, Some(1), None)
            .await
            .is_err()
    );
    // A bad token on a public tool is 401 (the client refreshes), not anonymous use.
    let (s, _, _) = rpc(
        &app,
        Some(&"0".repeat(64)),
        "tools/call",
        json!({"name":"get_service_status","arguments":{}}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn token_endpoint_limits_only_failures_and_answers_in_oauth_shape(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &[]).await;
    let ip = "192.0.2.44";
    let post = |body: String| {
        let app = app.clone();
        async move {
            let (s, h, b) = raw(
                &app,
                "POST",
                "/api/oauth/token",
                &[
                    ("content-type", "application/x-www-form-urlencoded"),
                    ("x-audeniq-client-ip", ip),
                ],
                body,
            )
            .await;
            (
                s,
                h,
                serde_json::from_str::<Value>(&b).unwrap_or(Value::Null),
            )
        }
    };
    // Many good refreshes from one address (an AI platform refreshing for
    // all of its users) are never limited.
    let mut rt = c.refresh.clone();
    for _ in 0..40 {
        let (s, _, v) = post(form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &rt),
            ("client_id", &c.client_id),
        ]))
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        rt = v["refresh_token"].as_str().unwrap().to_string();
    }
    // Floods of bad tokens are: once the address has too many failures it
    // gets OAuth's temporarily_unavailable with Retry-After (not our own
    // error shape, which OAuth libraries treat as fatal).
    sqlx::query("INSERT INTO identity.auth_limits(bucket_hash,attempts) VALUES($1,300) ON CONFLICT(bucket_hash) DO UPDATE SET attempts=300, window_start=now()")
        .bind(audeniq_core::auth::bucket_hash(&format!("mcp-token-fail:src:{ip}")))
        .execute(&pool)
        .await
        .unwrap();
    let (s, h, v) = post(form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", &rt),
        ("client_id", &c.client_id),
    ]))
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["error"], "temporarily_unavailable");
    assert_eq!(h["retry-after"], "60");
    // Other addresses are unaffected.
    let (s, _, b) = raw(
        &app,
        "POST",
        "/api/oauth/token",
        &[
            ("content-type", "application/x-www-form-urlencoded"),
            ("x-audeniq-client-ip", "192.0.2.45"),
        ],
        form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &rt),
            ("client_id", &c.client_id),
        ]),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
}

#[sqlx::test]
async fn submit_release_is_marked_destructive(pool: PgPool) {
    let (app, _) = app(pool).await;
    let (_, _, v) = rpc(&app, None, "tools/list", json!({})).await;
    let submit = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "submit_release")
        .unwrap()
        .clone();
    assert_eq!(submit["annotations"]["destructiveHint"], true);
}

#[sqlx::test]
async fn artist_pages_platform_requests_splits_prompts_and_upload_links(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write", "scope_settlement"]).await;

    // Artist pages: listed, registered from links the artist gave.
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/artists", u.org),
        json!({"name":"Han River"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let artist = v["id"].as_str().unwrap().to_string();
    let (err, v) = tool(&app, &c.access, "list_artist_pages", json!({})).await;
    assert!(!err, "{v}");
    assert_eq!(v["items"][0]["name"], "Han River");
    let (err, v) = tool(&app, &c.access, "set_artist_pages", json!({"artist_id": artist,
        "links": {"spotify": "https://open.spotify.com/artist/0123456789abcdefABCDEF", "melon": "https://www.melon.com/artist/detail.htm?artistId=123"}})).await;
    assert!(!err, "{v}");
    assert_eq!(v["saved"], 2);
    let (err, v) = tool(
        &app,
        &c.access,
        "set_artist_pages",
        json!({"artist_id": artist,
        "links": {"spotify": "https://evil.example/artist/x"}}),
    )
    .await;
    assert!(err);
    assert_eq!(v["error"]["code"], "ARTIST_PAGE_URL_INVALID");
    let (_, v) = tool(&app, &c.access, "list_artist_pages", json!({})).await;
    assert!(
        v["items"][0]["pages"]["spotify"]["url"]
            .as_str()
            .unwrap()
            .starts_with("https://open.spotify.com/artist/")
    );

    // Platform requests: filed, listed, cancelled.
    let (err, v) = tool(&app, &c.access, "create_platform_request", json!({"kind": "PROFILE_MERGE", "platform": "melon",
        "artist_name": "Han River", "urls": ["https://www.melon.com/artist/detail.htm?artistId=1", "https://www.melon.com/artist/detail.htm?artistId=2"]})).await;
    assert!(!err, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let (err, v) = tool(&app, &c.access, "create_platform_request", json!({"kind": "PROFILE_SPLIT", "platform": "melon",
        "artist_name": "Han River", "urls": ["https://www.melon.com/artist/detail.htm?artistId=1"]})).await;
    assert!(err, "a split needs a description");
    assert_eq!(v["error"]["code"], "PLATFORM_REQUEST_DESCRIPTION_REQUIRED");
    let (_, v) = tool(
        &app,
        &c.access,
        "list_platform_requests",
        json!({"group": "profile"}),
    )
    .await;
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    let (err, v) = tool(
        &app,
        &c.access,
        "cancel_platform_request",
        json!({"request_id": id}),
    )
    .await;
    assert!(!err, "{v}");
    assert_eq!(v["status"], "CANCELLED");

    // Splits and upload link on a draft.
    let (err, rel) = tool(
        &app,
        &c.access,
        "create_release_draft",
        json!({"title": "Spring", "release_type": "SINGLE", "artist_name": "Han River"}),
    )
    .await;
    assert!(!err, "{rel}");
    let release = rel["release_id"].as_str().unwrap().to_string();
    let (err, v) = tool(
        &app,
        &c.access,
        "get_release_splits",
        json!({"release_id": release}),
    )
    .await;
    assert!(!err, "{v}");
    assert!(v["tracks"].is_array());
    let (err, v) = tool(&app, &c.access, "list_received_splits", json!({})).await;
    assert!(!err);
    assert_eq!(v["items"], json!([]));
    let (err, v) = tool(
        &app,
        &c.access,
        "get_upload_link",
        json!({"release_id": release}),
    )
    .await;
    assert!(!err, "{v}");
    assert_eq!(v["upload_url"], format!("{ORIGIN}/upload?edit={release}"));
    assert_eq!(v["artwork_missing"], true);
    assert_eq!(v["complete"], false);
    // Splits need the settlement permission.
    let read_only = connect(&app, &u, &["scope_write"]).await;
    let (s, _, _) = rpc(
        &app,
        Some(&read_only.access),
        "tools/call",
        json!({"name": "get_release_splits", "arguments": {"release_id": release}}),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Prompts.
    let (s, _, v) = rpc(&app, None, "prompts/list", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let names: Vec<&str> = v["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"prepare_release"), "{names:?}");
    let (_, _, v) = rpc(
        &app,
        None,
        "prompts/get",
        json!({"name": "prepare_release", "arguments": {"title": "봄"}}),
    )
    .await;
    let text = v["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("어데닉") && text.contains("「봄」"), "{text}");
    let (_, _, v) = rpc(&app, None, "prompts/get", json!({"name": "nope"})).await;
    assert_eq!(v["error"]["code"], -32602);
    let (_, _, v) = rpc(&app, None, "initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}})).await;
    assert!(v["result"]["capabilities"]["prompts"].is_object());
}

/// A complete draft and its preview's confirmation token (not submitted).
async fn previewed_release(
    app: &Router,
    pool: &PgPool,
    store: &MemStore,
    u: &User,
    access: &str,
    title: &str,
) -> (String, String) {
    let (_, rel) = tool(
        app,
        access,
        "create_release_draft",
        json!({"title": title, "release_type":"SINGLE","artist_name":"Han River",
        "release_date":"2027-04-01","p_line":"2026 Han River","c_line":"2026 Han River"}),
    )
    .await;
    let release = rel["release_id"].as_str().unwrap().to_string();
    let asset = register_asset(pool, store, u, "a.wav", AUDIO).await;
    let (_, v) = tool(
        app,
        access,
        "upsert_track",
        json!({"release_id": release, "title": title, "track_number": 1, "asset_id": asset}),
    )
    .await;
    tool(
        app,
        access,
        "set_track_credits",
        json!({"release_id": release, "track_id": v["track_id"], "composers": ["Kim"]}),
    )
    .await;
    let (_, p) = tool(
        app,
        access,
        "preview_submission",
        json!({"release_id": release, "declarations": declarations(true)}),
    )
    .await;
    let token = p["confirmation_token"]
        .as_str()
        .unwrap_or_else(|| panic!("{p}"))
        .to_string();
    (release, token)
}

async fn notices(pool: &PgPool, user: Uuid) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT title FROM portal.notifications WHERE user_id=$1 ORDER BY created_at",
    )
    .bind(user)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn activity_notices_retry_safe_creates_and_studio_approval(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write", "scope_submit"]).await;
    // A new connection is announced to the user.
    assert_eq!(
        notices(&pool, u.user).await,
        vec!["새 AI 앱이 연결됐어요".to_string()]
    );

    // Retry-safe create: the same client_request_id returns the first draft.
    let args = json!({"title":"Once","release_type":"SINGLE","artist_name":"Han River","client_request_id":"req-0001-abcd"});
    let (err, first) = tool(&app, &c.access, "create_release_draft", args.clone()).await;
    assert!(!err, "{first}");
    let (err, again) = tool(&app, &c.access, "create_release_draft", args).await;
    assert!(!err, "{again}");
    assert_eq!(again["release_id"], first["release_id"]);
    assert_eq!(again["duplicate_request"], true);
    let drafts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog.releases WHERE org_id=$1 AND title='Once'",
    )
    .bind(u.org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(drafts, 1);
    let (_, _, v) = rpc(&app, None, "tools/list", json!({})).await;
    let create = v["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "create_release_draft")
        .unwrap()
        .clone();
    assert!(create["inputSchema"]["properties"]["client_request_id"].is_object());

    // Submitting without Studio approval: submitted, announced; a retry
    // after the release moved on still reports the same application.
    let (release, token) = previewed_release(&app, &pool, &store, &u, &c.access, "Direct").await;
    let submit = |release: String, token: String| {
        let app = app.clone();
        let access = c.access.clone();
        async move {
            tool(
                &app,
                &access,
                "submit_release",
                json!({"release_id": release, "confirmation_token": token,
                "declarations": declarations(true), "artist_confirmed": true}),
            )
            .await
        }
    };
    let (err, v) = submit(release.clone(), token.clone()).await;
    assert!(!err, "{v}");
    assert_eq!(v["view"], "submitted");
    sqlx::query("UPDATE catalog.releases SET status='IN_REVIEW' WHERE id=$1::uuid")
        .bind(&release)
        .execute(&pool)
        .await
        .ok();
    let (err, again) = submit(release.clone(), token).await;
    assert!(!err, "{again}");
    assert_eq!(again["duplicate_request"], true);
    assert_eq!(again["application_id"], v["application_id"]);
    assert!(
        notices(&pool, u.user)
            .await
            .contains(&"AI 앱으로 발매를 신청했어요".to_string())
    );

    // Studio approval on: the AI submission waits for the artist.
    let (s, v) = call(
        &app,
        "PUT",
        "/api/auth/ai-settings",
        json!({"studio_approval": true}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, v["studio_approval"].as_bool()),
        (StatusCode::OK, Some(true))
    );
    let (release, token) = previewed_release(&app, &pool, &store, &u, &c.access, "Held").await;
    let (err, v) = submit(release.clone(), token.clone()).await;
    assert!(!err, "{v}");
    assert_eq!(v["view"], "pending_approval");
    let status: String =
        sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1::uuid")
            .bind(&release)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "DRAFT", "nothing is submitted before approval");
    let (_, again) = submit(release.clone(), token.clone()).await;
    assert_eq!(again["duplicate_request"], true);
    assert!(
        notices(&pool, u.user)
            .await
            .contains(&"AI 앱이 보낸 발매 신청을 승인해 주세요".to_string())
    );
    let (s, list) = call(
        &app,
        "GET",
        "/api/auth/ai-submissions",
        json!(null),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "Held");
    assert_eq!(items[0]["declarations"]["rights_confirmed"], true);
    let id = items[0]["id"].as_str().unwrap().to_string();
    // Only that user can decide.
    let other = user(&app).await;
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-submissions/{id}/approve"),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-submissions/{id}/approve"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "APPROVED");
    let status: String =
        sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1::uuid")
            .bind(&release)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(status, "DRAFT");
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-submissions/{id}/approve"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "decided once");
    // The assistant's retry now sees the application.
    let (_, again) = submit(release.clone(), token).await;
    assert_eq!(again["view"], "submitted");

    // A release changed after the chat confirmation cannot be approved.
    let (release, token) = previewed_release(&app, &pool, &store, &u, &c.access, "Changed").await;
    let (_, v) = submit(release.clone(), token).await;
    assert_eq!(v["view"], "pending_approval");
    let id = v["approval_id"].as_str().unwrap().to_string();
    let (err, v) = tool(
        &app,
        &c.access,
        "update_release_draft",
        json!({"release_id": release, "genre": "Jazz"}),
    )
    .await;
    assert!(!err, "{v}");
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-submissions/{id}/approve"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "AI_SUBMISSION_STALE");

    // Rejecting leaves the draft as it was.
    let (release, token) = previewed_release(&app, &pool, &store, &u, &c.access, "Rejected").await;
    let (_, v) = submit(release.clone(), token).await;
    let id = v["approval_id"].as_str().unwrap().to_string();
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/auth/ai-submissions/{id}/reject"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, v["status"].as_str()),
        (StatusCode::OK, Some("REJECTED"))
    );
    let (_, list) = call(
        &app,
        "GET",
        "/api/auth/ai-submissions",
        json!(null),
        Some(&u),
    )
    .await;
    assert_eq!(list["items"], json!([]));
}

#[sqlx::test]
async fn parallel_track_writes_retries_and_protected_lyrics(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write"]).await;
    let (_, rel) = tool(
        &app,
        &c.access,
        "create_release_draft",
        json!({"title":"Album","release_type":"ALBUM","artist_name":"Han River"}),
    )
    .await;
    let release = rel["release_id"].as_str().unwrap().to_string();

    // An assistant adds eight tracks at once: every track is saved and no
    // call reports a failure for a write that went through.
    let mut set = tokio::task::JoinSet::new();
    for n in 1..=8 {
        let app = app.clone();
        let access = c.access.clone();
        let release = release.clone();
        set.spawn(async move {
            tool(
                &app,
                &access,
                "upsert_track",
                json!({"release_id": release, "title": format!("Song {n}"), "track_number": n}),
            )
            .await
        });
    }
    let results = set.join_all().await;
    let saved: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog.tracks WHERE release_id=$1::uuid AND archived_at IS NULL",
    )
    .bind(&release)
    .fetch_one(&pool)
    .await
    .unwrap();
    for (err, v) in &results {
        // A lost race before anything was written is an honest CONFLICT the
        // assistant can retry; a saved write never reports failure.
        if *err {
            assert_eq!(v["error"]["code"], "CONFLICT", "{v}");
        }
    }
    let ok = results.iter().filter(|(err, _)| !err).count() as i64;
    assert_eq!(saved, ok, "every saved track was reported as saved");
    assert!(ok >= 1);

    assert_eq!(
        ok, 8,
        "parallel writes retry on the latest version: {results:?}"
    );
    // A retry at a taken position names the existing track.
    let existing: String = sqlx::query_scalar("SELECT id::text FROM catalog.tracks WHERE release_id=$1::uuid AND track_number=1 AND archived_at IS NULL")
        .bind(&release).fetch_one(&pool).await.unwrap();
    let (err, v) = tool(
        &app,
        &c.access,
        "upsert_track",
        json!({"release_id": release, "title": "Again", "track_number": 1}),
    )
    .await;
    assert!(err, "{v}");
    assert_eq!(v["error"]["code"], "TRACK_POSITION_TAKEN");
    assert_eq!(v["error"]["details"]["track_id"], existing);

    // Lyrics naming a protected artist are refused before anything is saved.
    let before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM catalog.tracks WHERE release_id=$1::uuid")
            .bind(&release)
            .fetch_one(&pool)
            .await
            .unwrap();
    let (err, v) = tool(
        &app,
        &c.access,
        "upsert_track",
        json!({"release_id": release, "title": "Cover", "track_number": 20,
        "lyrics": "I sing like Taylor Swift tonight"}),
    )
    .await;
    assert!(err);
    assert_eq!(v["error"]["code"], "ARTIST_NAME_PROTECTED");
    let after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM catalog.tracks WHERE release_id=$1::uuid")
            .bind(&release)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, after);
    // The release stays editable afterwards.
    let (err, v) = tool(
        &app,
        &c.access,
        "upsert_track",
        json!({"release_id": release, "title": "Fine", "track_number": 21}),
    )
    .await;
    assert!(!err, "{v}");

    // A refused title leaves no orphan artist behind.
    let artists = || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM catalog.artists WHERE org_id=$1")
            .bind(u.org)
            .fetch_one(&pool)
            .await
            .unwrap()
    };
    let n = artists().await;
    let (err, _) = tool(&app, &c.access, "create_release_draft",
        json!({"title":"Bad","release_type":"SINGLE","artist_name":"Brand New Artist","upc":"123456789990"})).await;
    assert!(err);
    assert_eq!(artists().await, n);
}

#[sqlx::test]
async fn approval_is_decided_once_and_abandoned_request_ids_are_reclaimed(pool: PgPool) {
    use audeniq_core::mcp::safety;
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let c = connect(&app, &u, &["scope_write", "scope_submit"]).await;
    let (s, _) = call(
        &app,
        "PUT",
        "/api/auth/ai-settings",
        json!({"studio_approval": true}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (release, token) = previewed_release(&app, &pool, &store, &u, &c.access, "Race").await;
    let (_, v) = tool(
        &app,
        &c.access,
        "submit_release",
        json!({"release_id": release, "confirmation_token": token,
        "declarations": declarations(true), "artist_confirmed": true}),
    )
    .await;
    let id = v["approval_id"].as_str().unwrap().to_string();

    // Approve and reject at once (two tabs): exactly one decides.
    let mut set = tokio::task::JoinSet::new();
    for action in ["approve", "reject"] {
        let app = app.clone();
        let u = u.clone();
        let path = format!("/api/auth/ai-submissions/{id}/{action}");
        set.spawn(async move { call(&app, "POST", &path, json!({}), Some(&u)).await });
    }
    let results = set.join_all().await;
    let ok: Vec<_> = results
        .iter()
        .filter(|(s, _)| *s == StatusCode::OK)
        .collect();
    assert_eq!(ok.len(), 1, "{results:?}");
    let status: String =
        sqlx::query_scalar("SELECT status FROM identity.mcp_pending_submissions WHERE id=$1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let release_status: String =
        sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1::uuid")
            .bind(&release)
            .fetch_one(&pool)
            .await
            .unwrap();
    // The record matches what happened to the release.
    assert_eq!(
        status == "APPROVED",
        release_status != "DRAFT",
        "{status} / {release_status}"
    );

    // A client_request_id whose first call never finished is reclaimed after two minutes.
    let grant: Uuid = sqlx::query_scalar(
        "SELECT id FROM identity.mcp_grants WHERE user_id=$1 AND revoked_at IS NULL LIMIT 1",
    )
    .bind(u.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(matches!(
        safety::reserve(&pool, grant, "create_inquiry", "req-stuck-0001")
            .await
            .unwrap(),
        safety::Reserved::New
    ));
    assert!(matches!(
        safety::reserve(&pool, grant, "create_inquiry", "req-stuck-0001")
            .await
            .unwrap(),
        safety::Reserved::Running
    ));
    sqlx::query("UPDATE identity.mcp_idempotency SET created_at=now()-interval '3 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        safety::reserve(&pool, grant, "create_inquiry", "req-stuck-0001")
            .await
            .unwrap(),
        safety::Reserved::New
    ));
}
