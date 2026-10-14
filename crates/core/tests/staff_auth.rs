//! Staff sign-in protection: TOTP second factor, idle timeout, and the weak
//! password rules (개인정보의 안전성 확보조치 기준, NIST SP 800-63B).
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

mod support;
use support::*;

const PASSWORD: &str = "Long-test-password-123!";

async fn staff_user(app: &Router, pool: &PgPool) -> (User, String) {
    let s = user(app).await;
    sqlx::query(
        "INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','test-operator')",
    )
    .bind(s.user)
    .execute(pool)
    .await
    .unwrap();
    let keys = audeniq_core::payout_keys::KeyRing::from_env(false).unwrap();
    let email = audeniq_core::pii::user_email(&keys, pool, s.user)
        .await
        .unwrap();
    (s, email)
}

/// A second session for the same account.
async fn login(app: &Router, base: &User, email: &str) -> User {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("x-audeniq-service", SECRET)
                .header("origin", ORIGIN)
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"email": email, "password": PASSWORD}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    let l: Value = serde_json::from_slice(&bytes).unwrap();
    User {
        cookie,
        csrf: l["csrf_token"].as_str().unwrap().into(),
        ..base.clone()
    }
}

fn code(secret: &str, offset_steps: i64) -> String {
    let raw = audeniq_core::staff_mfa::base32_decode(secret).unwrap();
    let step = chrono::Utc::now().timestamp() / 30 + offset_steps;
    format!("{:06}", audeniq_core::staff_mfa::hotp(&raw, step as u64))
}

