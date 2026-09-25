//! F2 Pre-submit + Stage 1 integration tests: real Postgres, real ffprobe,
//! worker job pipeline end to end.
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
    http::{Request, StatusCode},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha2::Digest;
use sqlx::PgPool;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-only-service-secret-32-characters";
const ORIGIN: &str = "http://localhost:5173";

/// In-memory object store that serves fixture bytes and counts downloads.
#[derive(Default)]
struct FileStore {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    get_calls: AtomicUsize,
}
#[async_trait]
impl ObjectStore for FileStore {
    async fn presign_put(
        &self,
        _key: &str,
        _size: i64,
        _mime: &str,
        _nonce: &str,
        _expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        Err(Error::Storage)
    }
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>> {
        Ok(self.files.lock().await.get(key).map(|b| ObjectMeta {
            size: b.len() as i64,
            content_type: "application/octet-stream".into(),
            nonce: String::new(),
            etag: String::new(),
        }))
    }
    async fn freeze(&self, _source: &str, _target: &str, _etag: &str) -> Result<()> {
        Err(Error::Storage)
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        self.files
            .lock()
            .await
            .get(key)
            .cloned()
            .ok_or(Error::Storage)
    }
}

async fn app(pool: PgPool) -> (Router, Arc<FileStore>) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(FileStore::default());
    let s = AppState::new(
        pool,
        Config {
            database_url: String::new(),
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
#[allow(dead_code)]
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
) -> (StatusCode, Value) {
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
    let credentials = json!({"email":email,"password":"Long-test-password-123!"});
    let (s, r) = call(app, "POST", "/api/auth/register", credentials.clone(), None).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    // login via a raw request to capture both the session cookie and csrf token
    let req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .body(Body::from(credentials.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let l: Value = serde_json::from_slice(&bytes).unwrap();
    User {
        user: Uuid::parse_str(r["user_id"].as_str().unwrap()).unwrap(),
        org: Uuid::parse_str(r["org_id"].as_str().unwrap()).unwrap(),
        party: Uuid::parse_str(r["party_id"].as_str().unwrap()).unwrap(),
        cookie,
        csrf: l["csrf_token"].as_str().unwrap().into(),
    }
}

async fn create_release(app: &Router, u: &User) -> Uuid {
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases", u.org),
        json!({"name":"Draft","release_type":"SINGLE"}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}

async fn create_artist(app: &Router, u: &User) -> Uuid {
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/artists", u.org),
        json!({"name":"Artist"}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

/// Generate a valid 32s stereo 48kHz WAV with ffmpeg, mastered into the
/// -14 LUFS ±1 band so the loudness check passes cleanly.
fn make_good_wav(dir: &std::path::Path) -> Vec<u8> {
    make_sine_wav(dir, "good.wav", 440)
}

/// 32 s stereo sine at `freq` Hz, loud enough to pass QC loudness gates.
fn make_sine_wav(dir: &std::path::Path, name: &str, freq: u32) -> Vec<u8> {
    let out = dir.join(name);
    let st = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={freq}:duration=32"),
            "-ar",
            "48000",
            "-ac",
            "2",
            "-filter:a",
            "volume=8dB",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&out)
        .status()
        .expect("ffmpeg runs");
    assert!(st.success());
    std::fs::read(&out).unwrap()
}

async fn register_asset(
    pool: &PgPool,
    store: &FileStore,
    u: &User,
    name: &str,
    bytes: &[u8],
) -> Uuid {
    let org = u.org;
    let id = Uuid::new_v4();
    let key = format!("registered/{org}/{id}/{name}");
    store.files.lock().await.insert(key.clone(), bytes.to_vec());
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(org)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    for action in ["read", "write"] {
        sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action) VALUES($1,$2,$3,$4)")
            .bind(org)
            .bind(id)
            .bind(u.party)
            .bind(action)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,sha256,state) VALUES($1,$2,'AUDIO',$3,$4,'audio/wav',$5,'REGISTERED')")
        .bind(id)
        .bind(org)
        .bind(&key)
        .bind(bytes.len() as i64)
        .bind(sha256_hex(bytes))
        .execute(pool)
        .await
        .unwrap();
    id
}

async fn row_version(pool: &PgPool, release: Uuid) -> i64 {
    sqlx::query_scalar("SELECT row_version FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Build a release that is fully submittable: date, track+asset, credit.
async fn build_submittable(
    app: &Router,
    pool: &PgPool,
    _store: &FileStore,
    u: &User,
    asset: Uuid,
) -> (Uuid, Uuid) {
    let release = create_release(app, u).await;
    let artist = create_artist(app, u).await;
    sqlx::query("UPDATE catalog.releases SET draft = draft || '{\"release_date\":\"2027-03-01\",\"p_line\":\"℗ 2027 Test Label\",\"c_line\":\"© 2027 Test Label\"}'::jsonb, row_version = row_version + 1 WHERE id=$1")
        .bind(release)
        .execute(pool)
        .await
        .unwrap();
    let rv = row_version(pool, release).await;
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
        json!({"title":"T1","disc_number":1,"track_number":1,"artist_id":artist,"asset_id":asset,"row_version":rv}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let track = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
    let rv = row_version(pool, release).await;
    let (s, v) = call(
        app,
        "PUT",
        &format!(
            "/api/orgs/{}/releases/{release}/tracks/{track}/credits",
            u.org
        ),
        json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"},{"party_id":u.party,"role":"COMPOSER"}]}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    (release, artist)
}

async fn consent(app: &Router, u: &User, release: Uuid, minority: bool) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/consents", u.org),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":minority}),
        Some(u),
    )
    .await
}

async fn submit(
    app: &Router,
    u: &User,
    release: Uuid,
    consent_id: Uuid,
    minority: bool,
    key: &str,
) -> (StatusCode, Value) {
    call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/submit", u.org),
        json!({"consent_id":consent_id,"minority_declared":minority,"idempotency_key":key,"declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        Some(u),
    )
    .await
}

/// Object store wrapper whose head() inflates sizes: exercises the analyzer
/// size cap without moving gigabytes.
struct InflatedStore {
    inner: Arc<FileStore>,
}
#[async_trait]
impl ObjectStore for InflatedStore {
    async fn presign_put(
        &self,
        key: &str,
        size: i64,
        mime: &str,
        nonce: &str,
        expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        self.inner
            .presign_put(key, size, mime, nonce, expires)
            .await
    }
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>> {
        let mut m = self.inner.head(key).await?.ok_or(Error::Storage)?;
        m.size = i64::MAX / 2;
        Ok(Some(m))
    }
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()> {
        self.inner.freeze(source, target, etag).await
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        self.inner.get(key).await
    }
}

/// Claim the queued stage1 job and run it through the worker dispatcher.
async fn run_worker(pool: &PgPool, store: &Arc<FileStore>) {
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    run_worker_on(pool, &dyn_store, true).await;
}

/// Same, over an arbitrary store, with the expected terminal job status.
async fn run_worker_on(pool: &PgPool, store: &Arc<dyn ObjectStore>, expect_success: bool) {
    let job = operations::claim(pool, "qc", "test-worker", 60)
        .await
        .unwrap()
        .expect("stage1 job queued");
    assert_eq!(job.kind, "stage1");
    operations::execute(pool, store, &job).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
        .bind(job.id)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(
        status,
        if expect_success {
            "SUCCEEDED"
        } else {
            "QUEUED"
        }
    );
}

/// RAII temp dir: removed on drop, even if the test panics. Prevents
/// /tmp (512MB tmpfs) from filling up across parallel test runs.
struct TmpDir {
    path: PathBuf,
}
impl TmpDir {
    fn path(&self) -> &PathBuf {
        &self.path
    }
}
impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn tmpdir() -> TmpDir {
    let d = std::env::temp_dir().join(format!("audeniq-f2-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    TmpDir { path: d }
}

#[sqlx::test]
async fn presubmit_gates_track_requirement(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let release = create_release(&app, &u).await;
    let (s, v) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/releases/{release}/presubmit", u.org),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(!v["ready_to_submit"].as_bool().unwrap());
    assert!(
        v["gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "TRACK_REQUIRED")
    );

    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (full_release, _) = build_submittable(&app, &pool, &store, &u, asset).await;

    let (s, v) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/releases/{full_release}/presubmit", u.org),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["ready_to_submit"].as_bool().unwrap(), "{v}");
    assert!(v["gates"].as_array().unwrap().is_empty(), "{v}");
}

#[sqlx::test]
async fn minority_paths_are_hard_gated(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;

    let (s, v) = consent(&app, &u, release, true).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "MINORITY_REVIEW_REQUIRED");

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();

    let (s, v) = submit(&app, &u, release, consent_id, true, "k-minor").await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "MINORITY_REVIEW_REQUIRED");
}

#[sqlx::test]
async fn consent_scope_mismatch_rejects_submit(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, artist) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();

    // Change the draft after consent: add a second track.
    let rv = row_version(&pool, release).await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
        json!({"title":"T2","disc_number":1,"track_number":2,"artist_id":artist,"asset_id":asset,"row_version":rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let (s, v) = submit(&app, &u, release, consent_id, false, "k-scope").await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "CONSENT_SCOPE_MISMATCH");
}

#[sqlx::test]
async fn stage1_happy_path_passes(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;

    let (s, v) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/releases/{release}/presubmit", u.org),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["ready_to_submit"].as_bool().unwrap(), "{v}");
    assert!(v["gates"].as_array().unwrap().is_empty());

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();

    let (s, v) = submit(&app, &u, release, consent_id, false, "k-happy").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "SUBMITTED");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_PASSED");

    let codes: Vec<String> = sqlx::query_scalar(
        "SELECT check_code FROM operations.check_results WHERE revision_id=$1 ORDER BY check_code",
    )
    .bind(revision_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    // 21 metadata/policy checks (round 2 added TEXT_INVALID_CHARACTERS,
    // ARTIST_NAME_PROTECTED, IDENTIFIER_IN_USE, ASSET_REUSED) + 15 audio
    // checks (QC rule v3 added AUDIO_CONTENT_SUSPECT).
    assert_eq!(codes.len(), 36, "{codes:?}");
    let bad: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations.check_results WHERE revision_id=$1 AND status<>'PASS'",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bad, 0);

    let pkg: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, package_hash FROM distribution.validation_packages WHERE revision_id=$1",
    )
    .bind(revision_id)
    .fetch_optional(&pool)
    .await
    .unwrap();
    let (pkg_id, pkg_hash) = pkg.expect("validation package created");
    assert_eq!(pkg_hash.len(), 64);
    let body_hash: String =
        sqlx::query_scalar("SELECT body_hash FROM catalog.application_revisions WHERE id=$1")
            .bind(revision_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let pkg_rev: String = sqlx::query_scalar(
        "SELECT body->>'revision_hash' FROM distribution.validation_packages WHERE id=$1",
    )
    .bind(pkg_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pkg_rev, body_hash);

    let qc: String = sqlx::query_scalar("SELECT qc_status FROM catalog.assets WHERE id=$1")
        .bind(asset)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(qc, "PASS");

    // submission status endpoint reflects the passed state
    let (s, v) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/releases/{release}/submission", u.org),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "STAGE1_PASSED");
    assert!(
        v["validation_package"]["package_hash"]
            .as_str()
            .unwrap()
            .starts_with(&pkg_hash[..16])
    );

    // Stage 2 handoff: the pass-path enqueues the rights/stage2 job atomically
    // with the validation package.
    let stage2: Option<(Uuid, Uuid, Value)> = sqlx::query_as(
        "SELECT id, pinned_revision_id, payload FROM operations.jobs WHERE queue='rights' AND kind='stage2'",
    )
    .fetch_optional(&pool)
    .await
    .unwrap();
    let (_, pinned, payload) = stage2.expect("stage2 job enqueued");
    assert_eq!(pinned, revision_id);
    assert_eq!(
        Uuid::parse_str(payload["validation_package_id"].as_str().unwrap()).unwrap(),
        pkg_id
    );
    assert_eq!(payload["package_hash"].as_str().unwrap(), pkg_hash.as_str());
}
#[sqlx::test]
async fn stage1_duplicate_isrc_rejected(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, artist) = build_submittable(&app, &pool, &store, &u, asset).await;
    // EP allows two tracks; both carry the same ISRC.
    sqlx::query(
        "UPDATE catalog.releases SET release_type='EP', row_version=row_version+1 WHERE id=$1",
    )
    .bind(release)
    .execute(&pool)
    .await
    .unwrap();
    let rv = row_version(&pool, release).await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
        json!({"title":"T2","disc_number":1,"track_number":2,"artist_id":artist,"asset_id":asset,"row_version":rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let track2 = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
    let rv = row_version(&pool, release).await;
    let (s, v) = call(
        &app,
        "PUT",
        &format!(
            "/api/orgs/{}/releases/{release}/tracks/{track2}/credits",
            u.org
        ),
        json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"}]}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    sqlx::query("UPDATE catalog.tracks SET isrc='USABC2600001' WHERE release_id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-dupisrc").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_CORRECTION");
    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='ISRC_DUPLICATE' AND status='CORRECTION_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "duplicate ISRC must force correction");
}

#[sqlx::test]
async fn stage1_malformed_upc_rejected(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query(
        "UPDATE catalog.releases SET upc='012345678904', row_version=row_version+1 WHERE id=$1",
    )
    .bind(release)
    .execute(&pool)
    .await
    .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-badupc").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_CORRECTION");
    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='UPC_FORMAT_INVALID' AND status='CORRECTION_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "malformed UPC must force correction");
}

#[sqlx::test]
async fn stage1_bad_release_date_rejected(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.releases SET draft = draft || '{\"release_date\":\"next friday\"}'::jsonb, row_version = row_version + 1 WHERE id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-baddate").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_CORRECTION");
    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='FIELD_RELEASE_DATE_INVALID' AND status='CORRECTION_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "bad release_date format must force correction");
}

#[sqlx::test]
async fn submit_with_expired_consent_rejected(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    // Age the consent past its validity window without touching the scope.
    // The immutability trigger is disabled inside this test's transaction only.
    sqlx::query("ALTER TABLE catalog.consent_packages DISABLE TRIGGER immutable")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.consent_packages SET body = body || '{\"valid_until\":\"2020-01-01T00:00:00Z\"}'::jsonb WHERE id=$1")
        .bind(consent_id)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = submit(&app, &u, release, consent_id, false, "k-expired").await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "CONSENT_EXPIRED", "{v}");
}

#[sqlx::test]
async fn stage1_corrupt_audio_goes_to_correction(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let bad = vec![0xDE, 0xAD, 0xBE, 0xEF, 1, 2, 3, 4, 5, 6, 7, 8];
    let asset = register_asset(&pool, &store, &u, "bad.bin", &bad).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-bad").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_CORRECTION");

    let row: (String, String) = sqlx::query_as(
        "SELECT check_code, status FROM operations.check_results WHERE revision_id=$1 AND check_code='AUDIO_MAGIC_MISMATCH'",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            "AUDIO_MAGIC_MISMATCH".to_string(),
            "CORRECTION_REQUIRED".to_string()
        )
    );

    let qc: String = sqlx::query_scalar("SELECT qc_status FROM catalog.assets WHERE id=$1")
        .bind(asset)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(qc, "BLOCKED");
}

#[sqlx::test]
async fn unchanged_audio_is_not_reanalyzed(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    // one asset, shared by two releases
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;

    let (release_a, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release_a, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = submit(
        &app,
        &u,
        release_a,
        Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap(),
        false,
        "k-a",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    run_worker(&pool, &store).await;
    let rev_a = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();
    let downloads_after_a = store.get_calls.load(Ordering::SeqCst);
    assert_eq!(downloads_after_a, 1, "one asset downloaded for analysis");

    // Second release, same bytes. Its asset checks must come from cache.
    let (release_b, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release_b, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = submit(
        &app,
        &u,
        release_b,
        Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap(),
        false,
        "k-b",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let rev_b = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();
    assert_ne!(rev_a, rev_b);
    run_worker(&pool, &store).await;

    assert_eq!(
        store.get_calls.load(Ordering::SeqCst),
        downloads_after_a,
        "no re-download for unchanged audio"
    );
    let cached: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations.check_results WHERE revision_id=$1 AND rule_version=$2 AND check_code IN ('SHA256_MISMATCH','AUDIO_MAGIC_MISMATCH','AUDIO_PROBE_FAILED','AUDIO_TOO_SHORT','AUDIO_SAMPLE_RATE_LOW','AUDIO_CHANNEL_INVALID') AND detail='cache_hit'",
    )
    .bind(rev_b)
    .bind(audeniq_core::qc::QC_RULE_VERSION)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cached, 6, "all 6 audio checks served from cache");
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release_b)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_PASSED");
}

#[sqlx::test]
async fn submit_idempotency_key_dedups(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();

    let (s, v1) = submit(&app, &u, release, consent_id, false, "k-dedupe").await;
    assert_eq!(s, StatusCode::OK, "{v1}");
    assert_eq!(v1["deduped"], false);
    // Retry the identical request with the same key: converges, never duplicates.
    let (s, v2) = submit(&app, &u, release, consent_id, false, "k-dedupe").await;
    assert_eq!(s, StatusCode::OK, "{v2}");
    assert_eq!(v2["deduped"], true);
    assert_eq!(v2["revision_id"], v1["revision_id"]);

    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM catalog.application_revisions WHERE release_id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1, "one revision for the retried submit");
}

#[sqlx::test]
async fn submit_idempotency_key_reused_with_changed_body(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_a = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, _) = submit(&app, &u, release, consent_a, false, "k-reuse").await;
    assert_eq!(s, StatusCode::OK);

    // A second consent package over the same draft changes the revision body
    // (consent_package_hash). Reusing the key must be rejected loudly, never
    // silently fork the application history.
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_b = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    assert_ne!(consent_a, consent_b);
    let (s, v) = submit(&app, &u, release, consent_b, false, "k-reuse").await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "IDEMPOTENCY_KEY_REUSED");
}

