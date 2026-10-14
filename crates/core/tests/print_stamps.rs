//! Printouts carry a document code and a per-print verification code; the
//! public check shows a masked summary, staff see who, where and which device.
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

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// A request from a known address and browser.
async fn from_device(
    app: &Router,
    method: &str,
    uri: &str,
    body: Value,
    u: Option<&User>,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .header("x-audeniq-client-ip", "203.0.113.7")
        .header("user-agent", "Mozilla/5.0 (Macintosh) TestBrowser/1.0");
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
    let s = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    (s, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[sqlx::test]
async fn printouts_are_identified_and_verifiable(pool: PgPool) {
    // Safety: this test binary only ever sets these values.
    unsafe {
        std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32));
        std::env::set_var("STAFF_MFA_REQUIRED", "false");
    }
    let (app, _) = app(pool.clone()).await;
    let owner = user(&app).await;
    let other = user(&app).await;
    let release = create_release(&app, &owner).await;
    let device = Uuid::new_v4();

    // The owner prints the release application.
    let (s, a) = from_device(
        &app,
        "POST",
        "/api/print-stamps",
        json!({"kind": "APPLICATION", "ref": release, "content_hash": HASH_A, "device_id": device}),
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let doc_code = a["document_code"].as_str().unwrap().to_string();
    let code = a["verification_code"].as_str().unwrap().to_string();
    assert!(
        doc_code.starts_with("AQ-APP-") && doc_code.len() == 17,
        "{doc_code}"
    );
    assert_eq!(code.len(), 23, "{code}");
    assert!(a["title"].as_str().unwrap().ends_with("발매 신청서"));
    assert!(a["printed_by"].as_str().unwrap().contains("***@"), "{a}");
    assert_eq!(a["verify_url"], format!("{ORIGIN}/verify"));

    // Same content → same document code, new verification code; changed content → new document code.
    let (_, b) = from_device(
        &app,
        "POST",
        "/api/print-stamps",
        json!({"kind": "APPLICATION", "ref": release, "content_hash": HASH_A}),
        Some(&owner),
    )
    .await;
    assert_eq!(b["document_code"], doc_code);
    assert_ne!(b["verification_code"], code);
    let (_, c) = from_device(
        &app,
        "POST",
        "/api/print-stamps",
        json!({"kind": "APPLICATION", "ref": release, "content_hash": HASH_B}),
        Some(&owner),
    )
    .await;
    assert_ne!(c["document_code"], doc_code);

    // Nobody else can stamp (or learn about) this release.
    let (s, _) = from_device(
        &app,
        "POST",
        "/api/print-stamps",
        json!({"kind": "APPLICATION", "ref": release}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = from_device(
        &app,
        "POST",
        "/api/print-stamps",
        json!({"kind": "APPLICATION", "ref": release}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Public terms print without signing in.
    let (s, t) = from_device(&app, "POST", "/api/print-stamps",
        json!({"kind": "TERMS", "ref": "AUD-TERMS 2026.10", "title": "서비스 이용약관", "content_hash": HASH_A}), None).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["printed_by"], "로그인하지 않은 방문자");
    assert!(t["document_code"].as_str().unwrap().starts_with("AQ-TRM-"));

    // A signing-link recipient (no account) prints the document being signed.
    let token = audeniq_core::auth::random_token();
    let req = Uuid::new_v4();
    sqlx::query("INSERT INTO portal.signing_requests(id,org_id,release_id,document_no,form,document_kind,title,body,body_hash,rights_holder,signer_name,signer_role,channel,token_hash,expires_at,created_by)
                 VALUES($1,$2,$3,$4,'AUD-RIGHTS 2.0','master','마스터 권리 확인서','본문',$5,'서린','홍길동','권리자 본인','LINK',$6,now()+interval '1 day',$7)")
        .bind(req).bind(owner.org).bind(release).bind(Uuid::new_v4()).bind(HASH_A)
        .bind(audeniq_core::auth::hash_token(&token)).bind(owner.user)
        .execute(&pool).await.unwrap();
    let (s, g) = from_device(
        &app,
        "POST",
        "/api/print-stamps",
        json!({"kind": "SIGNING", "ref": token}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{g}");
    assert_eq!(g["printed_by"], "서명 요청 수신자 홍**");
    let stored: String = sqlx::query_scalar(
        "SELECT doc_ref FROM operations.print_stamps WHERE signing_request_id=$1",
    )
    .bind(req)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, req.to_string(), "the link token is never stored");

    // Anyone with the paper can check it: typed in lower case without dashes.
    let typed = code.replace('-', "").to_lowercase();
    let (s, v) = send(
        &app,
        "GET",
        &format!("/api/print-stamps/verify?code={typed}"),
        Value::Null,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["valid"], true);
    assert_eq!(v["document_code"], doc_code);
    assert_eq!(v["content_hash"], HASH_A);
    assert!(
        v.get("source").is_none() && v.get("device_id").is_none(),
        "public view hides the source"
    );
    let (_, v) = send(
        &app,
        "GET",
        "/api/print-stamps/verify?code=ZZZZZ-ZZZZZ-ZZZZZ-ZZZZZ",
        Value::Null,
        None,
    )
    .await;
    assert_eq!(v, json!({"valid": false}));

    // Staff see the whole record: who, session, device, address and browser.
    let staff = user(&app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','test-operator')")
        .bind(staff.user).execute(&pool).await.unwrap();
    let (s, full) = send(
        &app,
        "GET",
        &format!("/api/staff/print-stamps/verify?code={code}"),
        Value::Null,
        Some(&staff),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{full}");
    assert_eq!(full["user_id"], owner.user.to_string());
    assert_eq!(full["device_id"], device.to_string());
    assert!(full["session_id"].is_string());
    assert_eq!(full["source"]["ip"], "203.0.113.7");
    assert!(
        full["source"]["user_agent"]
            .as_str()
            .unwrap()
            .contains("TestBrowser")
    );
    assert!(
        full["printed_by"].as_str().unwrap().contains('@')
            && !full["printed_by"].as_str().unwrap().contains("***")
    );
    assert_eq!(full["prints_of_this_document"], 2);
    let (s, _) = send(
        &app,
        "GET",
        &format!("/api/staff/print-stamps/verify?code={code}"),
        Value::Null,
        Some(&owner),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Staff stamp their review sheets.
    let (s, st) = from_device(
        &app,
        "POST",
        "/api/staff/print-stamps",
        json!({"kind": "STAFF_SHEET", "ref": release}),
        Some(&staff),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert!(st["document_code"].as_str().unwrap().starts_with("AQ-SUB-"));
    assert!(
        st["printed_by"]
            .as_str()
            .unwrap()
            .starts_with("AUDENIQ 직원")
    );

    // Nothing is stored in plain text, and records cannot be changed.
    let dump: String = sqlx::query_scalar(
        "SELECT string_agg(row_to_json(p)::text, ' ') FROM operations.print_stamps p",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        !dump.contains("203.0.113.7")
            && !dump.contains("TestBrowser")
            && !dump.contains(&code.replace('-', ""))
    );
    assert!(
        sqlx::query("UPDATE operations.print_stamps SET title='x'")
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM operations.print_stamps")
            .execute(&pool)
            .await
            .is_err()
    );
}
