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
        "releases:read releases:write releases:submit",
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
    assert_eq!(tools.len(), 13);
    for t in tools {
        assert_eq!(
            t["inputSchema"]["additionalProperties"], false,
            "{}",
            t["name"]
        );
        assert_eq!(t["securitySchemes"][0]["type"], "oauth2");
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
    let asset = register_asset(&pool, &store, &u, "night.wav", support::wav_bytes()).await;
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
    let asset = register_asset(&pool, &store, &u, "t.wav", support::wav_bytes()).await;
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