#[sqlx::test]
async fn oversized_asset_is_technical_retry(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-big").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    // The store reports an absurd size: the worker must skip analysis per-asset
    // (TECHNICAL_RETRY) and requeue the job instead of dying or downloading.
    let big: Arc<dyn ObjectStore> = Arc::new(InflatedStore {
        inner: store.clone(),
    });
    run_worker_on(&pool, &big, false).await;

    assert_eq!(
        store.get_calls.load(Ordering::SeqCst),
        0,
        "oversized object never downloaded"
    );
    let retry: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations.check_results WHERE revision_id=$1 AND status='TECHNICAL_RETRY' AND detail LIKE 'object too large%'",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        retry as usize,
        audeniq_core::qc::AUDIO_CHECK_CODES.len(),
        "every audio check recorded as technical retry"
    );
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        status, "STAGE1_RUNNING",
        "release stays in-flight for the retry"
    );
}

#[sqlx::test]
async fn fingerprint_flags_byte_identical_reupload_for_review(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav_a = make_sine_wav(dir.path(), "a.wav", 440);
    let wav_b = make_sine_wav(dir.path(), "b.wav", 880);
    let store_obj: Arc<dyn ObjectStore> = store.clone();

    async fn submit_one(
        app: &Router,
        pool: &PgPool,
        store: &Arc<FileStore>,
        u: &User,
        name: &str,
        bytes: &[u8],
        idem: &str,
    ) -> Uuid {
        let asset = register_asset(pool, store, u, name, bytes).await;
        let (release, _) = build_submittable(app, pool, store, u, asset).await;
        let (s, v) = consent(app, u, release, false).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
        let (s, v) = submit(app, u, release, consent_id, false, idem).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap()
    }
    async fn fp_status(pool: &PgPool, revision: Uuid) -> (String, String) {
        sqlx::query_as(
            "SELECT status, detail FROM operations.check_results WHERE revision_id=$1 AND check_code='AUDIO_SIMILAR_TO_EXISTING'",
        )
        .bind(revision)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    // First upload: nothing in the catalog to match -> PASS.
    let rev_a = submit_one(&app, &pool, &store, &u, "a.wav", &wav_a, "k-fp-a").await;
    run_worker_on(&pool, &store_obj, true).await;
    let (st, _) = fp_status(&pool, rev_a).await;
    assert_eq!(st, "PASS", "first upload has nothing to match");

    // Same bytes under a new asset: BER 0 -> REVIEW_REQUIRED, never blocked.
    let rev_b = submit_one(&app, &pool, &store, &u, "a2.wav", &wav_a, "k-fp-b").await;
    run_worker_on(&pool, &store_obj, true).await;
    let (st, detail) = fp_status(&pool, rev_b).await;
    assert_eq!(
        st, "REVIEW_REQUIRED",
        "byte-identical re-upload flagged: {detail}"
    );
    assert!(
        detail.contains("BER=0.000"),
        "identical bytes score BER 0: {detail}"
    );

    // Different recording: no match -> PASS.
    let rev_c = submit_one(&app, &pool, &store, &u, "b.wav", &wav_b, "k-fp-c").await;
    run_worker_on(&pool, &store_obj, true).await;
    let (st, _) = fp_status(&pool, rev_c).await;
    assert_eq!(st, "PASS", "different recording does not match");

    // The fingerprint rows are persisted for future comparisons.
    // asset_fingerprints is FORCE RLS: authorize the read via app.org_id.
    let mut rtx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(u.org.to_string())
        .execute(&mut *rtx)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM catalog.asset_fingerprints")
        .fetch_one(&mut *rtx)
        .await
        .unwrap();
    rtx.rollback().await.unwrap();
    assert_eq!(n, 3, "one fingerprint row per analyzed asset");
}

#[sqlx::test]
async fn stage2_job_is_executed_not_parked(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, _) = submit(&app, &u, release, consent_id, false, "k-park").await;
    assert_eq!(s, StatusCode::OK);
    run_worker(&pool, &store).await;

    // F3: the rights queue now has a real Stage 2 handler. The handoff job is
    // executed to a decision, not parked.
    let job = operations::claim(&pool, "rights", "test-worker", 60)
        .await
        .unwrap()
        .expect("stage2 job queued");
    assert_eq!(job.kind, "stage2");
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    operations::execute(&pool, &dyn_store, &job).await.unwrap();
    let row: (String, i32) =
        sqlx::query_as("SELECT status, attempts FROM operations.jobs WHERE id=$1")
            .bind(job.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "SUCCEEDED");
    assert_eq!(row.1, 1, "one claim, succeeded first try");
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE2_PASSED");
}

