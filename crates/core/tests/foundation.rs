//! Real PostgreSQL integration tests. DATABASE_URL must name an isolated test server with CREATEDB.
use async_trait::async_trait;
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    error::{Error, Result},
    operations,
    storage::{ObjectMeta, ObjectStore, UploadGrant},
};
use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha2::Digest;
use sqlx::{PgPool, Row};
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
async fn noop_store() -> std::sync::Arc<dyn ObjectStore> {
    std::sync::Arc::new(MockStore::default())
}

#[derive(Clone)]
struct User {
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
#[sqlx::test]
async fn migration_constraints_and_reapplication(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    database::MIGRATOR.run(&pool).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM information_schema.schemata WHERE schema_name IN ('identity','catalog','rights','distribution','finance','operations')").fetch_one(&pool).await.unwrap(),6);
    assert!(
        sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role) VALUES($1,$2,'OWNER')")
            .bind(Uuid::new_v4())
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .is_err()
    );
}

#[sqlx::test]
async fn auth_audit_uses_server_request_id(pool: PgPool) {
    let (api, _) = app(pool.clone()).await;
    let (status, headers, _) = call(
        &api,
        "POST",
        "/api/auth/register",
        json!({"email":"request-id@example.test","password":"Long-test-password-123!"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let request_id = Uuid::parse_str(headers["x-request-id"].to_str().unwrap()).unwrap();
    let recorded: Uuid = sqlx::query_scalar(
        "SELECT request_id FROM operations.audit_events WHERE action='auth.register'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(recorded, request_id);
}
#[sqlx::test]
async fn auth_sessions_csrf_origin_and_gates(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    assert_eq!(
        call(&app, "GET", "/api/me", json!({}), Some(&u)).await.0,
        StatusCode::OK
    );
    let fail = call(
        &app,
        "POST",
        "/api/auth/login",
        json!({"email":"nobody@example.test","password":"wrong-password-long"}),
        None,
    )
    .await;
    assert_eq!(fail.0, StatusCode::UNAUTHORIZED);
    let mut forged = u.clone();
    forged.csrf = "invalid".into();
    assert_eq!(
        call(&app, "POST", "/api/auth/logout", json!({}), Some(&forged))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let bad = Request::builder()
        .method("POST")
        .uri("/api/auth/logout")
        .header("x-audeniq-service", SECRET)
        .header("origin", "https://evil.test")
        .header("cookie", &u.cookie)
        .header("x-csrf-token", &u.csrf)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(bad).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let release = create(&app, &u, "releases").await;
    // F2: submit now validates input; an empty body is rejected as invalid.
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/orgs/{}/releases/{release}/submit", u.org),
            json!({}),
            Some(&u)
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(&app, "POST", "/api/auth/logout", json!({}), Some(&u))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "GET", "/api/me", json!({}), Some(&u)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let v = user(&app).await;
    sqlx::query(
        "UPDATE identity.sessions SET expires_at=now()-interval '1 second' WHERE user_id=$1",
    )
    .bind(v.user)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        call(&app, "GET", "/api/me", json!({}), Some(&v)).await.0,
        StatusCode::UNAUTHORIZED
    );
}
#[sqlx::test]
async fn organization_acl_and_revocation(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let a = user(&app).await;
    let b = user(&app).await;
    for kind in ["artists", "labels", "releases"] {
        let id = create(&app, &a, kind).await;
        let path = format!("/api/orgs/{}/{kind}/{id}", a.org);
        assert_eq!(
            call(&app, "GET", &path, json!({}), Some(&b)).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(call(&app,"PUT",&path,json!({"name":"intrusion","row_version":0,"party_id":b.party,"release_type":"SINGLE"}),Some(&b)).await.0,StatusCode::FORBIDDEN);
    }
    let r = create(&app, &a, "releases").await;
    let (s, _, v) = call(
        &app,
        "PUT",
        &format!("/api/orgs/{}/memberships", a.org),
        json!({"user_id":b.user,"role":"EDITOR","status":"ACTIVE"}),
        Some(&a),
    )
    .await;
    // Adding a member only invites them; the invitee must accept.
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "INVITED");
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/orgs/{}/releases", a.org),
            json!({}),
            Some(&b)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/orgs/{}/memberships/accept", a.org),
            json!({}),
            Some(&b)
        )
        .await
        .0,
        StatusCode::OK
    );
    let path = format!("/api/orgs/{}/releases/{r}", a.org);
    assert_eq!(
        call(&app, "GET", &path, json!({}), Some(&b)).await.0,
        StatusCode::FORBIDDEN
    );
    call(
        &app,
        "PUT",
        &format!("/api/orgs/{}/resources/{r}/acl", a.org),
        json!({"user_id":b.user,"action":"read","revoked":false}),
        Some(&a),
    )
    .await;
    assert_eq!(
        call(&app, "GET", &path, json!({}), Some(&b)).await.0,
        StatusCode::OK
    );
    call(
        &app,
        "PUT",
        &format!("/api/orgs/{}/memberships", a.org),
        json!({"user_id":b.user,"role":"EDITOR","status":"REVOKED"}),
        Some(&a),
    )
    .await;
    assert_eq!(
        call(&app, "GET", &path, json!({}), Some(&b)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/orgs/{}/releases", a.org),
            json!({}),
            Some(&b)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
async fn upload(app: &Router, u: &User) -> Value {
    let (s, _, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/uploads", u.org),
        json!({"kind":"AUDIO","size_bytes":100,"content_type":"audio/wav"}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    v
}
#[sqlx::test]
async fn upload_errors_are_specific_and_content_is_sniffed(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let a = user(&app).await;
    let uploads = format!("/api/orgs/{}/uploads", a.org);
    for (body, status, code) in [
        (
            json!({"kind":"AUDIO","size_bytes":100,"content_type":"audio/mpeg"}),
            StatusCode::BAD_REQUEST,
            "UPLOAD_TYPE_UNSUPPORTED",
        ),
        (
            json!({"kind":"AUDIO","size_bytes":0,"content_type":"audio/wav"}),
            StatusCode::BAD_REQUEST,
            "UPLOAD_EMPTY",
        ),
        (
            json!({"kind":"AUDIO","size_bytes":600_i64*1024*1024,"content_type":"audio/wav"}),
            StatusCode::BAD_REQUEST,
            "UPLOAD_AUDIO_TOO_LARGE",
        ),
        (
            json!({"kind":"IMAGE","size_bytes":30_i64*1024*1024,"content_type":"image/png"}),
            StatusCode::BAD_REQUEST,
            "UPLOAD_IMAGE_TOO_LARGE",
        ),
    ] {
        let (s, _, v) = call(&app, "POST", &uploads, body, Some(&a)).await;
        assert_eq!(s, status, "{v}");
        assert_eq!(v["error"]["code"], code, "{v}");
        assert!(
            v["error"]["message"]
                .as_str()
                .is_some_and(|m| !m.is_empty()),
            "human-readable message missing: {v}"
        );
    }
    // An MP3 renamed to .wav (declared audio/wav) is refused at completion
    // and never registered, so it cannot reach QC or a DSP package.
    let up = upload(&app, &a).await;
    let key = up["expected_key"].as_str().unwrap();
    let mut mp3 = b"ID3\x04\x00".to_vec();
    mp3.resize(100, 0);
    store.bodies.lock().await.insert(key.into(), mp3);
    store.objects.lock().await.insert(
        key.into(),
        ObjectMeta {
            size: 100,
            content_type: "audio/wav".into(),
            nonce: up["grant"]["headers"]["x-amz-meta-upload-nonce"]
                .as_str()
                .unwrap()
                .into(),
            etag: "mp3-etag".into(),
        },
    );
    let path = format!(
        "/api/orgs/{}/uploads/{}/complete",
        a.org,
        up["upload_session_id"].as_str().unwrap()
    );
    let body = json!({"asset_id":up["asset_id"],"expected_key":key});
    let (s, _, v) = call(&app, "POST", &path, body, Some(&a)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "UPLOAD_CONTENT_MISMATCH");
    let (state, sha): (String, Option<String>) =
        sqlx::query_as("SELECT state,sha256 FROM catalog.assets WHERE id=$1")
            .bind(Uuid::parse_str(up["asset_id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_ne!(state, "REGISTERED");
    assert!(sha.is_none());
}

#[sqlx::test]
async fn upload_binding_expiry_duplicate_and_freeze(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let a = user(&app).await;
    let b = user(&app).await;
    let up = upload(&app, &a).await;
    let path = format!(
        "/api/orgs/{}/uploads/{}/complete",
        a.org,
        up["upload_session_id"].as_str().unwrap()
    );
    let body = json!({"asset_id":up["asset_id"],"expected_key":up["expected_key"]});
    assert_eq!(
        call(&app, "POST", &path, body.clone(), Some(&a)).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "POST", &path, body.clone(), Some(&b)).await.0,
        StatusCode::FORBIDDEN
    );
    let key = up["expected_key"].as_str().unwrap();
    let meta = ObjectMeta {
        size: 100,
        content_type: "audio/wav".into(),
        nonce: up["grant"]["headers"]["x-amz-meta-upload-nonce"]
            .as_str()
            .unwrap()
            .into(),
        etag: "opaque-multipart-etag-3".into(),
    };
    store.objects.lock().await.insert(key.into(), meta);
    assert_eq!(
        call(
            &app,
            "POST",
            &path,
            json!({"asset_id":up["asset_id"],"expected_key":"someone/elses/key"}),
            Some(&a)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (s, _, v) = call(&app, "POST", &path, body.clone(), Some(&a)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["qc_status"], "PENDING");
    // Regression (sandbox P0-1): completion must persist the content hash of
    // the frozen bytes, otherwise Stage 1 never runs audio QC.
    let expected_sha = hex::encode(sha2::Sha256::digest(synth_body("audio/wav", 100)));
    assert_eq!(v["sha256"], expected_sha.as_str());
    assert_eq!(v["detected_container"], "WAV");
    let stored: Option<String> =
        sqlx::query_scalar("SELECT sha256 FROM catalog.assets WHERE id=$1")
            .bind(Uuid::parse_str(up["asset_id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored.as_deref(), Some(expected_sha.as_str()));
    let (s, _, v) = call(&app, "POST", &path, body, Some(&a)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["duplicate"], true);
    let stable: String = sqlx::query_scalar("SELECT object_key FROM catalog.assets WHERE id=$1")
        .bind(Uuid::parse_str(up["asset_id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(stable, key);
    store.objects.lock().await.get_mut(key).unwrap().etag = "overwritten".into();
    assert_eq!(
        store.head(&stable).await.unwrap().unwrap().etag,
        "opaque-multipart-etag-3"
    );
    let before = store.calls.load(std::sync::atomic::Ordering::SeqCst);
    let expired = upload(&app, &a).await;
    let id = Uuid::parse_str(expired["upload_session_id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "UPDATE catalog.upload_sessions SET expires_at=now()-interval '1 second' WHERE id=$1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/orgs/{}/uploads/{id}/complete", a.org),
            json!({"asset_id":expired["asset_id"],"expected_key":expired["expected_key"]}),
            Some(&a)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        before,
        store.calls.load(std::sync::atomic::Ordering::SeqCst),
        "expired sessions must perform no storage IO"
    );
    sqlx::query("UPDATE identity.auth_limits SET attempts=60 WHERE bucket_hash=$1")
        .bind(audeniq_core::auth::hash_token(&format!(
            "upload-complete:{}",
            a.user
        )))
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &path,
            json!({"asset_id":up["asset_id"],"expected_key":up["expected_key"]}),
            Some(&a)
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        before,
        store.calls.load(std::sync::atomic::Ordering::SeqCst)
    );
    let file_path = format!(
        "/api/orgs/{}/assets/{}",
        a.org,
        up["asset_id"].as_str().unwrap()
    );
    assert_eq!(
        call(&app, "GET", &file_path, json!({}), Some(&b)).await.0,
        StatusCode::FORBIDDEN
    );
    let release = create(&app, &b, "releases").await;
    let artist = create(&app, &b, "artists").await;
    assert_eq!(call(&app,"POST",&format!("/api/orgs/{}/releases/{release}/tracks",b.org),json!({"title":"Track","disc_number":1,"track_number":1,"artist_id":artist,"asset_id":up["asset_id"],"row_version":0}),Some(&b)).await.0,StatusCode::FORBIDDEN);
}
#[sqlx::test]
async fn immutable_revisions_state_and_compare_and_swap(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let a = user(&app).await;
    let release = create(&app, &a, "releases").await;
    let rev = Uuid::new_v4();
    sqlx::query("INSERT INTO catalog.application_revisions(id,org_id,release_id,revision,body,body_hash,consent_package_hash,created_by,idempotency_key) VALUES($1,$2,$3,1,'{}',$4,$4,$5,'test-rev')").bind(rev).bind(a.org).bind(release).bind("a".repeat(64)).bind(a.user).execute(&pool).await.unwrap();
    assert!(
        sqlx::query("UPDATE catalog.application_revisions SET body='{}' WHERE id=$1")
            .bind(rev)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM catalog.application_revisions WHERE id=$1")
            .bind(rev)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("UPDATE catalog.releases SET status='READY_FOR_DELIVERY',row_version=row_version+1 WHERE id=$1").bind(release).execute(&pool).await.is_err());
    let verification = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    sqlx::query("INSERT INTO distribution.verification_packages(id,org_id,revision_id,body,package_hash,rights_epoch) VALUES($1,$2,$3,'{}',$4,0)").bind(verification).bind(a.org).bind(rev).bind("b".repeat(64)).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO distribution.release_snapshots(id,org_id,verification_id,body,snapshot_hash) VALUES($1,$2,$3,'{}',$4)").bind(snapshot).bind(a.org).bind(verification).bind("c".repeat(64)).execute(&pool).await.unwrap();
    for statement in [
        "UPDATE distribution.verification_packages SET body='{}'",
        "DELETE FROM distribution.verification_packages",
        "UPDATE distribution.release_snapshots SET body='{}'",
        "DELETE FROM distribution.release_snapshots",
    ] {
        let error = sqlx::query(statement).execute(&pool).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().unwrap(), "23514");
    }
    let path = format!("/api/orgs/{}/releases/{release}", a.org);
    let body = json!({"name":"Changed","release_type":"SINGLE","row_version":0});
    let (x, y) = tokio::join!(
        call(&app, "PUT", &path, body.clone(), Some(&a)),
        call(&app, "PUT", &path, body, Some(&a))
    );
    assert!(matches!(
        (x.0, y.0),
        (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
    ));
    let current: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(current, "DRAFT");
    assert!(
        sqlx::query("UPDATE operations.audit_events SET reason_code='tampered'")
            .execute(&pool)
            .await
            .is_err()
    );
}
#[sqlx::test]
async fn outbox_atomic_rollback_and_idempotency(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let a = user(&app).await;
    let aggregate = Uuid::new_v4();
    let mut tx = pool.begin().await.unwrap();
    let id = operations::event(&mut tx, a.org, aggregate, "test.event", "rollback-test")
        .await
        .unwrap();
    operations::audit(
        &mut tx,
        Some(a.user),
        Some(a.org),
        Some(aggregate),
        "test.created",
        "ROLLBACK_TEST",
        Uuid::new_v4(),
    )
    .await
    .unwrap();
    tx.rollback().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM operations.outbox WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM operations.jobs WHERE idempotency_key=$1"
        )
        .bind(format!("outbox:{id}"))
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    let mut tx = pool.begin().await.unwrap();
    let event = operations::event(&mut tx, a.org, aggregate, "test.event", "commit-test")
        .await
        .unwrap();
    let same = operations::event(&mut tx, a.org, aggregate, "test.event", "commit-test")
        .await
        .unwrap();
    assert_eq!(event, same);
    tx.commit().await.unwrap();
    let job = operations::claim(&pool, "interactive", "one", 60)
        .await
        .unwrap()
        .unwrap();
    operations::execute(&pool, &noop_store().await, &job)
        .await
        .unwrap();
    assert!(
        operations::execute(&pool, &noop_store().await, &job)
            .await
            .is_err()
    );
    // A duplicated event delivered in another safe job cannot duplicate the business effect.
    let mut tx = pool.begin().await.unwrap();
    operations::enqueue(
        &mut tx,
        "interactive",
        "outbox.record",
        &json!({"event_id":event}),
        "duplicate-delivery",
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let job = operations::claim(&pool, "interactive", "two", 60)
        .await
        .unwrap()
        .unwrap();
    operations::execute(&pool, &noop_store().await, &job)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM operations.event_receipts WHERE event_id=$1"
        )
        .bind(event)
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
}
#[sqlx::test]
async fn queue_claim_crash_fencing_retry_dead_letter(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    let id = operations::enqueue(
        &mut tx,
        "interactive",
        "outbox.record",
        &json!({"event_id":Uuid::new_v4()}),
        "only-job",
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let (a, b) = tokio::join!(
        operations::claim(&pool, "interactive", "a", 60),
        operations::claim(&pool, "interactive", "b", 60)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(usize::from(a.is_some()) + usize::from(b.is_some()), 1);
    let old = a.or(b).unwrap();
    sqlx::query("UPDATE operations.jobs SET lease_until=now()-interval '1 second' WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let new = operations::claim(&pool, "interactive", "restarted", 60)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(old.token, new.token);
    assert_eq!(new.attempts, 2);
    assert!(operations::heartbeat(&pool, &old, 60).await.is_err());
    assert!(
        operations::fail(&pool, &old, true, "STALE_WORKER")
            .await
            .is_err()
    );
    assert!(
        operations::execute(&pool, &noop_store().await, &old)
            .await
            .is_err()
    );
    operations::heartbeat(&pool, &new, 60).await.unwrap();
    operations::fail(&pool, &new, false, "RETRYABLE_TEST")
        .await
        .unwrap();
    assert!(
        operations::claim(&pool, "interactive", "early", 60)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query(
        "UPDATE operations.jobs SET run_at=now()-interval '1 second',max_attempts=3 WHERE id=$1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap();
    let last = operations::claim(&pool, "interactive", "last", 60)
        .await
        .unwrap()
        .unwrap();
    operations::fail(&pool, &last, false, "EXHAUSTED_TEST")
        .await
        .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "DEAD_LETTER");
    let mut tx = pool.begin().await.unwrap();
    operations::enqueue(
        &mut tx,
        "distribution",
        "dsp.send",
        &json!({}),
        "unimplemented-dsp",
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let j = operations::claim(&pool, "distribution", "dsp", 60)
        .await
        .unwrap()
        .unwrap();
    operations::execute(&pool, &noop_store().await, &j)
        .await
        .unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
        .bind(j.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "DEAD_LETTER");
}
#[sqlx::test]
async fn auth_rate_limit_and_db_token_hashes(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let row = sqlx::query("SELECT token_hash,csrf_hash FROM identity.sessions WHERE user_id=$1")
        .bind(u.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<Vec<u8>, _>("token_hash").len(), 32);
    assert_eq!(row.get::<Vec<u8>, _>("csrf_hash").len(), 32);
    for _ in 0..10 {
        let (s, _, _) = call(
            &app,
            "POST",
            "/api/auth/login",
            json!({"email":"bad@example.test","password":"invalid-password-long"}),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED)
    }
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/login",
            json!({"email":"bad@example.test","password":"invalid-password-long"}),
            None
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
}
#[sqlx::test]
async fn late_revision_check_never_advances_current_release(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let a = user(&app).await;
    let release = create(&app, &a, "releases").await;
    let old = Uuid::new_v4();
    let new = Uuid::new_v4();
    for (revision, id) in [(1, old), (2, new)] {
        sqlx::query("INSERT INTO catalog.application_revisions(id,org_id,release_id,revision,body,body_hash,consent_package_hash,created_by,idempotency_key) VALUES($1,$2,$3,$4,'{}',$5,$5,$6,$7)").bind(id).bind(a.org).bind(release).bind(revision).bind("a".repeat(64)).bind(a.user).bind(format!("test-{revision}")).execute(&pool).await.unwrap();
    }
    sqlx::query(
        "UPDATE catalog.releases SET current_revision_id=$2,row_version=row_version+1 WHERE id=$1",
    )
    .bind(release)
    .bind(new)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        operations::record_check(&pool, a.org, release, old, "AUDIO", "v1", &"b".repeat(64))
            .await
            .unwrap(),
        "STALE"
    );
    let r = sqlx::query(
        "SELECT status,current_revision_id,row_version FROM catalog.releases WHERE id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(r.get::<String, _>("status"), "DRAFT");
    assert_eq!(r.get::<Uuid, _>("current_revision_id"), new);
    assert_eq!(r.get::<i64, _>("row_version"), 1);
}

#[sqlx::test]
async fn runtime_roles_enforce_foundation_boundary(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    sqlx::raw_sql(
        "DO $$ BEGIN
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_api') THEN
           CREATE ROLE audeniq_api NOLOGIN;
         END IF;
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_worker') THEN
           CREATE ROLE audeniq_worker NOLOGIN;
         END IF;
         END $$;",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../../../deploy/grants.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let api_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE audeniq_api").execute(c).await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let state = AppState::new(
        api_pool.clone(),
        Config {
            database_url: "unused".into(),
            origin: ORIGIN.into(),
            service_secret: SECRET.into(),
            secure_cookie: false,
            bind: "127.0.0.1:0".into(),
            session_seconds: 3600,
        },
        Arc::new(MockStore::default()),
    )
    .await
    .unwrap();
    let api = router(state);
    let u = user(&api).await;
    let release = create(&api, &u, "releases").await;
    let artist = create(&api, &u, "artists").await;
    let base = format!("/api/orgs/{}/releases/{release}", u.org);
    let input = json!({"title":"Role test","disc_number":1,"track_number":1,"artist_id":artist,"row_version":0});
    let (status, _, added) = call(&api, "POST", &format!("{base}/tracks"), input, Some(&u)).await;
    assert_eq!(status, StatusCode::OK);
    let id = added["id"].as_str().unwrap();
    let path = format!("{base}/tracks/{id}/credits");
    let input = json!({"row_version":1,"credits":[{"party_id":u.party,"role":"composer"}]});
    let result = call(&api, "PUT", &path, input, Some(&u)).await;
    assert_eq!(result.0, StatusCode::OK);
    let input = json!({"row_version":2,"credits":[]});
    let result = call(&api, "PUT", &path, input, Some(&u)).await;
    assert_eq!(result.0, StatusCode::OK);
    let result = call(&api, "GET", "/api/auth/sessions", json!({}), Some(&u)).await;
    assert_eq!(result.0, StatusCode::OK);
    for statement in [
        "UPDATE operations.audit_events SET action='tampered'",
        "TRUNCATE operations.audit_events",
        // The API appends submitted revisions and consent packages (consent +
        // submit run in the request) but can never rewrite or remove them.
        "UPDATE catalog.application_revisions SET body='{}'",
        "DELETE FROM catalog.application_revisions",
        "UPDATE catalog.consent_packages SET body='{}'",
        "DELETE FROM catalog.consent_packages",
        "INSERT INTO operations.check_results DEFAULT VALUES",
        "INSERT INTO distribution.validation_packages DEFAULT VALUES",
        "INSERT INTO distribution.packages DEFAULT VALUES",
        "INSERT INTO rights.contracts DEFAULT VALUES",
        // Activation records are platform-operator owned: the API role must
        // not be able to flip a delivery profile on.
        "UPDATE execution.adapter_profiles SET delivery_enabled=true",
    ] {
        let error = sqlx::query(statement).execute(&api_pool).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().unwrap(), "42501");
    }
    let worker_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE audeniq_worker").execute(c).await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();
    let job = operations::claim(&worker_pool, "interactive", "role-test", 60)
        .await
        .unwrap()
        .unwrap();
    operations::execute(&worker_pool, &noop_store().await, &job)
        .await
        .unwrap();
    assert!(
        sqlx::query("SELECT password_hash FROM identity.users")
            .fetch_all(&worker_pool)
            .await
            .is_err()
    );
    // The worker runs deliveries but must not activate profiles either:
    // delivery_enabled is flipped only by the platform operator (owner).
    let error = sqlx::query("UPDATE execution.adapter_profiles SET delivery_enabled=false")
        .execute(&worker_pool)
        .await
        .unwrap_err();
    assert_eq!(error.as_database_error().unwrap().code().unwrap(), "42501");
}

#[sqlx::test]
async fn worker_pipeline_grants_cover_handoff_and_reconciler(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    sqlx::raw_sql(
        "DO $$ BEGIN
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_api') THEN
           CREATE ROLE audeniq_api NOLOGIN;
         END IF;
         IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname='audeniq_worker') THEN
           CREATE ROLE audeniq_worker NOLOGIN;
         END IF;
         END $$;",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!("../../../deploy/grants.sql"))
        .execute(&pool)
        .await
        .unwrap();
    // The worker needs these for the READY_FOR_DELIVERY -> delivery.enqueue
    // handoff and the F5 reconciler (identity.orgs enumeration).
    for (table, privs) in [
        ("identity.orgs", "SELECT"),
        ("rights.rights_epochs", "SELECT"),
        ("catalog.releases", "SELECT,UPDATE"),
        ("catalog.assets", "SELECT,UPDATE"),
        ("distribution.preparation_artifacts", "SELECT,INSERT"),
        ("distribution.identifier_assignments", "SELECT,INSERT"),
        ("distribution.ddex_messages", "SELECT,INSERT"),
        ("execution.delivery_jobs", "SELECT,INSERT,UPDATE"),
        ("execution.delivery_attempts", "SELECT,INSERT,UPDATE"),
        ("execution.live_bindings", "SELECT,INSERT,UPDATE"),
        ("execution.reconciliation_cases", "SELECT,INSERT,UPDATE"),
        ("execution.adapter_profiles", "SELECT"),
        ("finance.payout_orders", "SELECT,INSERT,UPDATE"),
        ("operations.check_results", "INSERT"),
    ] {
        for priv_name in privs.split(',') {
            let ok: bool =
                sqlx::query_scalar("SELECT has_table_privilege('audeniq_worker', $1, $2)")
                    .bind(table)
                    .bind(priv_name)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert!(ok, "audeniq_worker missing {priv_name} on {table}");
        }
    }
    // The API role must still be denied writes on the pipeline schemas.
    for (table, priv_name) in [
        ("execution.delivery_jobs", "INSERT"),
        ("distribution.packages", "INSERT"),
        ("finance.ledger_transactions", "INSERT"),
    ] {
        let ok: bool = sqlx::query_scalar("SELECT has_table_privilege('audeniq_api', $1, $2)")
            .bind(table)
            .bind(priv_name)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(!ok, "audeniq_api unexpectedly has {priv_name} on {table}");
    }
}

#[sqlx::test]
async fn draft_tracks_credits_archive_and_preflight(pool: PgPool) {
    let (api, _) = app(pool.clone()).await;
    let a = user(&api).await;
    let b = user(&api).await;
    let release = create(&api, &a, "releases").await;
    let artist = create(&api, &a, "artists").await;
    let base = format!("/api/orgs/{}/releases/{release}", a.org);
    let (_, _, empty) = call(
        &api,
        "GET",
        &format!("{base}/preflight"),
        json!({}),
        Some(&a),
    )
    .await;
    assert_eq!(empty["issues"][0]["code"], "TRACK_REQUIRED");
    let input = json!({"title":"First","disc_number":1,"track_number":1,"artist_id":artist,"row_version":0});
    let (status, _, added) = call(
        &api,
        "POST",
        &format!("{base}/tracks"),
        input.clone(),
        Some(&a),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{added}");
    let id = added["id"].as_str().unwrap();
    let path = format!("{base}/tracks/{id}");
    let mut edited = input;
    edited["title"] = json!("Edited");
    edited["row_version"] = json!(1);
    assert_eq!(
        call(&api, "PUT", &path, edited.clone(), Some(&b)).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&api, "PUT", &path, edited.clone(), Some(&a)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&api, "PUT", &path, edited, Some(&a)).await.0,
        StatusCode::CONFLICT
    );
    let credits = format!("{path}/credits");
    let bad = json!({"row_version":2,"credits":[{"party_id":b.party,"role":"composer"}]});
    assert_eq!(
        call(&api, "PUT", &credits, bad, Some(&a)).await.0,
        StatusCode::FORBIDDEN
    );
    let good = json!({"row_version":2,"credits":[{"party_id":a.party,"role":"composer"}]});
    assert_eq!(
        call(&api, "PUT", &credits, good, Some(&a)).await.0,
        StatusCode::OK
    );
    let (_, _, detail) = call(&api, "GET", &base, json!({}), Some(&a)).await;
    assert_eq!(detail["row_version"], 3);
    assert_eq!(detail["tracks"][0]["title"], "Edited");
    assert_eq!(
        detail["tracks"][0]["credits"][0]["party_id"],
        a.party.to_string()
    );
    let (_, _, preflight) = call(
        &api,
        "GET",
        &format!("{base}/preflight"),
        json!({}),
        Some(&a),
    )
    .await;
    assert_eq!(preflight["issues"][0]["code"], "AUDIO_REQUIRED");
    assert_eq!(preflight["ready_to_submit"], false);
    assert_eq!(preflight["submission_enabled"], false);
    assert_eq!(
        call(&api, "DELETE", &path, json!({"row_version":3}), Some(&a))
            .await
            .0,
        StatusCode::OK
    );
    let (_, _, detail) = call(&api, "GET", &base, json!({}), Some(&a)).await;
    assert_eq!(detail["tracks"], json!([]));
    let retained: bool =
        sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM catalog.tracks WHERE id=$1")
            .bind(Uuid::parse_str(id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(retained);
    let replacement = json!({"title":"Replacement","disc_number":1,"track_number":1,"artist_id":artist,"row_version":4});
    assert_eq!(
        call(
            &api,
            "POST",
            &format!("{base}/tracks"),
            replacement,
            Some(&a)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(
        sqlx::query("DELETE FROM catalog.tracks WHERE id=$1")
            .bind(Uuid::parse_str(id).unwrap())
            .execute(&pool)
            .await
            .is_err()
    );
    assert_eq!(
        call(
            &api,
            "PUT",
            &credits,
            json!({"row_version":5,"credits":[]}),
            Some(&a)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let version: i64 = sqlx::query_scalar("SELECT row_version FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 5);
}

#[sqlx::test]
async fn cursor_pagination_rechecks_acl_and_rejects_invalid_limits(pool: PgPool) {
    let (api, _) = app(pool.clone()).await;
    let a = user(&api).await;
    let b = user(&api).await;
    for _ in 0..3 {
        create(&api, &a, "artists").await;
    }
    create(&api, &b, "artists").await;
    let base = format!("/api/orgs/{}/artists", a.org);
    let (s, _, first) = call(&api, "GET", &format!("{base}?limit=2"), json!({}), Some(&a)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(first["items"].as_array().unwrap().len(), 2);
    let cursor = first["next_cursor"].as_str().unwrap();
    let (_, _, second) = call(
        &api,
        "GET",
        &format!("{base}?limit=2&after={cursor}"),
        json!({}),
        Some(&a),
    )
    .await;
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert!(second["next_cursor"].is_null());
    let last = Uuid::parse_str(second["items"][0]["id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "UPDATE identity.resource_acl SET revoked_at=now() WHERE resource_id=$1 AND action='read'",
    )
    .bind(last)
    .execute(&pool)
    .await
    .unwrap();
    let (_, _, hidden) = call(
        &api,
        "GET",
        &format!("{base}?limit=2&after={cursor}"),
        json!({}),
        Some(&a),
    )
    .await;
    assert_eq!(hidden["items"], json!([]));
    assert_eq!(
        call(&api, "GET", &base, json!({}), Some(&b)).await.0,
        StatusCode::FORBIDDEN
    );
    for query in ["limit=0", "limit=101", "after=invalid", "unknown=true"] {
        assert_eq!(
            call(&api, "GET", &format!("{base}?{query}"), json!({}), Some(&a))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
}

#[sqlx::test]
async fn session_inventory_revoke_and_password_rotation(pool: PgPool) {
    let (api, _) = app(pool.clone()).await;
    let a = user(&api).await;
    let b = user(&api).await;
    let email: String = sqlx::query_scalar("SELECT email FROM identity.users WHERE id=$1")
        .bind(a.user)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (_, _, inventory) = call(&api, "GET", "/api/auth/sessions", json!({}), Some(&a)).await;
    assert_eq!(inventory["items"].as_array().unwrap().len(), 1);
    assert_eq!(inventory["items"][0]["current"], true);
    assert!(inventory["items"][0].get("token_hash").is_none());
    let session_id = inventory["items"][0]["id"].as_str().unwrap();
    let path = format!("/api/auth/sessions/{session_id}/revoke");
    assert_eq!(
        call(&api, "POST", &path, json!({}), Some(&b)).await.0,
        StatusCode::NOT_FOUND
    );
    let credentials = json!({"email":email,"password":"Long-test-password-123!"});
    let (status, h, login) = call(&api, "POST", "/api/auth/login", credentials.clone(), None).await;
    assert_eq!(status, StatusCode::OK);
    let mut second = a.clone();
    second.cookie = h["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into();
    second.csrf = login["csrf_token"].as_str().unwrap().into();
    assert_eq!(
        call(&api, "POST", &path, json!({}), Some(&second)).await.0,
        StatusCode::OK
    );
    assert_eq!(
        call(&api, "GET", "/api/me", json!({}), Some(&a)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let bad = json!({"current_password":"Wrong-test-password!","new_password":"Changed-long-password-456!"});
    assert_eq!(
        call(&api, "POST", "/api/auth/password", bad, Some(&second))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&api, "GET", "/api/me", json!({}), Some(&second))
            .await
            .0,
        StatusCode::OK
    );
    let change = json!({"current_password":"Long-test-password-123!","new_password":"Changed-long-password-456!"});
    let (status, headers, response) =
        call(&api, "POST", "/api/auth/password", change, Some(&second)).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert!(
        headers["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        call(&api, "GET", "/api/me", json!({}), Some(&second))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&api, "POST", "/api/auth/login", credentials, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, h, login) = call(
        &api,
        "POST",
        "/api/auth/login",
        json!({"email":email,"password":"Changed-long-password-456!"}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    second.cookie = h["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into();
    second.csrf = login["csrf_token"].as_str().unwrap().into();
    assert_eq!(
        call(
            &api,
            "POST",
            "/api/auth/logout-all",
            json!({}),
            Some(&second)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&api, "GET", "/api/me", json!({}), Some(&second))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&api, "GET", "/api/me", json!({}), Some(&b)).await.0,
        StatusCode::OK
    );
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.sessions WHERE user_id=$1 AND revoked_at IS NULL",
    )
    .bind(a.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active, 0);
}

#[sqlx::test]
async fn immutable_contract_route_package_lineage(pool: PgPool) {
    use audeniq_core::{
        artifacts,
        contracts::{DeliveryOperation, DistributionPackage, RouteContract, RouteKind},
    };
    let (api, _) = app(pool.clone()).await;
    let a = user(&api).await;
    let b = user(&api).await;
    let release = create(&api, &a, "releases").await;
    let revision = Uuid::new_v4();
    let verification = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let other_party = Uuid::new_v4();
    let document = Uuid::new_v4();
    let contract = Uuid::new_v4();
    let contract_revision = Uuid::new_v4();
    let endpoint = Uuid::new_v4();
    let dsp = Uuid::new_v4();
    let route = Uuid::new_v4();
    let fee = Uuid::new_v4();
    let hash = "a".repeat(64);
    // Synthetic owner-only fixtures; these are not approved agreements or production packages.
    sqlx::query("INSERT INTO catalog.application_revisions(id,org_id,release_id,revision,body,body_hash,consent_package_hash,created_by,idempotency_key) VALUES($1,$2,$3,1,'{}',$4,$4,$5,'test-rev')").bind(revision).bind(a.org).bind(release).bind(&hash).bind(a.user).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO distribution.verification_packages(id,org_id,revision_id,body,package_hash,rights_epoch) VALUES($1,$2,$3,'{}',$4,0)").bind(verification).bind(a.org).bind(revision).bind(&hash).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO distribution.release_snapshots(id,org_id,verification_id,body,snapshot_hash) VALUES($1,$2,$3,'{}',$4)").bind(snapshot).bind(a.org).bind(verification).bind(&hash).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON','Synthetic counterparty')").bind(other_party).bind(a.org).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(a.org)
        .bind(document)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type) VALUES($1,$2,'IMAGE',$3,1,'image/png')").bind(document).bind(a.org).bind(format!("test-document/{document}")).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rights.contracts(id,org_id,grantor_party_id,grantee_party_id) VALUES($1,$2,$3,$4)").bind(contract).bind(a.org).bind(a.party).bind(other_party).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO rights.contract_revisions(id,org_id,contract_id,revision,document_asset_id,document_hash,policy_version) VALUES($1,$2,$3,1,$4,$5,'fixture-only')").bind(contract_revision).bind(a.org).bind(contract).bind(document).bind(&hash).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO distribution.dsp_endpoints(id,org_id,dsp_id,adapter_version,profile_version) VALUES($1,$2,$3,'fixture-adapter','fixture-profile')").bind(endpoint).bind(a.org).bind(dsp).execute(&pool).await.unwrap();
    let route_sql = "INSERT INTO distribution.route_plans(id,org_id,dsp_id,route_kind,contract_id,contract_revision_id,endpoint_id,fee_schedule_id,enabled) VALUES($1,$2,$3,'DIRECT',$4,$5,$6,$7,$8)";
    assert!(
        sqlx::query(route_sql)
            .bind(route)
            .bind(a.org)
            .bind(dsp)
            .bind(contract)
            .bind(contract_revision)
            .bind(endpoint)
            .bind(fee)
            .bind(true)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query(route_sql)
            .bind(route)
            .bind(b.org)
            .bind(dsp)
            .bind(contract)
            .bind(contract_revision)
            .bind(endpoint)
            .bind(fee)
            .bind(false)
            .execute(&pool)
            .await
            .is_err()
    );
    sqlx::query(route_sql)
        .bind(route)
        .bind(a.org)
        .bind(dsp)
        .bind(contract)
        .bind(contract_revision)
        .bind(endpoint)
        .bind(fee)
        .bind(false)
        .execute(&pool)
        .await
        .unwrap();
    let mut package = DistributionPackage {
        id: Uuid::new_v4(),
        org_id: a.org,
        snapshot_id: snapshot,
        route: RouteContract {
            id: route,
            org_id: a.org,
            dsp_id: dsp,
            kind: RouteKind::Direct,
            contract_id: contract,
            endpoint_id: endpoint,
            fee_schedule_id: fee,
            profile_version: "fixture-profile".into(),
            adapter_version: "fixture-adapter".into(),
        },
        operation: DeliveryOperation::NewRelease,
        package_digest: hash.clone(),
        manifest_hash: hash,
        immutable_bytes_ref: Uuid::new_v4(),
    };
    let mut c = pool.acquire().await.unwrap();
    assert_eq!(
        artifacts::store_package(&mut c, &package).await.unwrap(),
        package.id
    );
    assert_eq!(
        artifacts::store_package(&mut c, &package).await.unwrap(),
        package.id
    );
    package.manifest_hash = "b".repeat(64);
    assert!(artifacts::store_package(&mut c, &package).await.is_err());
    package.route.adapter_version = "different".into();
    assert!(artifacts::store_package(&mut c, &package).await.is_err());
    for table in [
        "rights.contracts",
        "rights.contract_revisions",
        "distribution.dsp_endpoints",
        "distribution.route_plans",
        "distribution.packages",
    ] {
        let error = sqlx::query(&format!("DELETE FROM {table}"))
            .execute(&pool)
            .await
            .unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().unwrap(), "23514");
    }
    assert!(
        sqlx::query("UPDATE distribution.packages SET manifest_hash=$1")
            .bind("c".repeat(64))
            .execute(&pool)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM distribution.packages")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test]
async fn upload_cancel_blocks_late_completion_and_is_idempotent(pool: PgPool) {
    let (api, store) = app(pool.clone()).await;
    let a = user(&api).await;
    let b = user(&api).await;
    let up = upload(&api, &a).await;
    let id = up["upload_session_id"].as_str().unwrap();
    let base = format!("/api/orgs/{}/uploads/{id}", a.org);
    let result = call(&api, "GET", &base, json!({}), Some(&a)).await;
    assert_eq!(result.2["status"], "ISSUED");
    assert!(result.2.get("expected_key").is_none());
    assert_eq!(
        call(&api, "GET", &base, json!({}), Some(&b)).await.0,
        StatusCode::FORBIDDEN
    );
    let cancel = format!("{base}/cancel");
    let result = call(&api, "POST", &cancel, json!({}), Some(&b)).await;
    assert_eq!(result.0, StatusCode::FORBIDDEN);
    let result = call(&api, "POST", &cancel, json!({}), Some(&a)).await;
    assert_eq!(result.0, StatusCode::OK);
    assert_eq!(result.2["duplicate"], false);
    let result = call(&api, "POST", &cancel, json!({}), Some(&a)).await;
    assert_eq!(result.2["duplicate"], true);
    let key = up["expected_key"].as_str().unwrap();
    store.objects.lock().await.insert(
        key.into(),
        ObjectMeta {
            size: 100,
            content_type: "audio/wav".into(),
            nonce: up["grant"]["headers"]["x-amz-meta-upload-nonce"]
                .as_str()
                .unwrap()
                .into(),
            etag: "late-upload".into(),
        },
    );
    let body = json!({"asset_id":up["asset_id"],"expected_key":key});
    let result = call(&api, "POST", &format!("{base}/complete"), body, Some(&a)).await;
    assert_eq!(result.0, StatusCode::CONFLICT);
    let result = call(&api, "GET", &base, json!({}), Some(&a)).await;
    assert_eq!(result.2["status"], "CANCELLED");
    let state: String = sqlx::query_scalar("SELECT state FROM catalog.assets WHERE id=$1")
        .bind(Uuid::parse_str(up["asset_id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "REJECTED");
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events WHERE action='upload.cancelled'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audits, 1);
}

#[sqlx::test]
async fn csrf_bootstrap_is_same_origin_stable_and_revocation_safe(pool: PgPool) {
    let (app, _) = app(pool).await;
    let mut a = user(&app).await;
    let (_, _, v) = call(&app, "POST", "/api/auth/csrf", json!({}), Some(&a)).await;
    let token = v["csrf_token"].as_str().unwrap().to_owned();
    assert_eq!(token, a.csrf);
    let request = Request::builder()
        .method("POST")
        .uri("/api/auth/csrf")
        .header("x-audeniq-service", SECRET)
        .header("origin", "https://evil.invalid")
        .header("cookie", &a.cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    a.csrf = token;
    assert_eq!(
        call(&app, "POST", "/api/auth/logout", json!({}), Some(&a))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", "/api/auth/csrf", json!({}), Some(&a))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test]
async fn inactive_users_cannot_receive_active_membership(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let a = user(&app).await;
    let b = user(&app).await;
    sqlx::query("UPDATE identity.users SET status='DISABLED' WHERE id=$1")
        .bind(b.user)
        .execute(&pool)
        .await
        .unwrap();
    let path = format!("/api/orgs/{}/memberships", a.org);
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            json!({"user_id":b.user,"role":"EDITOR","status":"ACTIVE"}),
            Some(&a)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            &path,
            json!({"user_id":b.user,"role":"EDITOR","status":"REVOKED"}),
            Some(&a)
        )
        .await
        .0,
        StatusCode::OK
    );
}
