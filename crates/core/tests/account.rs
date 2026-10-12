//! 계정 정보 API (본인인증·해외 납세 의무) against real PostgreSQL.
//! Own test binary: it switches `IDENTITY_PROVIDER`, a process-wide variable.
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
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
        cookie,
        csrf: l["csrf_token"].as_str().unwrap().into(),
    }
}

#[sqlx::test]
async fn account_identity_and_tax(pool: PgPool) {
    let app = app(pool.clone()).await;
    let u = user(&app).await;
    let other = user(&app).await;

    // Signed out: nothing.
    let (s, _) = call(&app, "GET", "/api/me/account", Value::Null, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Empty account; no provider contract yet → verification refused.
    // SAFETY: single test in this binary.
    unsafe { std::env::remove_var("IDENTITY_PROVIDER") };
    let (s, a) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["real_name"], "");
    assert!(a["verified"].is_null() && a["tax"].is_null());
    assert_eq!(a["identity_provider"]["ready"], false);
    let (s, e) = call(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:홍길동:1990-01-01"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{e}");
    assert_eq!(e["error"]["code"], "IDENTITY_PROVIDER_NOT_CONFIGURED");

    unsafe { std::env::set_var("IDENTITY_PROVIDER", "test") };
    // CSRF is required for the write.
    let no_csrf = User {
        cookie: u.cookie.clone(),
        csrf: "wrong".into(),
    };
    let (s, _) = call(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:홍길동:1990-01-01"}),
        Some(&no_csrf),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, e) = call(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:홍길동:not-a-date"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "IDENTITY_NOT_VERIFIED");

    let (s, v) = call(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:홍길동:1990-01-01"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["real_name"], "홍길동",
        "the provider's name becomes the real name"
    );
    assert_eq!(v["verified"]["method"], "TEST");
    assert_eq!(v["verified"]["adult"], true);
    let (_, a) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert_eq!(a["real_name"], "홍길동");
    let plain: (Option<String>, Option<chrono::NaiveDate>, bool) = sqlx::query_as(
        "SELECT real_name, birth_date, real_name_cipher IS NOT NULL AND birth_date_cipher IS NOT NULL
           FROM portal.account_identities LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        plain,
        (None, None, true),
        "real name and birth date stored sealed"
    );
    assert_eq!(a["verified"]["provider"], "test");
    assert_eq!(
        a["identity_provider"],
        json!({"ready": true, "name": "test"})
    );
    assert!(
        a.to_string().find("1990").is_none(),
        "birth date is never returned"
    );

    // The same provider transaction cannot verify another account.
    let (s, e) = call(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:홍길동:1990-01-01"}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "IDENTITY_NOT_VERIFIED");
    // A verified account can only be verified again by the same person.
    let (s, e) = call(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:김철수:1985-05-05"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "IDENTITY_ACCOUNT_MISMATCH");
    // A minor is verified but reported as not adult.
    let (s, v) = call(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:이영희:2015-03-03"}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["verified"]["adult"], false);
    unsafe { std::env::remove_var("IDENTITY_PROVIDER") };

    // Tax residency: domestic only.
    let (s, t) = call(
        &app,
        "PUT",
        "/api/me/tax",
        json!({"foreign": false, "row_version": 0}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["row_version"], 1);
    // Stale version → conflict.
    let (s, _) = call(
        &app,
        "PUT",
        "/api/me/tax",
        json!({"foreign": true, "country": "US", "row_version": 0}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, e) = call(
        &app,
        "PUT",
        "/api/me/tax",
        json!({"foreign": true, "country": "KR", "row_version": 0}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(e["error"]["code"], "TAX_COUNTRY_INVALID");
    let (s, t) = call(&app, "PUT", "/api/me/tax",
        json!({"foreign": true, "country": "US", "tin": "123-45-6789", "us_person": true, "row_version": 1}), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["row_version"], 2);
    let (_, a) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert_eq!(a["tax"]["foreign"], true);
    assert_eq!(a["tax"]["country"], "US");
    assert_eq!(a["tax"]["tin_last4"], "6789");
    assert_eq!(a["tax"]["us_person"], true);
    assert!(
        !a.to_string().contains("123456789"),
        "full TIN never returned"
    );
    let raw: Vec<u8> = sqlx::query_scalar("SELECT tin_cipher FROM portal.tax_residencies LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        !raw.windows(9).any(|w| w == b"123456789"),
        "TIN sealed at rest"
    );
    // keep_tin keeps the stored number while other fields change.
    let (s, t) = call(&app, "PUT", "/api/me/tax",
        json!({"foreign": true, "country": "US", "keep_tin": true, "us_person": false, "row_version": 2}), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let (_, a) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert_eq!(a["tax"]["tin_last4"], "6789");
    assert_eq!(a["tax"]["us_person"], false);
    // Back to domestic clears the foreign details.
    let (s, _) = call(
        &app,
        "PUT",
        "/api/me/tax",
        json!({"foreign": false, "row_version": 3}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, a) = call(&app, "GET", "/api/me/account", Value::Null, Some(&u)).await;
    assert_eq!(a["tax"]["foreign"], false);
    assert!(a["tax"]["tin_last4"].is_null());
    // Another user's account stays separate.
    let (_, b) = call(&app, "GET", "/api/me/account", Value::Null, Some(&other)).await;
    assert!(b["tax"].is_null());
    assert_eq!(b["real_name"], "이영희");

    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events WHERE action IN ('portal.account.identity_verified','portal.account.tax_updated')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 6);
}