/// Spotify Metadata Style Guide 8.1: track titles must be unique within a
/// product unless they are different versions of the same track.
#[sqlx::test]
async fn stage1_duplicate_track_title_rejected(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, artist) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query(
        "UPDATE catalog.releases SET release_type='EP', row_version=row_version+1 WHERE id=$1",
    )
    .bind(release)
    .execute(&pool)
    .await
    .unwrap();
    let rv = row_version(&pool, release).await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
        json!({"title":"T1","disc_number":1,"track_number":2,"artist_id":artist,"asset_id":asset,"row_version":rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-duptitle").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_CORRECTION");
    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='TRACK_TITLE_DUPLICATE' AND status='CORRECTION_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "duplicate track title must force correction");
}

/// Spotify 8.2/8.4: version info belongs in the version field, not the title.
#[sqlx::test]
async fn stage1_title_version_info_flagged(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, artist) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.tracks SET title='Midnight (Radio Edit)' WHERE release_id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();
    let _ = artist;

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-verinfo").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='TRACK_TITLE_HAS_VERSION_INFO' AND status='REVIEW_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "version info in title must route to review");
}

/// Spotify 8.9: SEO terms in titles risk removal/strike — human review.
#[sqlx::test]
async fn stage1_title_seo_spam_flagged(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.tracks SET title='Deep Sleep Music' WHERE release_id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-seospam").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='TRACK_TITLE_SEO_SPAM' AND status='REVIEW_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "SEO terms in title must route to review");
}

