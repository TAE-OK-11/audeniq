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
        Ok(self.objects.lock().await.get(key).cloned())
    }
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()> {
        let mut m = self.objects.lock().await;
        let obj = m.get(source).cloned().ok_or(Error::Storage)?;
        if obj.etag != etag {
            return Err(Error::Conflict);
        }
        m.insert(target.into(), obj);
        Ok(())
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
        StatusCode::NOT_IMPLEMENTED
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
    call(
        &app,
        "PUT",
        &format!("/api/orgs/{}/memberships", a.org),
        json!({"user_id":b.user,"role":"EDITOR","status":"ACTIVE"}),
        Some(&a),
    )
    .await;
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
    sqlx::query("INSERT INTO catalog.application_revisions(id,org_id,release_id,revision,body,body_hash,consent_package_hash,created_by) VALUES($1,$2,$3,1,'{}',$4,$4,$5)").bind(rev).bind(a.org).bind(release).bind("a".repeat(64)).bind(a.user).execute(&pool).await.unwrap();
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
    operations::execute(&pool, &job).await.unwrap();
    assert!(operations::execute(&pool, &job).await.is_err());
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
    operations::execute(&pool, &job).await.unwrap();
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
    assert!(operations::execute(&pool, &old).await.is_err());
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
    operations::execute(&pool, &j).await.unwrap();
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
        sqlx::query("INSERT INTO catalog.application_revisions(id,org_id,release_id,revision,body,body_hash,consent_package_hash,created_by) VALUES($1,$2,$3,$4,'{}',$5,$5,$6)").bind(id).bind(a.org).bind(release).bind(revision).bind("a".repeat(64)).bind(a.user).execute(&pool).await.unwrap();
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
    create(&api, &u, "releases").await;
    for statement in [
        "UPDATE operations.audit_events SET action='tampered'",
        "TRUNCATE operations.audit_events",
        "INSERT INTO catalog.application_revisions DEFAULT VALUES",
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
    operations::execute(&worker_pool, &job).await.unwrap();
    assert!(
        sqlx::query("SELECT password_hash FROM identity.users")
            .fetch_all(&worker_pool)
            .await
            .is_err()
    );
}
