//! 직원 실명인증 (현장·고객센터에서 신분증으로) against real PostgreSQL.
//! Own test binary: it sets `IDENTITY_PROVIDER`, a process-wide variable.
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

/// A signed-in user with a known e-mail.
async fn member(app: &Router, email: &str) -> User {
    let credentials = json!({"email":email,"password":"Long-test-password-123!","terms_version":"AUD-TERMS 2026.10.1"});
    let (s, r) = send(app, "POST", "/api/auth/register", credentials.clone(), None).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let resp = app
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
    let cookie = resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let l: Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap(),
    )
    .unwrap();
    User {
        user: r["user_id"].as_str().unwrap().parse().unwrap(),
        org: r["org_id"].as_str().unwrap().parse().unwrap(),
        party: r["party_id"].as_str().unwrap().parse().unwrap(),
        cookie,
        csrf: l["csrf_token"].as_str().unwrap().into(),
    }
}

async fn staff(app: &Router, pool: &PgPool, role: &str) -> User {
    let u = user(app).await;
    sqlx::query(
        "INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,$2,'test-operator')",
    )
    .bind(u.user)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    u
}

fn id_card(name: &str, birth: &str) -> Value {
    json!({"real_name": name, "birth_date": birth, "channel": "CUSTOMER_CENTER", "document_type": "RESIDENT_CARD"})
}