/// DDEX ERN requires P-line/C-line: missing lines block at submit time now,
/// not at prepare time.
#[sqlx::test]
async fn stage1_missing_pline_cline_rejected(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.releases SET draft = draft - 'p_line' - 'c_line', row_version = row_version + 1 WHERE id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-noline").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    for code in ["PLINE_MISSING", "CLINE_MISSING"] {
        let hit: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code=$2 AND status='CORRECTION_REQUIRED')",
        )
        .bind(revision_id)
        .bind(code)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(hit, "{code} must force correction");
    }
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_CORRECTION");
}

/// Deezer requires a composer/lyricist credit per track; presence is
/// objective and blocks, name authenticity stays human review.
#[sqlx::test]
async fn stage1_writer_credit_missing_rejected(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    // Strip the writer credit, keep the performer credit.
    let track: Uuid = sqlx::query_scalar("SELECT id FROM catalog.tracks WHERE release_id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM catalog.credits WHERE track_id=$1 AND role='COMPOSER'")
        .bind(track)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-nowriter").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='TRACK_WRITER_CREDIT_MISSING' AND status='CORRECTION_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "missing writer credit must force correction");
}

/// Explicit content: the flag is recorded and the 19금 marking step is
/// surfaced for review, but the release is NOT blocked — harmfulness is a
/// human judgment.
#[sqlx::test]
async fn stage1_explicit_content_review_does_not_block(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.tracks SET parental_advisory=true WHERE release_id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-explicit").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='ADULT_MARKING_REVIEW' AND status='REVIEW_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "explicit content must surface the 19금 marking review");
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_PASSED", "review-only flags must not block");
}

