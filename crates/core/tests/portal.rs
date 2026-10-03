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
            test_only_bypass_dsp_gate: true,
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
async fn legacy_direct_electronic_rights_are_refused(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let input = json!({"release_id": release, "title": "Cover permission", "body": "Original / digital distribution / Writer",
        "asset_id": null, "file_name": "", "electronic": {"document_no": Uuid::new_v4(), "form": "AUD-RIGHTS 1.0",
        "document_kind": "composition", "rights_holder": "Writer", "signer_name": "Writer", "signer_role": "권리자 본인", "signature": SIG, "consent": true}});
    let (status, _, body) = call(&app, "POST", &org_path(&u, "/documents"), input, Some(&u)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "SIGNING_REQUEST_REQUIRED");
}

fn signing_input(release: Uuid, receipt: Uuid, channel: &str) -> Value {
    json!({"release_id": release, "document_no": receipt, "document_kind": "composition",
        "title": "Draft · 작사·작곡 및 커버곡 이용 허락서", "body": "허락서 본문\n1. 이용 허락",
        "rights_holder": "홍길동", "signer_name": "홍길동", "signer_role": "권리자 본인", "channel": channel})
}

#[sqlx::test]
async fn rights_holder_signs_after_identity_verification(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let receipt = Uuid::new_v4();
    let path = org_path(&u, "/signing-requests");
    let (s, _, created) = call(
        &app,
        "POST",
        &path,
        signing_input(release, receipt, "LINK"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{created}");
    let token = created["token"].as_str().unwrap().to_string();
    assert_eq!(token.len(), 64);
    // Same receipt + same text: the open request gets a fresh link, the old one stops.
    let (s, _, again) = call(
        &app,
        "POST",
        &path,
        signing_input(release, receipt, "LINK"),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{again}");
    assert_eq!(again["id"], created["id"]);
    let old = token;
    let token = again["token"].as_str().unwrap().to_string();
    assert_eq!(
        call(&app, "GET", &format!("/api/sign/{old}"), Value::Null, None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let mut changed = signing_input(release, receipt, "LINK");
    changed["body"] = json!("다른 본문");
    assert_eq!(
        call(&app, "POST", &path, changed, Some(&u)).await.0,
        StatusCode::CONFLICT
    );
    // Another org cannot open requests on this release.
    let other = user(&app).await;
    assert_eq!(
        call(
            &app,
            "POST",
            &org_path(&other, "/signing-requests"),
            signing_input(release, Uuid::new_v4(), "LINK"),
            Some(&other)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );

    // The signer reads it without an account.
    let sign = format!("/api/sign/{token}");
    let (s, _, view) = call(&app, "GET", &sign, Value::Null, None).await;
    assert_eq!(s, StatusCode::OK, "{view}");
    assert_eq!(view["status"], "PENDING");
    assert_eq!(view["signer_name"], "홍길동");
    assert!(view["signature"].is_null());

    // No provider configured: nobody can verify, so nobody can sign.
    // SAFETY: the provider variable is only read by the signing endpoints of this test.
    unsafe { std::env::remove_var("IDENTITY_PROVIDER") };
    let (s, _, e) = call(
        &app,
        "POST",
        &format!("{sign}/identity"),
        json!({"transaction_id": "test:홍길동:1990-01-01"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(e["error"]["code"], "IDENTITY_PROVIDER_NOT_CONFIGURED");
    let consents = json!({"document": true, "electronic_signature": true, "privacy": true});
    let (_, _, e) = call(
        &app,
        "POST",
        &format!("{sign}/sign"),
        json!({"signature": SIG, "consents": consents}),
        None,
    )
    .await;
    assert_eq!(e["error"]["code"], "IDENTITY_NOT_VERIFIED");

    unsafe { std::env::set_var("IDENTITY_PROVIDER", "test") };
    // A different person cannot verify as the named signer.
    let (_, _, e) = call(
        &app,
        "POST",
        &format!("{sign}/identity"),
        json!({"transaction_id": "test:김철수:1990-01-01"}),
        None,
    )
    .await;
    assert_eq!(e["error"]["code"], "IDENTITY_NAME_MISMATCH");
    let (s, _, v) = call(
        &app,
        "POST",
        &format!("{sign}/identity"),
        json!({"transaction_id": "test:홍 길동:1990-01-01"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "VERIFIED");
    // All three consents are required.
    let (_, _, e) = call(&app, "POST", &format!("{sign}/sign"),
        json!({"signature": SIG, "consents": {"document": true, "electronic_signature": true, "privacy": false}}), None).await;
    assert_eq!(e["error"]["code"], "SIGNING_CONSENT_REQUIRED");
    let (s, _, signed) = call(
        &app,
        "POST",
        &format!("{sign}/sign"),
        json!({"signature": SIG, "consents": consents}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{signed}");
    assert_eq!(signed["status"], "SIGNED");
    // Signing twice is refused; the signed copy stays readable with its evidence.
    let (_, _, e) = call(
        &app,
        "POST",
        &format!("{sign}/sign"),
        json!({"signature": SIG, "consents": consents}),
        None,
    )
    .await;
    assert_eq!(e["error"]["code"], "SIGNING_REQUEST_CLOSED");
    let (_, _, copy) = call(&app, "GET", &sign, Value::Null, None).await;
    assert_eq!(copy["status"], "SIGNED");
    assert_eq!(copy["signature"], SIG);
    assert_eq!(copy["certificate_hash"], signed["certificate_hash"]);
    let events: Vec<&str> = copy["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["event"].as_str().unwrap())
        .collect();
    assert_eq!(
        events,
        [
            "CREATED",
            "LINK_REISSUED",
            "VIEWED",
            "IDENTITY_FAILED",
            "IDENTITY_VERIFIED",
            "SIGNED"
        ]
    );

    // The document waits for AUDENIQ review, linked to the verified signer.
    let (_, _, listed) = call(
        &app,
        "GET",
        &org_path(&u, "/documents"),
        Value::Null,
        Some(&u),
    )
    .await;
    let doc = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == signed["document_id"])
        .unwrap();
    assert_eq!(doc["status"], "REVIEW");
    assert_eq!(doc["signature"], SIG);
    let record = &doc["electronic_record"];
    assert_eq!(record["form"], "AUD-RIGHTS 2.0");
    assert_eq!(record["certificate_hash"], signed["certificate_hash"]);
    assert_eq!(record["identity"]["name"], "홍 길동");
    assert!(
        record["identity"]["birth_date"].is_null(),
        "birth date stays out of the document"
    );
    let (_, _, requests) = call(&app, "GET", &path, Value::Null, Some(&u)).await;
    assert_eq!(requests["items"][0]["status"], "SIGNED");
    assert!(requests["items"][0]["identity"]["verified_at"].is_string());

    // Evidence rows cannot be rewritten.
    let edited = sqlx::query("UPDATE portal.signing_events SET event='VIEWED'")
        .execute(&pool)
        .await;
    assert!(edited.is_err());

    // Decline and cancel close a request.
    let (_, _, second) = call(
        &app,
        "POST",
        &path,
        signing_input(release, Uuid::new_v4(), "IN_PERSON"),
        Some(&u),
    )
    .await;
    let t2 = second["token"].as_str().unwrap();
    let (s, _, d) = call(
        &app,
        "POST",
        &format!("/api/sign/{t2}/decline"),
        json!({"reason": "지분 비율이 달라요"}),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    let (_, _, e) = call(
        &app,
        "POST",
        &format!("/api/sign/{t2}/identity"),
        json!({"transaction_id": "test:홍길동:1990-01-01"}),
        None,
    )
    .await;
    assert_eq!(e["error"]["code"], "SIGNING_REQUEST_CLOSED");
    let (_, _, third) = call(
        &app,
        "POST",
        &path,
        signing_input(release, Uuid::new_v4(), "LINK"),
        Some(&u),
    )
    .await;
    let id3 = third["id"].as_str().unwrap();
    let (s, _, c) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/signing-requests/{id3}/cancel")),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{c}");
    let t3 = third["token"].as_str().unwrap();
    let (_, _, v3) = call(&app, "GET", &format!("/api/sign/{t3}"), Value::Null, None).await;
    assert_eq!(v3["status"], "CANCELLED");
    unsafe { std::env::remove_var("IDENTITY_PROVIDER") };
}

#[sqlx::test]
async fn content_access_requires_active_admin_session_and_csrf_for_writes(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let mut u = user(&app).await;
    let path = "/api/staff/content-access";
    assert_eq!(
        call(&app, "GET", path, Value::Null, Some(&u)).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,status,granted_by) VALUES($1,'ADMIN','ACTIVE','test')")
        .bind(u.user).execute(&pool).await.unwrap();
    assert_eq!(
        call(&app, "GET", path, Value::Null, Some(&u)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", path, json!({}), Some(&u)).await.0,
        StatusCode::OK
    );
    u.csrf = "wrong".into();
    assert_eq!(
        call(&app, "POST", path, json!({}), Some(&u)).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE identity.staff_members SET role='SUPPORT' WHERE user_id=$1")
        .bind(u.user)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", path, Value::Null, Some(&u)).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE identity.staff_members SET role='ADMIN',status='REVOKED',revoked_at=now() WHERE user_id=$1").bind(u.user).execute(&pool).await.unwrap();
    assert_eq!(
        call(&app, "GET", path, Value::Null, Some(&u)).await.0,
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test]
async fn release_documents_and_inquiries_enforce_active_resource_acl(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let owner = user(&app).await;
    let editor = user(&app).await;
    let release = create(&app, &owner, "releases").await;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role) VALUES($1,$2,'EDITOR')")
        .bind(owner.org)
        .bind(editor.user)
        .execute(&pool)
        .await
        .unwrap();
    let doc = Uuid::new_v4();
    let proof = Uuid::new_v4();
    let global_doc = Uuid::new_v4();
    for (id, linked, kind) in [
        (doc, Some(release), "AGREEMENT"),
        (proof, Some(release), "RIGHTS_PROOF"),
        (global_doc, None, "RIGHTS_PROOF"),
    ] {
        sqlx::query("INSERT INTO portal.documents(id,org_id,release_id,kind,title,status,checked_at) VALUES($1,$2,$3,$4,'Private document','APPROVED',now())")
            .bind(id).bind(owner.org).bind(linked).bind(kind).execute(&pool).await.unwrap();
    }
    let thread = Uuid::new_v4();
    let global_thread = Uuid::new_v4();
    for (id, linked) in [(thread, Some(release)), (global_thread, None)] {
        sqlx::query("INSERT INTO portal.inquiries(id,org_id,created_by,category,release_id,subject) VALUES($1,$2,$3,'RELEASE',$4,'Private inquiry')")
            .bind(id).bind(owner.org).bind(owner.user).bind(linked).execute(&pool).await.unwrap();
    }
    for state in ["absent", "revoked", "expired", "future"] {
        sqlx::query("DELETE FROM identity.resource_acl WHERE org_id=$1 AND resource_id=$2 AND principal_party_id=$3")
            .bind(owner.org).bind(release).bind(editor.party).execute(&pool).await.unwrap();
        if state != "absent" {
            sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action,starts_at,ends_at,revoked_at)
                SELECT $1,$2,$3,action,CASE WHEN $4='future' THEN now()+interval '1 day' ELSE now()-interval '2 days' END,
                    CASE WHEN $4='expired' THEN now()-interval '1 day' ELSE NULL END,
                    CASE WHEN $4='revoked' THEN now() ELSE NULL END FROM unnest(ARRAY['read','write']) action")
                .bind(owner.org).bind(release).bind(editor.party).bind(state).execute(&pool).await.unwrap();
        }
        for (path, global) in [("/documents", global_doc), ("/inquiries", global_thread)] {
            let (s, _, v) = call(
                &app,
                "GET",
                &org_path(&owner, path),
                Value::Null,
                Some(&editor),
            )
            .await;
            assert_eq!(s, StatusCode::OK, "{state}: {v}");
            assert_eq!(v["items"].as_array().unwrap().len(), 1, "{state}: {v}");
            assert_eq!(v["items"][0]["id"], global.to_string());
        }
        for (method, path, body) in [
            ("GET", format!("/inquiries/{thread}"), Value::Null),
            (
                "POST",
                format!("/inquiries/{thread}/messages"),
                json!({"body":"unauthorized"}),
            ),
            ("POST", format!("/inquiries/{thread}/close"), json!({})),
            ("POST", format!("/documents/{doc}/check"), json!({})),
            (
                "POST",
                format!("/documents/{doc}/sign"),
                json!({"signer_name":"Editor","signature":SIG,"row_version":0}),
            ),
            (
                "POST",
                format!("/documents/{proof}/proof"),
                json!({"asset_id":Uuid::new_v4(),"file_name":"proof.pdf","row_version":0}),
            ),
        ] {
            let (s, _, v) = call(&app, method, &org_path(&owner, &path), body, Some(&editor)).await;
            assert_eq!(s, StatusCode::FORBIDDEN, "{state} {path}: {v}");
        }
    }
    // Null-linked records retain their organization-wide behavior.
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&owner, &format!("/documents/{global_doc}/check")),
        json!({}),
        Some(&editor),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    sqlx::query("DELETE FROM identity.resource_acl WHERE resource_id=$1 AND principal_party_id=$2")
        .bind(release)
        .bind(editor.party)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action) VALUES($1,$2,$3,'read')")
        .bind(owner.org).bind(release).bind(editor.party).execute(&pool).await.unwrap();
    // Inquiry write operations deliberately require release read, as creation does.
    for (path, body) in [
        (
            format!("/inquiries/{thread}/messages"),
            json!({"body":"authorized reader"}),
        ),
        (format!("/inquiries/{thread}/close"), json!({})),
    ] {
        let (s, _, v) = call(&app, "POST", &org_path(&owner, &path), body, Some(&editor)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&owner, &format!("/documents/{doc}/check")),
        json!({}),
        Some(&editor),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE identity.resource_acl SET action='write' WHERE resource_id=$1 AND principal_party_id=$2")
        .bind(release).bind(editor.party).execute(&pool).await.unwrap();
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&owner, &format!("/documents/{doc}/check")),
        json!({}),
        Some(&editor),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let rv = v["row_version"].as_i64().unwrap();
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&owner, &format!("/documents/{doc}/sign")),
        json!({"signer_name":"Editor","signature":SIG,"row_version":rv}),
        Some(&editor),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // A write grant does not imply read, and VIEWER remains unable to write.
    let (s, _, _) = call(
        &app,
        "GET",
        &org_path(&owner, &format!("/inquiries/{thread}")),
        Value::Null,
        Some(&editor),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    sqlx::query("UPDATE identity.memberships SET role='VIEWER' WHERE org_id=$1 AND user_id=$2")
        .bind(owner.org)
        .bind(editor.user)
        .execute(&pool)
        .await
        .unwrap();
    let (s, _, _) = call(
        &app,
        "POST",
        &org_path(&owner, &format!("/documents/{doc}/check")),
        json!({}),
        Some(&editor),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn replacement_applications_preserve_terminal_agreements_and_require_reread(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let path = org_path(&u, &format!("/releases/{release}/application"));
    let body = |suffix: &str| json!({"application_no":format!("AUD-20260930-{suffix}"),"form":"AUD-DIST-APP 1.0","content_hash":"ab".repeat(32),"signer_name":"Artist","signer_role":"Owner","agreements":["truth","terms"],"signature":SIG,"submitted_at":"2026-09-30 10:00"});
    let (s, _, v) = call(&app, "POST", &path, body("AAAAAA"), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    sqlx::query("UPDATE portal.documents SET status='APPROVED',checked_at=now(),review_note='Old review' WHERE release_id=$1")
        .bind(release).execute(&pool).await.unwrap();
    let (s, _, v) = call(&app, "POST", &path, body("BBBBBB"), Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (status, checked, note): (String, bool, String) = sqlx::query_as("SELECT status,checked_at IS NOT NULL,review_note FROM portal.documents WHERE release_id=$1")
        .bind(release).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "REVIEW");
    assert!(!checked);
    assert!(note.is_empty());
    for terminal in ["SIGNED", "REJECTED", "CANCELLED"] {
        sqlx::query("UPDATE portal.documents SET status=$2,signature=$3,signed_by=$4,signed_at=now(),signer_name='Original signer' WHERE release_id=$1")
            .bind(release).bind(terminal).bind(SIG).bind(u.user).execute(&pool).await.unwrap();
        let before: Value =
            sqlx::query_scalar("SELECT to_jsonb(d) FROM portal.documents d WHERE release_id=$1")
                .bind(release)
                .fetch_one(&pool)
                .await
                .unwrap();
        let (s, _, v) = call(&app, "POST", &path, body("CCCCCC"), Some(&u)).await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{terminal}: {v}");
        let after: Value =
            sqlx::query_scalar("SELECT to_jsonb(d) FROM portal.documents d WHERE release_id=$1")
                .bind(release)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(before, after);
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM portal.release_applications WHERE release_id=$1",
        )
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            call(&app, "POST", &path, body("BBBBBB"), Some(&u)).await.0,
            StatusCode::OK
        );
    }
    let mut changed_retry = body("BBBBBB");
    changed_retry["signer_name"] = json!("Different signer");
    assert_eq!(
        call(&app, "POST", &path, changed_retry, Some(&u)).await.0,
        StatusCode::CONFLICT
    );
}

#[sqlx::test]
async fn concurrent_signing_and_replacement_never_bind_an_old_signature_to_new_content(
    pool: PgPool,
) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let path = org_path(&u, &format!("/releases/{release}/application"));
    let body = |suffix: &str| json!({"application_no":format!("AUD-20260930-{suffix}"),"form":"AUD-DIST-APP 1.0","content_hash":"ab".repeat(32),"signer_name":"Artist","signer_role":"Owner","agreements":["truth"],"signature":SIG,"submitted_at":"2026-09-30 10:00"});
    assert_eq!(
        call(&app, "POST", &path, body("AAAAAA"), Some(&u)).await.0,
        StatusCode::OK
    );
    let (id, old_body): (Uuid, String) =
        sqlx::query_as("SELECT id,body FROM portal.documents WHERE release_id=$1")
            .bind(release)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("UPDATE portal.documents SET status='APPROVED',checked_at=now() WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let sign_path = org_path(&u, &format!("/documents/{id}/sign"));
    let (replacement, signing) = tokio::join!(
        call(&app, "POST", &path, body("BBBBBB"), Some(&u)),
        call(
            &app,
            "POST",
            &sign_path,
            json!({"signer_name":"Artist","signature":SIG,"row_version":0}),
            Some(&u)
        ),
    );
    let (status, content, sig): (String, String, String) =
        sqlx::query_as("SELECT status,body,signature FROM portal.documents WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    if status == "SIGNED" {
        assert_eq!(signing.0, StatusCode::OK);
        assert_eq!(replacement.0, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(content, old_body);
        assert_eq!(sig, SIG);
    } else {
        assert_eq!(status, "REVIEW");
        assert_eq!(replacement.0, StatusCode::OK);
        assert_ne!(signing.0, StatusCode::OK);
        assert!(content.contains("AUD-20260930-BBBBBB"));
        assert!(sig.is_empty());
    }
}

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
async fn staff_sheet_shows_the_signed_application_with_signature_and_contact(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let path = org_path(&u, &format!("/releases/{release}/application"));
    let (s, _, v) = call(
        &app,
        "POST",
        &path,
        json!({
            "application_no":"AUD-20260930-QWERTY","form":"AUD-DIST-APP 1.0","content_hash":"ef".repeat(32),
            "signer_name":"서린","signer_role":"아티스트 본인","agreements":["truth","terms","privacy","esign"],
            "signature":SIG,"submitted_at":"2026-09-30 09:00"
        }),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // No profile yet: the account email is the contact.
    let staff = user(&app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'REVIEWER','test-operator')")
        .bind(staff.user)
        .execute(&pool)
        .await
        .unwrap();
    let sheet = format!("/api/staff/releases/{release}");
    let (s, _, v) = call(&app, "GET", &sheet, Value::Null, Some(&staff)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let a = &v["signed_application"];
    assert_eq!(a["application_no"], "AUD-20260930-QWERTY");
    assert_eq!(a["signature"], SIG);
    assert_eq!(
        a["agreements"],
        json!(["truth", "terms", "privacy", "esign"])
    );
    let account: String = sqlx::query_scalar("SELECT email FROM identity.users WHERE id=$1")
        .bind(u.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(a["contact_email"], account);
    // With a profile contact email, that one is shown.
    sqlx::query("INSERT INTO portal.artist_profiles(user_id,display_name,contact_email) VALUES($1,'서린','contact@example.test')")
        .bind(u.user)
        .execute(&pool)
        .await
        .unwrap();
    let (_, _, v) = call(&app, "GET", &sheet, Value::Null, Some(&staff)).await;
    assert_eq!(
        v["signed_application"]["contact_email"],
        "contact@example.test"
    );
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

#[sqlx::test]
async fn distribution_agreement_needs_staff_terms_and_artist_confirmations(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    sqlx::query("UPDATE catalog.releases SET draft = draft || $2, row_version = row_version + 1 WHERE id=$1")
        .bind(release)
        .bind(json!({"artist": "서린", "platforms": ["spotify"], "territories": ["WORLD"], "options": {"shared": true}}))
        .execute(&pool)
        .await
        .unwrap();
    let app_body = json!({
        "application_no":"AUD-20261003-ABCDEF","form":"AUD-DIST-APP 1.0","content_hash":"ab".repeat(32),"signer_name":"서린",
        "signer_role":"아티스트 본인","agreements":["truth","terms","privacy","esign"],"signature":SIG,"submitted_at":"2026-10-03 09:00"
    });
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/releases/{release}/application")),
        app_body,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    // Only staff with the review duty set the commercial terms.
    let terms_path = format!("/api/staff/releases/{release}/agreement-terms");
    let terms = json!({"exclusivity":"NON_EXCLUSIVE","fee_bps":800,"rate_note":"프로모션 요율","territory_note":"","min_payout_note":"","special_terms":""});
    assert_eq!(
        call(&app, "PUT", &terms_path, terms.clone(), Some(&u))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let staff = user(&app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,status,granted_by) VALUES($1,'REVIEWER','ACTIVE','test')")
        .bind(staff.user)
        .execute(&pool)
        .await
        .unwrap();
    let mut bad = terms.clone();
    bad["fee_bps"] = json!(10_001);
    assert_eq!(
        call(&app, "PUT", &terms_path, bad, Some(&staff)).await.0,
        StatusCode::BAD_REQUEST
    );
    let (s, _, set) = call(&app, "PUT", &terms_path, terms, Some(&staff)).await;
    assert_eq!(s, StatusCode::OK, "{set}");
    let body = set["body"].as_str().unwrap();
    assert!(
        body.contains("서식 AUD-DIST 2.0")
            && body.contains("회사 8% / 이용자 92%")
            && body.contains("Spotify"),
        "{body}"
    );
    assert!(
        body.contains("(대표권리자)"),
        "co-owned release gets the representative article"
    );
    let required: Vec<String> = set["terms"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert!(required.contains(&"representative".to_string()));
    assert!(!required.contains(&"settlement_authority".to_string()));

    // Staff approve (the decision path is covered elsewhere), the artist reads.
    let (_, _, docs) = call(
        &app,
        "GET",
        &org_path(&u, "/documents"),
        Value::Null,
        Some(&u),
    )
    .await;
    let doc = docs["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "AGREEMENT")
        .unwrap()
        .clone();
    assert_eq!(doc["agreement_terms"]["fee_bps"], 800);
    let id = doc["id"].as_str().unwrap().to_string();
    sqlx::query("UPDATE portal.documents SET status='APPROVED' WHERE id=$1::uuid")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/check")),
        json!({}),
        Some(&u),
    )
    .await;
    let rv = v["row_version"].as_i64().unwrap();
    let sign_path = org_path(&u, &format!("/documents/{id}/sign"));
    // Without every required confirmation there is no signature.
    let (s, _, v) = call(&app, "POST", &sign_path, json!({"signer_name":"서린","signature":SIG,"row_version":rv,"confirmations":required[1..]}), Some(&u)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["error"]["code"], "AGREEMENT_CONFIRMATION_REQUIRED");
    let (s, _, v) = call(
        &app,
        "POST",
        &sign_path,
        json!({"signer_name":"서린","signature":SIG,"row_version":rv,"confirmations":["ai_voice"]}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    let (s, _, v) = call(
        &app,
        "POST",
        &sign_path,
        json!({"signer_name":"서린","signature":SIG,"row_version":rv,"confirmations":required}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, _, docs) = call(
        &app,
        "GET",
        &org_path(&u, "/documents"),
        Value::Null,
        Some(&u),
    )
    .await;
    let doc = docs["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == id.as_str())
        .unwrap();
    assert_eq!(doc["status"], "SIGNED");
    let items = doc["confirmations"]["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .find(|i| i["id"] == "settlement_authority")
            .unwrap()["checked"]
            == false
    );
    assert!(doc["confirmations"]["content_hash"].as_str().unwrap().len() == 64);
    // A signed agreement's terms can no longer be changed.
    let (_, _, v) = call(
        &app,
        "PUT",
        &terms_path,
        json!({"exclusivity":"EXCLUSIVE","fee_bps":2000}),
        Some(&staff),
    )
    .await;
    assert_eq!(v["error"]["code"], "AGREEMENT_NOT_IN_REVIEW");
}

#[sqlx::test]
async fn documents_of_deleted_releases_are_not_listed(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let app_body = json!({
        "application_no":"AUD-20261003-QWERTY","form":"AUD-DIST-APP 1.0","content_hash":"ab".repeat(32),"signer_name":"서린",
        "signer_role":"아티스트 본인","agreements":["truth","terms","privacy","esign"],"signature":SIG,"submitted_at":"2026-10-03 09:00"
    });
    let (s, _, v) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/releases/{release}/application")),
        app_body,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let count = |v: &Value| {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["release_id"] == release.to_string())
            .count()
    };
    let (_, _, listed) = call(
        &app,
        "GET",
        &org_path(&u, "/documents"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(count(&listed), 1);
    sqlx::query(
        "UPDATE catalog.releases SET archived_at=now(), row_version=row_version+1 WHERE id=$1",
    )
    .bind(release)
    .execute(&pool)
    .await
    .unwrap();
    let (_, _, listed) = call(
        &app,
        "GET",
        &org_path(&u, "/documents"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(
        count(&listed),
        0,
        "a deleted release's agreement stays out of 계약서"
    );
}
