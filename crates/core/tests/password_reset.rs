//! 휴대폰 번호 등록과 문자 인증 비밀번호 재설정 against real PostgreSQL.
//! Own test binary: it switches `SMS_PROVIDER`, a process-wide variable.
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database, sms,
    storage::DisabledStore,
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

const SECRET: &str = "test-only-service-secret-32-characters";
const ORIGIN: &str = "http://localhost:5173";

async fn app(pool: PgPool) -> Router {
    // SAFETY: this binary has a single test; nothing reads the env concurrently.
    unsafe { std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32)) };
    database::MIGRATOR.run(&pool).await.unwrap();
    let s = AppState::new(
        pool,
        Config {
            database_url: "unused".into(),
            origin: ORIGIN.into(),
            service_secret: SECRET.into(),
            secure_cookie: false,
            bind: "127.0.0.1:0".into(),
            session_seconds: 3600,
            test_only_bypass_dsp_gate: true,
        },
        Arc::new(DisabledStore),
    )
    .await
    .unwrap();
    router(s)
}

struct User {
    email: String,
    cookie: String,
    csrf: String,
}

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    u: Option<&User>,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("content-type", "application/json");
    if let Some(u) = u {
        b = b
            .header("cookie", &u.cookie)
            .header("x-csrf-token", &u.csrf);
    }
    let r = app
        .clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn user(app: &Router) -> User {
    let email = format!("{}@example.test", Uuid::new_v4());
    let credentials = json!({"email": email, "password": "Long-test-password-123!", "terms_version":"AUD-TERMS 2026.10.1"});
    let (s, r) = call(app, "POST", "/api/auth/register", credentials.clone(), None).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("x-audeniq-service", SECRET)
                .header("origin", ORIGIN)
                .header("content-type", "application/json")
                .body(Body::from(credentials.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let cookie = r.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let l: Value = serde_json::from_slice(&bytes).unwrap();
    User {
        email,
        cookie,
        csrf: l["csrf_token"].as_str().unwrap().into(),
    }
}

async fn login(app: &Router, email: &str, password: &str) -> StatusCode {
    call(
        app,
        "POST",
        "/api/auth/login",
        json!({"email": email, "password": password}),
        None,
    )
    .await
    .0
}

#[sqlx::test]
async fn phone_registration_and_password_reset(pool: PgPool) {
    let app = app(pool.clone()).await;
    let u = user(&app).await;
    let phone = "010-2345-6789";

    // No SMS contract yet: refused before any lookup.
    // SAFETY: single test in this binary.
    unsafe { std::env::remove_var("SMS_PROVIDER") };
    let (s, e) = call(
        &app,
        "POST",
        "/api/auth/password-reset/start",
        json!({"email": u.email, "phone": phone}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    assert_eq!(e["error"]["code"], "SMS_PROVIDER_NOT_CONFIGURED");
    let (_, a) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert!(a["phone"].is_null());
    assert_eq!(a["sms_ready"], false);

    unsafe { std::env::set_var("SMS_PROVIDER", "test") };
    // Register the phone (signed in).
    let (s, e) = call(
        &app,
        "POST",
        "/api/me/phone/start",
        json!({"phone": "02-123-4567"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "PHONE_INVALID");
    let (s, start) = call(
        &app,
        "POST",
        "/api/me/phone/start",
        json!({"phone": phone}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{start}");
    assert_eq!(start["phone_last4"], "6789");
    let code = sms::test_last_code(phone).expect("code sent");
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let (s, e) = call(
        &app,
        "POST",
        "/api/me/phone/verify",
        json!({"challenge_id": start["challenge_id"], "code": wrong}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "SMS_CODE_INVALID");
    let (s, v) = call(
        &app,
        "POST",
        "/api/me/phone/verify",
        json!({"challenge_id": start["challenge_id"], "code": code}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["phone"]["last4"], "6789");
    // A used challenge cannot be replayed.
    let (s, _) = call(
        &app,
        "POST",
        "/api/me/phone/verify",
        json!({"challenge_id": start["challenge_id"], "code": code}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, a) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert_eq!(a["phone"]["last4"], "6789");
    assert_eq!(a["sms_ready"], true);
    let stored: String = sqlx::query_scalar("SELECT phone_hash FROM portal.account_phones")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        !stored.contains("23456789"),
        "phone number is not stored in clear"
    );

    // Unknown email or another phone: same answer shape, no SMS.
    let sent_before = sms::test_last_code("010-9999-8888");
    let (s, r) = call(
        &app,
        "POST",
        "/api/auth/password-reset/start",
        json!({"email": u.email, "phone": "010-9999-8888"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(r["reset_id"].is_string() && r["expires_at"].is_string());
    assert_eq!(
        sms::test_last_code("010-9999-8888"),
        sent_before,
        "no SMS for a phone not on the account"
    );
    let (s, decoy) = call(
        &app,
        "POST",
        "/api/auth/password-reset/start",
        json!({"email": "nobody@example.test", "phone": phone}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, e) = call(
        &app,
        "POST",
        "/api/auth/password-reset/verify",
        json!({"reset_id": decoy["reset_id"], "code": code}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "RESET_CODE_INVALID");

    // Five wrong codes close the request even if the right code follows.
    let (_, r) = call(
        &app,
        "POST",
        "/api/auth/password-reset/start",
        json!({"email": u.email.to_uppercase(), "phone": "01023456789"}),
        None,
    )
    .await;
    let code = sms::test_last_code(phone).unwrap();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        let (s, _) = call(
            &app,
            "POST",
            "/api/auth/password-reset/verify",
            json!({"reset_id": r["reset_id"], "code": wrong}),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    let (s, e) = call(
        &app,
        "POST",
        "/api/auth/password-reset/verify",
        json!({"reset_id": r["reset_id"], "code": code}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "RESET_CODE_INVALID");

    // The real reset.
    let (s, r) = call(
        &app,
        "POST",
        "/api/auth/password-reset/start",
        json!({"email": u.email, "phone": phone}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let code = sms::test_last_code(phone).unwrap();
    let (s, t) = call(
        &app,
        "POST",
        "/api/auth/password-reset/verify",
        json!({"reset_id": r["reset_id"], "code": code}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let token = t["reset_token"].as_str().unwrap().to_string();
    // The verified code cannot mint a second token.
    let (s, _) = call(
        &app,
        "POST",
        "/api/auth/password-reset/verify",
        json!({"reset_id": r["reset_id"], "code": code}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &app,
        "POST",
        "/api/auth/password-reset/complete",
        json!({"reset_token": token, "new_password": "short"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, done) = call(
        &app,
        "POST",
        "/api/auth/password-reset/complete",
        json!({"reset_token": token, "new_password": "Brand-new-password-456!"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{done}");
    assert_eq!(done["reauthentication_required"], true);
    // Token is single use; old sessions are gone; only the new password works.
    let (s, e) = call(
        &app,
        "POST",
        "/api/auth/password-reset/complete",
        json!({"reset_token": token, "new_password": "Another-password-789!"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "RESET_TOKEN_INVALID");
    let (s, _) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "sessions revoked");
    assert_eq!(
        login(&app, &u.email, "Long-test-password-123!").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, &u.email, "Brand-new-password-456!").await,
        StatusCode::OK
    );

    // Browser requests from another origin are refused.
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/password-reset/start")
                .header("x-audeniq-service", SECRET)
                .header("origin", "https://evil.example")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"email": u.email, "phone": phone}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    unsafe { std::env::remove_var("SMS_PROVIDER") };

    let audits: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM operations.audit_events WHERE action LIKE 'auth.password_reset%' OR action='portal.account.phone_verified' ORDER BY action",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(
        audits.contains(&"auth.password_reset".to_string()),
        "{audits:?}"
    );
    assert!(audits.contains(&"portal.account.phone_verified".to_string()));
}