/// Review-only flags (version info in title) no longer block the release:
/// they are recorded for the Stage 2 human reviewer.
#[sqlx::test]
async fn stage1_review_flags_do_not_block(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let (release, _) = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.tracks SET title='Midnight (Radio Edit)' WHERE release_id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-reviewpass").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    run_worker(&pool, &store).await;

    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='TRACK_TITLE_HAS_VERSION_INFO' AND status='REVIEW_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(hit, "version info in title must be flagged for review");
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "STAGE1_PASSED", "review-only flags must not block");
}

/// The version field travels from the track API into the revision body, and
/// identical title + identical version is still a duplicate.
#[sqlx::test]
async fn stage1_track_version_round_trip(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(dir.path());
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = create_release(&app, &u).await;
    let artist = create_artist(&app, &u).await;
    sqlx::query("UPDATE catalog.releases SET draft = draft || '{\"release_date\":\"2027-03-01\",\"p_line\":\"℗ 2027 T\",\"c_line\":\"© 2027 T\"}'::jsonb, row_version = row_version + 1 WHERE id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();
    let rv = row_version(&pool, release).await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
        json!({"title":"Midnight","version":"Radio Edit","disc_number":1,"track_number":1,"artist_id":artist,"asset_id":asset,"row_version":rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let track = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
    let rv = row_version(&pool, release).await;
    let (s, v) = call(
        &app,
        "PUT",
        &format!(
            "/api/orgs/{}/releases/{release}/tracks/{track}/credits",
            u.org
        ),
        json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"COMPOSER"}]}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let (s, v) = consent(&app, &u, release, false).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = submit(&app, &u, release, consent_id, false, "k-version").await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();

    let body: serde_json::Value =
        sqlx::query_scalar("SELECT body FROM catalog.application_revisions WHERE id=$1")
            .bind(revision_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(body["tracks"][0]["version"], "Radio Edit");
    // Plain title carries no version info -> no review flag.
    run_worker(&pool, &store).await;
    let hit: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM operations.check_results WHERE revision_id=$1 AND check_code='TRACK_TITLE_HAS_VERSION_INFO' AND status='REVIEW_REQUIRED')",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!hit, "version in the version field must not flag the title");
}
