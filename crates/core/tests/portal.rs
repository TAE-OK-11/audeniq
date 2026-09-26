//! Studio portal API against real PostgreSQL (profile, payout account,
//! inquiries, notifications, documents, applications, settlement).
use async_trait::async_trait;
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    error::{Error, Result},
    storage::{ObjectMeta, ObjectStore, UploadGrant},
};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;
const SECRET: &str = "test-only-service-secret-32-characters";
const ORIGIN: &str = "http://localhost:5173";
#[derive(Default)]
struct MockStore {
    objects: Mutex<BTreeMap<String, ObjectMeta>>,
    /// Explicit object bodies; objects without one get synthesized bytes
    /// whose magic matches their content type (see `get`).
    bodies: Mutex<BTreeMap<String, Vec<u8>>>,
    calls: std::sync::atomic::AtomicUsize,
}
/// Deterministic placeholder bytes: `size` long, starting with the container
/// magic of `content_type`, so upload completion's content sniff passes.
fn synth_body(content_type: &str, size: i64) -> Vec<u8> {
    let magic: &[u8] = match content_type {
        "audio/wav" | "audio/x-wav" => b"RIFF\0\0\0\0WAVE",
        "audio/flac" => b"fLaC",
        "image/png" => b"\x89PNG\r\n\x1a\n",
        "image/jpeg" => b"\xFF\xD8\xFF",
        _ => b"",
    };
    let mut v = vec![0u8; size.max(0) as usize];
    let n = magic.len().min(v.len());
    v[..n].copy_from_slice(&magic[..n]);
    v
}
#[async_trait]
impl ObjectStore for MockStore {
    async fn presign_put(
        &self,
        key: &str,
        size: i64,
        mime: &str,
        nonce: &str,
        expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        Ok(UploadGrant {
            url: format!("https://mock.invalid/{key}"),
            method: "PUT",
            headers: BTreeMap::from([
                ("content-length".into(), size.to_string()),
                ("content-type".into(), mime.into()),
                ("x-amz-meta-upload-nonce".into(), nonce.into()),
            ]),
            expires_at: expires,
        })
    }
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.objects.lock().await.get(key).cloned())
    }
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut m = self.objects.lock().await;
        let obj = m.get(source).cloned().ok_or(Error::Storage)?;
        if obj.etag != etag {
            return Err(Error::Conflict);
        }
        m.insert(target.into(), obj);
        let mut b = self.bodies.lock().await;
        if let Some(body) = b.get(source).cloned() {
            b.insert(target.into(), body);
        }
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        if let Some(b) = self.bodies.lock().await.get(key) {
            return Ok(b.clone());
        }
        let m = self.objects.lock().await;
        let meta = m.get(key).ok_or(Error::Storage)?;
        Ok(synth_body(&meta.content_type, meta.size))
    }
}
async fn app(pool: PgPool) -> (Router, Arc<MockStore>) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(MockStore::default());
    let s = AppState::new(
        pool,
        Config {
            database_url: "unused".into(),
            origin: ORIGIN.into(),
            service_secret: SECRET.into(),
            secure_cookie: false,
            bind: "127.0.0.1:0".into(),
            session_seconds: 3600,
        },
        store.clone(),
    )
    .await
    .unwrap();
    (router(s), store)
}