#[sqlx::test]
async fn staff_need_a_second_factor_and_idle_sessions_end(pool: PgPool) {
    // Safety: this test binary only ever sets these values.
    unsafe {
        std::env::set_var("STAFF_MFA_REQUIRED", "true");
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
    }
    let (app, _) = app(pool.clone()).await;
    let (staff, email) = staff_user(&app, &pool).await;

    // Password alone does not open the console.
    let (s, v) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&staff)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "STAFF_MFA_ENROLL_REQUIRED");
    let (_, v) = send(
        &app,
        "GET",
        "/api/staff/overview",
        Value::Null,
        Some(&staff),
    )
    .await;
    assert_eq!(v["error"]["code"], "STAFF_MFA_ENROLL_REQUIRED");
    let (s, v) = send(&app, "GET", "/api/staff/mfa", Value::Null, Some(&staff)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v,
        json!({"required": true, "enrolled": false, "verified": false})
    );

    // Enrollment can restart until the first code is confirmed.
    let (_, first) = send(
        &app,
        "POST",
        "/api/staff/mfa/enroll",
        json!({}),
        Some(&staff),
    )
    .await;
    let (s, e) = send(
        &app,
        "POST",
        "/api/staff/mfa/enroll",
        json!({}),
        Some(&staff),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{e}");
    let secret = e["secret"].as_str().unwrap().to_string();
    assert_ne!(first["secret"], e["secret"]);
    assert_eq!(secret.len(), 32, "160-bit secret");
    let uri = e["otpauth_uri"].as_str().unwrap();
    assert!(
        uri.starts_with("otpauth://totp/AUDENIQ%20Admin%3A") && uri.contains(&secret),
        "{uri}"
    );
    // Stored sealed: the database never holds the secret.
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT totp_cipher FROM identity.staff_mfa WHERE user_id=$1")
            .bind(staff.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!stored.windows(8).any(|w| w == &secret.as_bytes()[..8]));

    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/mfa/verify",
        json!({"code": code(&secret, 5)}),
        Some(&staff),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "STAFF_MFA_CODE_INVALID");
    let now_code = code(&secret, 0);
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/mfa/verify",
        json!({"code": now_code}),
        Some(&staff),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&staff)).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // A new session needs its own code; a used code is refused (replay).
    let other = login(&app, &staff, &email).await;
    let (_, v) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&other)).await;
    assert_eq!(v["error"]["code"], "STAFF_MFA_REQUIRED");
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/mfa/verify",
        json!({"code": now_code}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // An enrolled factor cannot be replaced from a password-only session.
    let (_, v) = send(
        &app,
        "POST",
        "/api/staff/mfa/enroll",
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(v["error"]["code"], "STAFF_MFA_ALREADY_ENROLLED");
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/mfa/verify",
        json!({"code": code(&secret, 1)}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&other)).await;
    assert_eq!(s, StatusCode::OK);

    // Activity is recorded at most once a minute; 30 idle minutes end the session.
    sqlx::query(
        "UPDATE identity.sessions SET last_seen_at=now()-interval '5 minutes' WHERE user_id=$1",
    )
    .bind(staff.user)
    .execute(&pool)
    .await
    .unwrap();
    let (s, _) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&staff)).await;
    assert_eq!(s, StatusCode::OK);
    let fresh: bool = sqlx::query_scalar(
        "SELECT bool_or(last_seen_at > now()-interval '1 minute') FROM identity.sessions WHERE user_id=$1",
    )
    .bind(staff.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(fresh);
    sqlx::query(
        "UPDATE identity.sessions SET last_seen_at=now()-interval '31 minutes' WHERE user_id=$1",
    )
    .bind(staff.user)
    .execute(&pool)
    .await
    .unwrap();
    let (s, _) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&staff)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = send(&app, "GET", "/api/staff/mfa", Value::Null, Some(&other)).await;
    assert_eq!(
        s,
        StatusCode::UNAUTHORIZED,
        "idle sessions are revoked, not only refused"
    );
    let revoked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.sessions WHERE user_id=$1 AND revoked_at IS NOT NULL",
    )
    .bind(staff.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revoked, 2);

    // Lost device: an owner resets the factor; sessions end; enrollment restarts.
    let third = login(&app, &staff, &email).await;
    audeniq_core::staff_mfa::reset(&pool, "test-operator", staff.user)
        .await
        .unwrap();
    let (s, _) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&third)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let fourth = login(&app, &staff, &email).await;
    let (_, v) = send(&app, "GET", "/api/staff/me", Value::Null, Some(&fourth)).await;
    assert_eq!(v["error"]["code"], "STAFF_MFA_ENROLL_REQUIRED");

    // Members who are not staff never reach the factor endpoints.
    let member = user(&app).await;
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/mfa/enroll",
        json!({}),
        Some(&member),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM operations.audit_events WHERE resource_id=$1 AND action LIKE 'staff.mfa.%' ORDER BY occurred_at",
    )
    .bind(staff.user)
    .fetch_all(&pool)
    .await
    .unwrap_or_default();
    for a in [
        "staff.mfa.failed",
        "staff.mfa.enrolled",
        "staff.mfa.verified",
        "staff.mfa.reset",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a} audited: {actions:?}");
    }
}

#[sqlx::test]
async fn weak_passwords_are_refused_on_every_path(pool: PgPool) {
    unsafe { std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32)) };
    let (app, _) = app(pool.clone()).await;
    let email = format!("{}@example.test", Uuid::new_v4().simple());
    for weak in ["password1234", "1q2w3e4r5t6y7u", "aaaaaaaaaaaaaa"] {
        let (s, v) = send(
            &app,
            "POST",
            "/api/auth/register",
            json!({"email": email, "password": weak, "terms_version":"AUD-TERMS 2026.10.1"}),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{weak}: {v}");
        assert_eq!(v["error"]["code"], "PASSWORD_TOO_WEAK");
    }
    let u = user(&app).await;
    let (s, v) = send(
        &app,
        "POST",
        "/api/auth/password",
        json!({"current_password": PASSWORD, "new_password": "qwertyuiop1234"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "PASSWORD_TOO_WEAK");
    // Stored as Argon2id with the pinned parameters.
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM identity.users WHERE id=$1")
        .bind(u.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{hash}"
    );
}