#[sqlx::test(migrations = false)]
async fn staff_verify_real_names_from_an_id(pool: PgPool) {
    // SAFETY: this binary has a single test; nothing reads the env concurrently.
    unsafe { std::env::set_var("IDENTITY_PROVIDER", "test") };
    let (app, _) = app(pool.clone()).await;
    let email = format!("{}@Example.test", Uuid::new_v4());
    let artist = member(&app, &email).await;
    let support = staff(&app, &pool, "SUPPORT").await;
    let reviewer = staff(&app, &pool, "REVIEWER").await;
    let admin = staff(&app, &pool, "ADMIN").await;

    // Only ADMIN and SUPPORT; artists are not staff.
    for u in [&reviewer, &artist] {
        let (s, _) = send(
            &app,
            "POST",
            "/api/staff/identity/lookup",
            json!({"email": email}),
            Some(u),
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/identity/lookup",
        json!({"email": "nobody@example.test"}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // The e-mail matches whatever its case; the answer masks it.
    let (s, v) = send(
        &app,
        "POST",
        "/api/staff/identity/lookup",
        json!({"email": email.to_uppercase()}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["user_id"], artist.user.to_string());
    assert!(v["email_masked"].as_str().unwrap().contains("***@"), "{v}");
    assert!(v["identity"].is_null());

    let verify = format!("/api/staff/identity/{}/verify", artist.user);
    // Input checks; no ID numbers are accepted at all.
    for (body, code) in [
        (id_card("", "1990-01-01"), "REAL_NAME_INVALID"),
        (id_card("홍길동", "1990-02-30"), "BIRTH_DATE_INVALID"),
        (id_card("홍길동", "2999-01-01"), "BIRTH_DATE_INVALID"),
    ] {
        let (s, v) = send(&app, "POST", &verify, body, Some(&support)).await;
        assert_eq!(
            (s, v["error"]["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some(code)),
            "{v}"
        );
    }
    let mut with_rrn = id_card("홍길동", "1990-01-01");
    with_rrn["resident_number"] = json!("900101-1234567");
    let (s, _) = send(&app, "POST", &verify, with_rrn, Some(&support)).await;
    assert!(s.is_client_error());
    // Staff cannot verify themselves.
    let (s, v) = send(
        &app,
        "POST",
        &format!("/api/staff/identity/{}/verify", support.user),
        id_card("직원", "1990-01-01"),
        Some(&support),
    )
    .await;
    assert_eq!(v["error"]["code"], "IDENTITY_SELF_VERIFY", "{s} {v}");

    // Verified at the customer center.
    let (s, v) = send(
        &app,
        "POST",
        &verify,
        id_card(" 홍길동 ", "1990-01-01"),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["identity"]["provider"], "OFFLINE");
    assert_eq!(v["identity"]["method"], "CUSTOMER_CENTER");
    assert_eq!(v["identity"]["name_masked"], "홍**");
    assert_eq!(v["identity"]["adult"], true);
    assert_eq!(v["checks"][0]["staff_user_id"], support.user.to_string());

    // The artist sees it as their verified real name, and was told.
    let (_, acct) = send(&app, "GET", "/api/me/account", Value::Null, Some(&artist)).await;
    assert_eq!(acct["real_name"], "홍길동");
    assert_eq!(acct["verified"]["provider"], "OFFLINE");
    let told: i64 = sqlx::query_scalar("SELECT count(*) FROM portal.notifications WHERE user_id=$1 AND kind='ACCOUNT' AND title='실명인증을 마쳤어요'")
        .bind(artist.user).fetch_one(&pool).await.unwrap();
    assert_eq!(told, 1);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action='portal.account.identity_verified' AND actor_user_id=$1")
        .bind(support.user).fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 1);

    // A different person on the same account is refused; SUPPORT cannot correct.
    let (_, v) = send(
        &app,
        "POST",
        &verify,
        id_card("김철수", "1990-01-01"),
        Some(&support),
    )
    .await;
    assert_eq!(v["error"]["code"], "IDENTITY_ACCOUNT_MISMATCH");
    let mut fix = id_card("홍길돈", "1990-01-01");
    fix["correction"] = json!(true);
    fix["note"] = json!("이름 오타");
    let (s, _) = send(&app, "POST", &verify, fix.clone(), Some(&support)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // ADMIN corrects a typo, with a reason.
    let mut no_reason = fix.clone();
    no_reason["note"] = json!("");
    let (_, v) = send(&app, "POST", &verify, no_reason, Some(&admin)).await;
    assert_eq!(v["error"]["code"], "NOTE_REQUIRED");
    let (s, v) = send(&app, "POST", &verify, fix, Some(&admin)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["checks"][0]["correction"], true);
    assert_eq!(v["checks"].as_array().unwrap().len(), 2);

    // Online verification later must be the same person as the ID check.
    let (_, v) = send(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:김철수:1990-01-01"}),
        Some(&artist),
    )
    .await;
    assert_eq!(v["error"]["code"], "IDENTITY_ACCOUNT_MISMATCH", "{v}");
    let (s, v) = send(
        &app,
        "POST",
        "/api/me/identity",
        json!({"transaction_id": "test:홍길돈:1990-01-01"}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["verified"]["provider"], "test");
    // Now verified by the provider: staff can still record a check, but the
    // provider record (with its CI) stays, and it cannot be "corrected".
    let (s, v) = send(
        &app,
        "POST",
        &verify,
        id_card("홍길돈", "1990-01-01"),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["identity"]["provider"], "test");
    let mut fix = id_card("홍길동", "1990-01-01");
    fix["correction"] = json!(true);
    fix["note"] = json!("x");
    let (_, v) = send(&app, "POST", &verify, fix, Some(&admin)).await;
    assert_eq!(v["error"]["code"], "IDENTITY_ACCOUNT_MISMATCH");
    let ci: Option<String> =
        sqlx::query_scalar("SELECT ci_hash FROM portal.account_identities WHERE user_id=$1")
            .bind(artist.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(ci.is_some());
    // The check log is append-only for the API role.
    let checks: i64 =
        sqlx::query_scalar("SELECT count(*) FROM portal.offline_identity_checks WHERE user_id=$1")
            .bind(artist.user)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(checks, 3);

    // Minors: no resident card yet, so a youth card or a family relation
    // certificate with the legal guardian who came along. Adults cannot use those.
    let (_, v) = send(
        &app,
        "POST",
        &verify,
        json!({"real_name": "홍길돈", "birth_date": "1990-01-01", "channel": "IN_PERSON", "document_type": "YOUTH_CARD"}),
        Some(&support),
    )
    .await;
    assert_eq!(v["error"]["code"], "IDENTITY_MINORS_ONLY", "{v}");
    let kid_email = format!("{}@example.test", Uuid::new_v4());
    let kid = member(&app, &kid_email).await;
    let kid_verify = format!("/api/staff/identity/{}/verify", kid.user);
    let family = |guardian: Value| {
        json!({"real_name": "홍아기", "birth_date": "2016-03-01", "channel": "IN_PERSON",
               "document_type": "FAMILY_RELATION_CERT", "guardian_relation": guardian})
    };
    let (s, v) = send(
        &app,
        "POST",
        &kid_verify,
        family(Value::Null),
        Some(&support),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("GUARDIAN_REQUIRED")),
        "{v}"
    );
    let (s, _) = send(
        &app,
        "POST",
        &kid_verify,
        family(json!("UNCLE")),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v) = send(
        &app,
        "POST",
        &kid_verify,
        family(json!("PARENT")),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["identity"]["provider"], "OFFLINE");
    assert_eq!(v["identity"]["adult"], false);
    assert_eq!(v["checks"][0]["document_type"], "FAMILY_RELATION_CERT");
    assert_eq!(v["checks"][0]["guardian_relation"], "PARENT");
    let (_, acct) = send(&app, "GET", "/api/me/account", Value::Null, Some(&kid)).await;
    assert_eq!(acct["real_name"], "홍아기");
    assert_eq!(acct["verified"]["adult"], false, "{acct}");
}
