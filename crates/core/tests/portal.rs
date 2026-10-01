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
async fn electronic_rights_are_signed_without_upload_and_remain_separate_from_approval(
    pool: PgPool,
) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create(&app, &u, "releases").await;
    let receipt = Uuid::new_v4();
    let input = json!({"release_id": release, "title": "Cover permission", "body": "Original / digital distribution / Writer",
        "asset_id": null, "file_name": "", "electronic": {"document_no": receipt, "form": "AUD-RIGHTS 1.0",
        "document_kind": "composition", "rights_holder": "Writer", "signer_name": "Writer", "signer_role": "권리자 본인", "signature": SIG, "consent": true}});
    let path = org_path(&u, "/documents");
    let (a, b) = tokio::join!(
        call(&app, "POST", &path, input.clone(), Some(&u)),
        call(&app, "POST", &path, input.clone(), Some(&u))
    );
    assert_eq!(a.0, StatusCode::OK, "{}", a.2);
    assert_eq!(b.0, StatusCode::OK, "{}", b.2);
    assert_eq!(a.2["id"], b.2["id"]);
    let id = a.2["id"].as_str().unwrap();
    let (_, _, listed) = call(&app, "GET", &path, Value::Null, Some(&u)).await;
    let doc = &listed["items"][0];
    assert_eq!(doc["status"], "REVIEW");
    assert!(doc["asset_id"].is_null());
    assert_eq!(doc["signature"], SIG);
    assert!(doc["signed_at"].is_string());
    assert!(doc["checked_at"].is_string());
    let hash = audeniq_core::domain::digest(
        &json!({"title":"Cover permission","body":"Original / digital distribution / Writer",
        "document_no":receipt,"form":"AUD-RIGHTS 1.0","document_kind":"composition","rights_holder":"Writer",
        "signer_name":"Writer","signer_role":"권리자 본인","signature":SIG}),
    );
    assert_eq!(doc["electronic_record"]["content_hash"], hash);
    let mut changed = input.clone();
    changed["body"] = json!("Changed scope");
    assert_eq!(
        call(&app, "POST", &path, changed, Some(&u)).await.0,
        StatusCode::CONFLICT
    );
    let mut no_consent = input.clone();
    no_consent["electronic"]["consent"] = json!(false);
    assert_eq!(
        call(&app, "POST", &path, no_consent, Some(&u)).await.0,
        StatusCode::BAD_REQUEST
    );
    let other = user(&app).await;
    assert_eq!(
        call(
            &app,
            "POST",
            &org_path(&other, "/documents"),
            input,
            Some(&other)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, _, denied) = call(
        &app,
        "POST",
        &org_path(&u, &format!("/documents/{id}/proof")),
        json!({"asset_id":Uuid::new_v4(),"file_name":"replace.pdf","row_version":0}),
        Some(&u),
    )
    .await;
    assert_eq!(denied["error"]["code"], "DOCUMENT_NOT_EDITABLE");
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