#[derive(Clone)]
struct User {
    #[allow(dead_code)]
    user: Uuid,
    org: Uuid,
    party: Uuid,
    cookie: String,
    csrf: String,
}
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    user: Option<&User>,
) -> (StatusCode, HeaderMap, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("content-type", "application/json");
    if let Some(u) = user {
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
    let headers = r.headers().clone();
    let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}
async fn user(app: &Router) -> User {
    let email = format!("{}@example.test", Uuid::new_v4());
    let credentials = json!({"email":email,"password":"Long-test-password-123!"});
    let (s, _, r) = call(app, "POST", "/api/auth/register", credentials.clone(), None).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (s, h, l) = call(app, "POST", "/api/auth/login", credentials, None).await;
    assert_eq!(s, StatusCode::OK, "{l}");
    User {
        user: Uuid::parse_str(r["user_id"].as_str().unwrap()).unwrap(),
        org: Uuid::parse_str(r["org_id"].as_str().unwrap()).unwrap(),
        party: Uuid::parse_str(r["party_id"].as_str().unwrap()).unwrap(),
        cookie: h["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into(),
        csrf: l["csrf_token"].as_str().unwrap().into(),
    }
}
async fn create(app: &Router, u: &User, kind: &str) -> Uuid {
    let body = match kind {
        "labels" => json!({"name":"Label","party_id":u.party}),
        "releases" => json!({"name":"Draft","release_type":"SINGLE"}),
        _ => json!({"name":"Artist"}),
    };
    let (s, _, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/{kind}", u.org),
        body,
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}

fn key() {
    // SAFETY: every test sets the same constant before touching the API.
    unsafe { std::env::set_var("PAYOUT_ACCOUNT_KEY", "3a".repeat(32)) };
}
fn org_path(u: &User, p: &str) -> String {
    format!("/api/orgs/{}{p}", u.org)
}
const SIG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

#[sqlx::test]
async fn profile_is_per_user_with_optimistic_versions(pool: PgPool) {
    let (app, _) = app(pool).await;
    let u = user(&app).await;
    let (s, _, v) = call(&app, "GET", "/api/me/profile", Value::Null, Some(&u)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["row_version"], 0);
    assert!(
        v["contact_email"]
            .as_str()
            .unwrap()
            .ends_with("@example.test")
    );
    let body = json!({"display_name":"서린","contact_email":"a@b.kr","bio":"첫 줄\n둘째 줄","country":"kr","row_version":0});
    let (s, _, v) = call(&app, "PUT", "/api/me/profile", body.clone(), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // Stale version and bidi control characters are refused.
    let (s, _, _) = call(&app, "PUT", "/api/me/profile", body, Some(&u)).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let bad = json!({"display_name":"a\u{202e}b","contact_email":"","bio":"","country":"KR","row_version":1});
    let (s, _, v) = call(&app, "PUT", "/api/me/profile", bad, Some(&u)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (_, _, v) = call(&app, "GET", "/api/me/profile", Value::Null, Some(&u)).await;
    assert_eq!(v["display_name"], "서린");
    assert_eq!(v["country"], "KR");
    assert_eq!(v["row_version"], 0);
    // Another user sees only their own profile.
    let other = user(&app).await;
    let (_, _, v) = call(&app, "GET", "/api/me/profile", Value::Null, Some(&other)).await;
    assert_eq!(v["display_name"], "");
}

#[sqlx::test]
async fn payout_account_is_encrypted_masked_and_owner_only(pool: PgPool) {
    key();
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let other = user(&app).await;
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/payout-account"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["registered"], false);
    let bad = json!({"payee_type":"INDIVIDUAL","holder_name":"서린","bank_name":"국민은행","account_number":"12-34"});
    let (s, _, v) = call(&app, "PUT", &org_path(&u, "/payout-account"), bad, Some(&u)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "ACCOUNT_NUMBER_INVALID");
    let good = json!({"payee_type":"INDIVIDUAL","holder_name":"서린","bank_name":"국민은행","account_number":"123456-78-901234"});
    let (s, _, v) = call(
        &app,
        "PUT",
        &org_path(&u, "/payout-account"),
        good.clone(),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/payout-account"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["account_last4"], "1234");
    assert!(v.get("account_number").is_none() && v.get("account_cipher").is_none());
    let sealed: Vec<u8> =
        sqlx::query_scalar("SELECT account_cipher FROM portal.payout_accounts WHERE org_id=$1")
            .bind(u.org)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!sealed.windows(6).any(|w| w == b"901234"));
    assert_eq!(
        audeniq_core::portal::open_account(u.org, &sealed).unwrap(),
        "12345678901234"
    );
    // Another account cannot read or write this organisation.
    let (s, _, _) = call(
        &app,
        "GET",
        &org_path(&u, "/payout-account"),
        Value::Null,
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = call(
        &app,
        "PUT",
        &org_path(&u, "/payout-account"),
        good,
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn inquiries_threads_staff_replies_and_notifications(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let body = json!({"category":"RELEASE","release_id":release,"subject":"발매일 문의","body":"언제 공개되나요?\n감사합니다."});
    let (s, _, v) = call(&app, "POST", &org_path(&u, "/inquiries"), body, Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/inquiries"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["items"][0]["release_title"], "Draft");
    assert_eq!(v["items"][0]["status"], "OPEN");
    // Operations answers; the thread becomes ANSWERED and the author is notified.
    sqlx::query("SELECT portal.staff_reply($1::uuid,'다음 주 금요일에 공개돼요.')")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, &format!("/inquiries/{id}")),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["status"], "ANSWERED");
    assert_eq!(v["messages"].as_array().unwrap().len(), 2);
    assert_eq!(v["messages"][1]["author_kind"], "STAFF");
    let (_, _, n) = call(
        &app,
        "GET",
        &org_path(&u, "/notifications"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert!(
        n["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["kind"] == "INQUIRY")
    );
    // Artist follow-up reopens; closed threads refuse new messages.
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/inquiries/{id}/messages")),
        json!({"body":"확인했어요."}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, &format!("/inquiries/{id}")),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["status"], "OPEN");
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/inquiries/{id}/close")),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/inquiries/{id}/messages")),
        json!({"body":"하나 더"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    // Other organisations cannot read the thread or attach their release.
    let other = user(&app).await;
    let (s, _, _) = call(
        &app,
        "GET",
        &org_path(&u, &format!("/inquiries/{id}")),
        Value::Null,
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let foreign = json!({"category":"RELEASE","release_id":release,"subject":"x","body":"y"});
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&other, "/inquiries"),
        foreign,
        Some(&other),
    )
    .await;
    assert_ne!(s, StatusCode::OK);
}

#[sqlx::test]
async fn release_status_raises_notifications_and_reads_are_per_user(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    sqlx::query(
        "UPDATE catalog.releases SET status='SUBMITTED',row_version=row_version+1 WHERE id=$1",
    )
    .bind(release)
    .execute(&pool)
    .await
    .unwrap();
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/notifications"),
        Value::Null,
        Some(&u),
    )
    .await;
    let items = v["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|x| x["kind"] == "RELEASE" && x["link"] == format!("/releases/{release}")),
        "{v}"
    );
    assert!(v["unread"].as_u64().unwrap() >= 1);
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&u, "/notifications/read"),
        json!({"all":true}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/notifications"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["unread"], 0);
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&u, "/notifications/read"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let other = user(&app).await;
    let (s, _, _) = call(
        &app,
        "GET",
        &org_path(&u, "/notifications"),
        Value::Null,
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn signed_application_creates_agreement_that_signs_after_review(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let hash = "ab".repeat(32);
    let app_body = |no: &str, hash: &str| {
        json!({
            "application_no":no,"form":"AUD-DIST-APP 1.0","content_hash":hash,"signer_name":"서린","signer_role":"아티스트 본인",
            "agreements":["truth","terms","privacy","esign"],"signature":SIG,"submitted_at":"2026-09-26 09:00"
        })
    };
    let path = org_path(&u, &format!("/releases/{release}/application"));
    let (s, _, v) = call(
        &app,
        "POST",
        &path,
        app_body("AQ-20260926-ABCDEF", &hash),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _, v) = call(
        &app,
        "POST",
        &path,
        app_body("AUD-20260926-ABCDEF", &hash),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // Retry is idempotent; the same number with other content is a conflict.
    let (s, _, _) = call(
        &app,
        "POST",
        &path,
        app_body("AUD-20260926-ABCDEF", &hash),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = call(
        &app,
        "POST",
        &path,
        app_body("AUD-20260926-ABCDEF", &"cd".repeat(32)),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, _, v) = call(&app, "GET", &path, Value::Null, Some(&u)).await;
    assert_eq!(v["application_no"], "AUD-20260926-ABCDEF");
    assert_eq!(v["content_hash"], hash);

    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/documents"),
        Value::Null,
        Some(&u),
    )
    .await;
    let doc = &v["items"][0];
    assert_eq!(doc["kind"], "AGREEMENT");
    assert_eq!(doc["status"], "REVIEW");
    assert!(
        doc["body"]
            .as_str()
            .unwrap()
            .contains("AUD-20260926-ABCDEF")
    );
    let id = doc["id"].as_str().unwrap().to_string();
    let sign = |rv: i64| json!({"signer_name":"서린","signature":SIG,"row_version":rv});
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/sign")),
        sign(0),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["error"]["code"], "DOCUMENT_NOT_APPROVED");
    // Staff approval notifies; signing still needs the read confirmation.
    sqlx::query("UPDATE portal.documents SET status='APPROVED' WHERE id=$1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, _, n) = call(
        &app,
        "GET",
        &org_path(&u, "/notifications"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert!(
        n["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["kind"] == "DOCUMENT")
    );
    let (_, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/sign")),
        sign(0),
        Some(&u),
    )
    .await;
    assert_eq!(v["error"]["code"], "DOCUMENT_NOT_CHECKED");
    let (_, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/check")),
        json!({}),
        Some(&u),
    )
    .await;
    let rv = v["row_version"].as_i64().unwrap();
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/sign")),
        sign(rv - 1),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let bad_sig = json!({"signer_name":"서린","signature":"data:image/svg+xml;base64,PHN2Zz4=","row_version":rv});
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/sign")),
        bad_sig,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/sign")),
        sign(rv),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "SIGNED");
    // Other orgs cannot see the application.
    let other = user(&app).await;
    let (s, _, _) = call(&app, "GET", &path, Value::Null, Some(&other)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Artist-added rights proof without a file waits for documents (and reminds).
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, "/documents"),
        json!({"release_id":release,"title":"샘플 이용 허락서","body":"원곡: A"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/documents"),
        Value::Null,
        Some(&u),
    )
    .await;
    let proof = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "RIGHTS_PROOF")
        .unwrap()
        .clone();
    assert_eq!(proof["status"], "AWAITING_DOCUMENTS");
    // A foreign asset id cannot be attached.
    let bad =
        json!({"asset_id":Uuid::new_v4(),"file_name":"a.pdf","row_version":proof["row_version"]});
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(
            &u,
            &format!("/documents/{}/proof", proof["id"].as_str().unwrap()),
        ),
        bad,
        Some(&u),
    )
    .await;
    assert_ne!(s, StatusCode::OK);
    // Another org cannot add documents to this release.
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&other, "/documents"),
        json!({"release_id":release,"title":"x"}),
        Some(&other),
    )
    .await;
    assert_ne!(s, StatusCode::OK);
}

#[sqlx::test]
async fn settlement_balance_payout_requests_and_reports(pool: PgPool) {
    key();
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/finance/summary"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["payable"], "0");
    use audeniq_core::finance::{self, EntrySide, LedgerEntryInput, PostTransaction};
    let entry = |account, side, amount: &str| LedgerEntryInput {
        account,
        side,
        amount: amount.parse().unwrap(),
        currency: None,
        party_id: Some(u.party),
        isrc: None,
        split_snapshot_id: None,
    };
    finance::post_transaction(
        &pool,
        u.org,
        PostTransaction {
            transaction_code: "ROYALTY_ACCRUAL",
            currency: "KRW",
            entries: vec![
                entry("ROYALTY_RECEIVABLE", EntrySide::Debit, "50000"),
                entry("ROYALTY_PAYABLE", EntrySide::Credit, "50000"),
            ],
            match_status: Some("AUTO"),
            fx_rate: None,
            fee_policy_version: Some("fee-1"),
            contract_version: Some("ctr-1"),
            tax_rule_version: Some("tax-1"),
            source_ref: json!({"dsp":"spotify","period":"2026-08"}),
            description: "2026-08 Spotify",
            created_by: None,
        },
    )
    .await
    .unwrap();
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/finance/summary"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["payable"], "50000");
    assert_eq!(v["available"], "50000");
    assert_eq!(v["account_registered"], false);
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/finance/statements"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["items"][0]["amount"], "50000");
    assert_eq!(v["items"][0]["description"], "2026-08 Spotify");

    let req = |amount: &str, key: &str| json!({"amount":amount,"idempotency_key":key});
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, "/finance/payouts"),
        req("30000", "payout-key-1"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["error"]["code"], "PAYOUT_ACCOUNT_REQUIRED");
    let acct = json!({"payee_type":"INDIVIDUAL","holder_name":"서린","bank_name":"국민은행","account_number":"12345678901234"});
    call(
        &app,
        "PUT",
        &org_path(&u, "/payout-account"),
        acct,
        Some(&u),
    )
    .await;
    let (_, _, v) = call(
        &app,
        "POST",
        &org_path(&u, "/finance/payouts"),
        req("5000", "payout-key-0"),
        Some(&u),
    )
    .await;
    assert_eq!(v["error"]["code"], "PAYOUT_BELOW_MINIMUM");
    let (_, _, v) = call(
        &app,
        "POST",
        &org_path(&u, "/finance/payouts"),
        req("60000", "payout-key-1"),
        Some(&u),
    )
    .await;
    assert_eq!(v["error"]["code"], "PAYOUT_EXCEEDS_BALANCE");
    let (s, _, first) = call(
        &app,
        "POST",
        &org_path(&u, "/finance/payouts"),
        req("30000", "payout-key-1"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{first}");
    let (_, _, again) = call(
        &app,
        "POST",
        &org_path(&u, "/finance/payouts"),
        req("30000", "payout-key-1"),
        Some(&u),
    )
    .await;
    assert_eq!(first["id"], again["id"]);
    // The pending request is held back from the available balance.
    let (_, _, v) = call(
        &app,
        "POST",
        &org_path(&u, "/finance/payouts"),
        req("30000", "payout-key-2"),
        Some(&u),
    )
    .await;
    assert_eq!(v["error"]["code"], "PAYOUT_EXCEEDS_BALANCE");
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/finance/summary"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["pending"], "30000");
    assert_eq!(v["available"], "20000");
    let (_, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/finance/payouts"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(v["items"].as_array().unwrap().len(), 1);
    assert_eq!(v["items"][0]["status"], "REQUESTED");
    let (s, _, v) = call(
        &app,
        "GET",
        &org_path(&u, "/reports"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["by_month"].as_array().unwrap().is_empty());
    assert!(v["rows"].as_array().unwrap().is_empty());
    let other = user(&app).await;
    let (s, _, _) = call(
        &app,
        "GET",
        &org_path(&u, "/finance/summary"),
        Value::Null,
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn portal_writes_need_csrf_and_document_uploads_accept_pdf(pool: PgPool) {
    let (app, _) = app(pool).await;
    let mut u = user(&app).await;
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, "/uploads"),
        json!({"kind":"DOCUMENT","size_bytes":1000,"content_type":"application/pdf"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, _, v) = call(
        &app,
        "POST",
        &org_path(&u, "/uploads"),
        json!({"kind":"DOCUMENT","size_bytes":1000,"content_type":"application/zip"}),
        Some(&u),
    )
    .await;
    assert_eq!(v["error"]["code"], "UPLOAD_TYPE_UNSUPPORTED");
    u.csrf = "wrong".into();
    let (s, _, _) = call(
        &app,
        "PUT",
        "/api/me/profile",
        json!({"display_name":"x","contact_email":"","bio":"","country":"KR","row_version":0}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}
