//! F5 MockDSP + execution integration tests: DSP-01..DSP-12.
//!
//! Real Postgres, full worker pipeline stage1 -> stage2 -> prepare_release
//! -> delivery (E-0..E-5) against the scripted MockDSP. Each test proves one
//! BLUEPRINT 16.3/16.4/17.1 behavior; DSP-07 proves the zero-duplicate-send
//! invariant by key count on the mock.
use async_trait::async_trait;
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    error::{Error, Result},
    execution,
    mockdsp::{MockBehavior, MockDsp},
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
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-only-service-secret-32-characters";
const ORIGIN: &str = "http://localhost:5173";

struct FileStore {
    dir: std::path::PathBuf,
    get_calls: AtomicUsize,
}
impl FileStore {
    fn path_for(&self, key: &str) -> std::path::PathBuf {
        // Sanitize key to a safe filename.
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '.' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(safe)
    }
    async fn put(&self, key: &str, bytes: &[u8], content_type: &str) {
        let path = self.path_for(key);
        tokio::fs::write(&path, bytes).await.unwrap();
        let meta_path = self.dir.join(
            self.path_for(key)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string()
                + ".ct",
        );
        tokio::fs::write(&meta_path, content_type).await.unwrap();
    }
}
impl Default for FileStore {
    fn default() -> Self {
        // Disk-backed store root: AUDENIQ_TEST_STORE_DIR, else the system temp
        // dir (a hard-coded developer home path fails on CI and other hosts).
        let root = std::env::var_os("AUDENIQ_TEST_STORE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("audeniq-test-stores"));
        let dir = root.join(format!("audeniq-test-store-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self {
            dir,
            get_calls: AtomicUsize::new(0),
        }
    }
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
        let path = self.path_for(key);
        match tokio::fs::metadata(&path).await {
            Ok(m) => {
                let ct_path = self
                    .dir
                    .join(path.file_name().unwrap().to_str().unwrap().to_string() + ".ct");
                let ct = tokio::fs::read_to_string(&ct_path)
                    .await
                    .unwrap_or_default();
                Ok(Some(ObjectMeta {
                    size: m.len() as i64,
                    content_type: ct,
                    nonce: String::new(),
                    etag: String::new(),
                }))
            }
            Err(_) => Ok(None),
        }
    }
    async fn freeze(&self, _source: &str, _target: &str, _etag: &str) -> Result<()> {
        Err(Error::Storage)
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        self.get_calls.fetch_add(1, Ordering::SeqCst);
        tokio::fs::read(self.path_for(key))
            .await
            .map_err(|_| Error::Storage)
    }
}

async fn app(pool: PgPool) -> (Router, Arc<FileStore>) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(FileStore::default());
    let s = AppState::new(
        pool,
        Config {
            database_url: "postgres://f2test:f2test-local-dev-only@localhost/audeniq_f2".into(),
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

/// 3.5-minute audio fixtures for realistic stress tests. Generated with
/// ffmpeg on first use (under `AUDENIQ_STRESS_FIXTURE_DIR`, default
/// `temp_dir()/audeniq-stress`) so the test runs on any host and in CI
/// instead of depending on files pre-seeded on one developer machine.
fn stress_fixture(name: &str, sample_rate: u32) -> Vec<u8> {
    let dir = std::env::var_os("AUDENIQ_STRESS_FIXTURE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("audeniq-stress"));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(name);
    if !out.exists() {
        // Write to a unique temp name and rename, so parallel test binaries
        // never read a half-written file.
        let tmp = dir.join(format!("{name}.{}.tmp", std::process::id()));
        let st = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=210",
                "-ar",
                &sample_rate.to_string(),
                "-ac",
                "1",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
            ])
            .arg(&tmp)
            .status()
            .expect("ffmpeg runs");
        assert!(st.success(), "ffmpeg generated {name}");
        std::fs::rename(&tmp, &out).unwrap();
    }
    std::fs::read(&out).expect("stress fixture readable")
}
fn long_wav_bytes() -> Vec<u8> {
    stress_fixture("normal_35min_mono.wav", 44_100)
}
fn long_lowrate_wav_bytes() -> Vec<u8> {
    stress_fixture("problematic_35min_mono.wav", 22_050)
}

fn wav_bytes() -> &'static [u8] {
    static ONCE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        // QC flags audio under 30s as suspiciously short (MIN_AUDIO_SECS):
        // the shared fixture is 32s like the F4 suite. Generated once per
        // test binary; each test still gets its own database.
        let dir = std::env::temp_dir().join("audeniq-f5-shared");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("t32.wav");
        if !out.exists() {
            let st = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=32",
                    "-ar",
                    "48000",
                    "-ac",
                    "2",
                    "-c:a",
                    "pcm_s16le",
                ])
                .arg(&out)
                .status()
                .expect("ffmpeg runs");
            assert!(st.success());
        }
        std::fs::read(&out).unwrap()
    })
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
    store.put(&key, bytes, "audio/wav").await;
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(org)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    for action in ["read", "write"] {
        sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action) VALUES($1,$2,$3,$4)")
            .bind(org).bind(id).bind(u.party).bind(action)
            .execute(pool).await.unwrap();
    }
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,sha256,state) VALUES($1,$2,'AUDIO',$3,$4,'audio/wav',$5,'REGISTERED')")
        .bind(id).bind(org).bind(&key).bind(bytes.len() as i64).bind(sha256_hex(bytes))
        .execute(pool).await.unwrap();
    id
}

async fn row_version(pool: &PgPool, release: Uuid) -> i64 {
    sqlx::query_scalar("SELECT row_version FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn build_submittable(app: &Router, pool: &PgPool, u: &User, asset: Uuid) -> Uuid {
    let release = create_release(app, u).await;
    let artist = create_artist(app, u).await;
    sqlx::query("UPDATE catalog.releases SET draft = draft || '{\"release_date\":\"2027-03-01\"}'::jsonb, row_version = row_version + 1 WHERE id=$1")
        .bind(release).execute(pool).await.unwrap();
    let rv = row_version(pool, release).await;
    let (s, v) = call(
        app, "POST",
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
    release
}

async fn consent_and_submit(app: &Router, u: &User, release: Uuid, key: &str) -> Uuid {
    consent_and_submit_decl(app, u, release, key, json!({})).await
}

/// Submit with declaration overrides merged over the all-false baseline.
async fn consent_and_submit_decl(
    app: &Router,
    u: &User,
    release: Uuid,
    key: &str,
    decl_overrides: Value,
) -> Uuid {
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/consents", u.org),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":false}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let mut decl = json!({"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false});
    for (k, val) in decl_overrides.as_object().unwrap() {
        decl[k] = val.clone();
    }
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/submit", u.org),
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":key,"declarations":decl}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap()
}

async fn run_one(pool: &PgPool, store: &Arc<FileStore>, queue: &str, kind: &str) -> String {
    let job = operations::claim(pool, queue, "test-worker", 60)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{kind} job queued"));
    assert_eq!(job.kind, kind);
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    operations::execute(pool, &dyn_store, &job).await.unwrap();
    sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
        .bind(job.id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn release_status(pool: &PgPool, release: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn add_preparation_supplements(
    pool: &PgPool,
    store: &Arc<FileStore>,
    u: &User,
    release: Uuid,
) {
    let art_id = Uuid::new_v4();
    let art_key = format!("registered/{}/cover-{}.png", u.org, art_id);
    let art_bytes = b"\x89PNGfake";
    store.put(&art_key, art_bytes, "image/png").await;
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(u.org)
        .bind(art_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,sha256,state) VALUES($1,$2,'IMAGE',$3,$4,'image/png',$5,'REGISTERED')")
        .bind(art_id).bind(u.org).bind(&art_key).bind(art_bytes.len() as i64).bind(sha256_hex(art_bytes))
        .execute(pool).await.unwrap();
    // Identifiers are unique per release: Stage 1 refuses a UPC/ISRC already
    // used by another release in the org (IDENTIFIER_IN_USE), and several
    // tests here put many releases in one org.
    // Process-wide counter: concurrent tests/tasks never share a code.
    static NEXT_CODE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT_CODE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let (upc, isrc) = unique_codes(n);
    sqlx::query("UPDATE catalog.releases SET upc=$3, artwork_asset_id=$1, draft = draft || '{\"language\":\"ko\",\"artist\":\"Test Artist\",\"p_line\":\"P 2027 Test Label\",\"c_line\":\"C 2027 Test Label\"}'::jsonb, row_version = row_version + 1 WHERE id=$2")
        .bind(art_id)
        .bind(release)
        .bind(&upc)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc=$2 WHERE release_id=$1")
        .bind(release)
        .bind(&isrc)
        .execute(pool)
        .await
        .unwrap();
}

/// The n-th fixture UPC (valid GS1 check digit) and ISRC; n=0 gives the
/// historic fixture values 036000291452 / USABC2600001.
fn unique_codes(n: u32) -> (String, String) {
    let body = if n == 0 {
        "03600029145".to_string()
    } else {
        format!("036{n:08}")
    };
    let sum: u32 = body
        .chars()
        .enumerate()
        .map(|(i, c)| c.to_digit(10).unwrap() * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    let upc = format!("{body}{}", (10 - sum % 10) % 10);
    (upc, format!("USABC26{:05}", n + 1))
}

struct ReadyCtx {
    org: Uuid,
    release: Uuid,
    package_id: Uuid,
    store: Arc<FileStore>,
}

/// Full pipeline to READY_FOR_DELIVERY, then pin the mock profile to the
async fn ready_package(pool: &PgPool) -> ReadyCtx {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let wav = wav_bytes();
    let asset = register_asset(pool, &store, &u, "t.wav", wav).await;
    let release = build_submittable(&app, pool, &u, asset).await;
    add_preparation_supplements(pool, &store, &u, release).await;
    consent_and_submit(&app, &u, release, &format!("k-f5-{}", Uuid::new_v4())).await;
    // Pin the seeded MockDSP profile to a fixed DSP id BEFORE stage2 runs:
    // stage2's DSP-eligibility module reads activated adapter profiles, so
    // the frozen route plan contains the mock and E-0 can enqueue for it.
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(run_one(pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(run_one(pool, &store, "rights", "stage2").await, "SUCCEEDED");
    assert_eq!(
        run_one(pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(pool, release).await, "READY_FOR_DELIVERY");
    let package_id: Uuid = sqlx::query_scalar(
        "SELECT dp.id FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         WHERE cr.release_id=$1",
    )
    .bind(release)
    .fetch_one(pool)
    .await
    .unwrap();
    // The frozen route plan must contain exactly the mock DSP item.
    let plan_dsp: String = sqlx::query_scalar(
        "SELECT route_plan->0->'scope'->>'dsp_id' FROM distribution.preparation_artifacts WHERE package_id=$1",
    )
    .bind(package_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(plan_dsp, mock_dsp.to_string());
    ReadyCtx {
        org: u.org,
        release,
        package_id,
        store,
    }
}

/// Session-authorized connection for the RLS-protected execution tables.
async fn authed(pool: &PgPool, org: Uuid) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut c = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,false)")
        .bind(org.to_string())
        .execute(&mut *c)
        .await
        .unwrap();
    c
}

async fn job_status(pool: &PgPool, org: Uuid, job_id: Uuid) -> String {
    let mut c = authed(pool, org).await;
    sqlx::query_scalar("SELECT status FROM execution.delivery_jobs WHERE id=$1")
        .bind(job_id)
        .fetch_one(&mut *c)
        .await
        .unwrap()
}

async fn attempt_outcome(pool: &PgPool, org: Uuid, job_id: Uuid) -> String {
    let mut c = authed(pool, org).await;
    sqlx::query_scalar(
        "SELECT outcome FROM execution.delivery_attempts WHERE job_id=$1 ORDER BY attempt_no DESC LIMIT 1",
    )
    .bind(job_id)
    .fetch_one(&mut *c)
    .await
    .unwrap()
}

async fn live_status(pool: &PgPool, org: Uuid, package_id: Uuid) -> String {
    let mut c = authed(pool, org).await;
    sqlx::query_scalar(
        "SELECT live_status FROM execution.live_bindings WHERE package_id=$1 AND partner_id='mockdsp'",
    )
    .bind(package_id)
    .fetch_one(&mut *c)
    .await
    .unwrap()
}

async fn case_exists(pool: &PgPool, org: Uuid, job_id: Uuid, kind: &str) -> bool {
    let mut c = authed(pool, org).await;
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM execution.reconciliation_cases WHERE job_id=$1 AND kind=$2 AND status='OPEN')",
    )
    .bind(job_id)
    .bind(kind)
    .fetch_one(&mut *c)
    .await
    .unwrap()
}

/// Enqueue + claim + run one delivery against the given mock.
async fn run_send(pool: &PgPool, ctx: &ReadyCtx, mock: &MockDsp) -> (Uuid, String) {
    let (job_ids, org) = execution::enqueue_delivery_jobs(pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(job_ids.len(), 1, "one eligible DSP");
    assert_eq!(org, ctx.org);
    let job = execution::claim_delivery_job(pool, org, "mockdsp", "test-worker", 60)
        .await
        .unwrap()
        .expect("delivery job claimed");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    let status = execution::run_delivery(pool, &dyn_store, mock, &job)
        .await
        .unwrap();
    (job.id, status)
}

#[sqlx::test]
async fn dsp_01_accept_happy_path_goes_live(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "DELIVERED");
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "DELIVERED");
    assert_eq!(attempt_outcome(&pool, ctx.org, job_id).await, "ACCEPTED");
    assert_eq!(
        live_status(&pool, ctx.org, ctx.package_id).await,
        "INGESTING"
    );

    // Partner webhook: release went live.
    let pmid: String = {
        let mut c = authed(&pool, ctx.org).await;
        sqlx::query_scalar(
            "SELECT partner_message_id FROM execution.delivery_attempts WHERE job_id=$1",
        )
        .bind(job_id)
        .fetch_one(&mut *c)
        .await
        .unwrap()
    };
    let payload = mock.emit_webhook(&pmid, "live");
    assert_eq!(
        execution::ingest_ack(&pool, ctx.org, &mock, &payload)
            .await
            .unwrap(),
        "APPLIED"
    );
    assert_eq!(live_status(&pool, ctx.org, ctx.package_id).await, "LIVE");
    // No reconciliation case on the happy path.
    assert!(!case_exists(&pool, ctx.org, job_id, "MISSING_ACK").await);
}

#[sqlx::test]
async fn dsp_02_reject_fails_job_and_opens_case(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Reject {
        code: "MOCK_BAD_METADATA".into(),
    });
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "FAILED");
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "FAILED");
    assert_eq!(attempt_outcome(&pool, ctx.org, job_id).await, "REJECTED");
    assert!(case_exists(&pool, ctx.org, job_id, "PARTNER_REJECTED").await);
}

#[sqlx::test]
async fn dsp_03_timeout_parks_without_retry(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Timeout);
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "AWAITING_RECONCILIATION");
    assert_eq!(attempt_outcome(&pool, ctx.org, job_id).await, "TIMEOUT");
    assert!(case_exists(&pool, ctx.org, job_id, "SENT_UNKNOWN").await);
    assert_eq!(mock.received().len(), 1);

    // A second worker run must NOT send again: the unresolved attempt is
    // reconciled, never re-sent.
    {
        let mut c = authed(&pool, ctx.org).await;
        sqlx::query("UPDATE execution.delivery_jobs SET status='QUEUED', lock_token=NULL, lease_until=NULL WHERE id=$1")
            .bind(job_id)
            .execute(&mut *c)
            .await
            .unwrap();
    }
    let job = execution::claim_delivery_job(&pool, ctx.org, "mockdsp", "test-worker-2", 60)
        .await
        .unwrap()
        .expect("re-claimed");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    let status = execution::run_delivery(&pool, &dyn_store, &mock, &job)
        .await
        .unwrap();
    assert_eq!(status, "AWAITING_RECONCILIATION");
    assert_eq!(mock.received().len(), 1, "no second wire call");
    for (key, count) in mock.key_counts() {
        assert_eq!(count, 1, "key {key} sent exactly once");
    }
}

#[sqlx::test]
async fn dsp_04_unknown_resolved_by_explicit_inquiry(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Unknown);
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "AWAITING_RECONCILIATION");
    assert_eq!(attempt_outcome(&pool, ctx.org, job_id).await, "UNKNOWN");

    // Explicit inquiry (never an automatic re-send) reconciles by
    // idempotency key: the partner did receive it.
    let resolved = execution::resolve_unknown(&pool, ctx.org, &mock, job_id)
        .await
        .unwrap();
    assert_eq!(resolved, "DELIVERED");
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "DELIVERED");
    assert_eq!(attempt_outcome(&pool, ctx.org, job_id).await, "ACCEPTED");
    assert_eq!(mock.received().len(), 1, "inquiry is not a re-send");
}

#[sqlx::test]
async fn dsp_05_webhook_duplicate_applies_once(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_id, _) = run_send(&pool, &ctx, &mock).await;
    let pmid: String = {
        let mut c = authed(&pool, ctx.org).await;
        sqlx::query_scalar(
            "SELECT partner_message_id FROM execution.delivery_attempts WHERE job_id=$1",
        )
        .bind(job_id)
        .fetch_one(&mut *c)
        .await
        .unwrap()
    };
    let payload = mock.emit_webhook(&pmid, "accepted");
    assert_eq!(
        execution::ingest_ack(&pool, ctx.org, &mock, &payload)
            .await
            .unwrap(),
        "APPLIED"
    );
    // The partner redelivers the same event: acknowledged, not re-applied.
    assert_eq!(
        execution::ingest_ack(&pool, ctx.org, &mock, &payload)
            .await
            .unwrap(),
        "DUPLICATE_IGNORED"
    );
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "DELIVERED");
}

#[sqlx::test]
async fn dsp_06_delayed_live_resolves_via_poll(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::DelayedLive {
        polls_before_live: 2,
    });
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "DELIVERED");
    assert_eq!(
        execution::poll_live(&pool, ctx.org, &mock, ctx.package_id, "mockdsp")
            .await
            .unwrap(),
        "INGESTING"
    );
    assert_eq!(
        live_status(&pool, ctx.org, ctx.package_id).await,
        "INGESTING"
    );
    assert_eq!(
        execution::poll_live(&pool, ctx.org, &mock, ctx.package_id, "mockdsp")
            .await
            .unwrap(),
        "LIVE"
    );
    assert_eq!(live_status(&pool, ctx.org, ctx.package_id).await, "LIVE");
    let _ = job_id;
}

#[sqlx::test]
async fn dsp_07_zero_duplicate_sends(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Accept);
    // Two packages would double the pipeline cost; instead prove the
    // invariant across crash-replay paths on one job (see dsp_03) and
    // across a re-enqueue: E-0 never creates a second job for the same
    // (package, partner).
    let (job_ids, _) = execution::enqueue_delivery_jobs(&pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(job_ids.len(), 1);
    let (job_ids2, _) = execution::enqueue_delivery_jobs(&pool, ctx.package_id)
        .await
        .unwrap();
    assert!(job_ids2.is_empty(), "re-enqueue creates no duplicate job");
    // Run the already-enqueued job directly (run_send would enqueue a third
    // time and find nothing).
    let job = execution::claim_delivery_job(&pool, ctx.org, "mockdsp", "test-worker", 60)
        .await
        .unwrap()
        .expect("delivery job claimed");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    let status = execution::run_delivery(&pool, &dyn_store, &mock, &job)
        .await
        .unwrap();
    assert_eq!(status, "DELIVERED");
    for (key, count) in mock.key_counts() {
        assert_eq!(count, 1, "key {key} reached the wire exactly once");
    }
    // A replayed run after delivery must not create a second attempt.
    let mut c = authed(&pool, ctx.org).await;
    let attempts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution.delivery_attempts WHERE job_id=$1")
            .bind(job.id)
            .fetch_one(&mut *c)
            .await
            .unwrap();
    assert_eq!(attempts, 1);
}

#[sqlx::test]
async fn dsp_08_freshness_guard_blocks_stale_rights(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    // Rights changed after preparation: the epoch pinned in the canonical
    // snapshot no longer matches.
    sqlx::query("UPDATE rights.rights_epochs SET epoch=epoch+1 WHERE org_id=$1 AND release_id=$2")
        .bind(ctx.org)
        .bind(ctx.release)
        .execute(&pool)
        .await
        .unwrap();
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_ids, org) = execution::enqueue_delivery_jobs(&pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(job_ids.len(), 1);
    let job = execution::claim_delivery_job(&pool, org, "mockdsp", "test-worker", 60)
        .await
        .unwrap()
        .expect("claimed");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    let err = execution::run_delivery(&pool, &dyn_store, &mock, &job)
        .await
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("EXECUTION_RIGHTS_EPOCH_DRIFT"),
        "{err:?}"
    );
    assert_eq!(job_status(&pool, ctx.org, job.id).await, "FAILED");
    assert!(mock.received().is_empty(), "nothing reached the wire");
}

#[sqlx::test]
async fn dsp_09_takedown_lifecycle(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "DELIVERED");
    // Go live first so the takedown has something to take down.
    let pmid: String = {
        let mut c = authed(&pool, ctx.org).await;
        sqlx::query_scalar(
            "SELECT partner_message_id FROM execution.delivery_attempts WHERE job_id=$1",
        )
        .bind(job_id)
        .fetch_one(&mut *c)
        .await
        .unwrap()
    };
    let payload = mock.emit_webhook(&pmid, "live");
    execution::ingest_ack(&pool, ctx.org, &mock, &payload)
        .await
        .unwrap();
    assert_eq!(live_status(&pool, ctx.org, ctx.package_id).await, "LIVE");

    assert_eq!(
        execution::takedown_release(&pool, ctx.org, &mock, ctx.package_id, "mockdsp")
            .await
            .unwrap(),
        "TAKEDOWN_REQUESTED"
    );
    assert_eq!(
        live_status(&pool, ctx.org, ctx.package_id).await,
        "TAKEDOWN_REQUESTED"
    );
    let payload = mock.emit_webhook(&pmid, "takedown_confirmed");
    assert_eq!(
        execution::ingest_ack(&pool, ctx.org, &mock, &payload)
            .await
            .unwrap(),
        "APPLIED"
    );
    assert_eq!(
        live_status(&pool, ctx.org, ctx.package_id).await,
        "TAKEN_DOWN"
    );
}

#[sqlx::test]
async fn dsp_10_update_release_accepted(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Accept);
    let (_, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "DELIVERED");
    assert_eq!(
        execution::update_release(
            &pool,
            ctx.org,
            &mock,
            ctx.package_id,
            "mockdsp",
            &json!({"title": "T1 (Remastered)"}),
        )
        .await
        .unwrap(),
        "UPDATE_ACCEPTED"
    );
    assert!(
        mock.received()
            .iter()
            .any(|r| r.idempotency_key.ends_with(":update")),
        "update reached the wire"
    );
}

#[sqlx::test]
async fn dsp_11_lease_fencing_single_claimer(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let (job_ids, org) = execution::enqueue_delivery_jobs(&pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(job_ids.len(), 1);
    let first = execution::claim_delivery_job(&pool, org, "mockdsp", "worker-a", 60)
        .await
        .unwrap();
    assert!(first.is_some(), "worker A claims the job");
    let second = execution::claim_delivery_job(&pool, org, "mockdsp", "worker-b", 60)
        .await
        .unwrap();
    assert!(second.is_none(), "worker B cannot steal a live lease");
    // The specific-id lease path honors the same fence.
    let again = execution::lease_delivery_job(&pool, job_ids[0], org, "worker-b", 60)
        .await
        .unwrap();
    assert!(again.is_none(), "lease_delivery_job also fenced");
}

#[sqlx::test]
async fn dsp_12_capability_flags_gate_wire_calls(pool: PgPool) {
    use audeniq_core::execution::Capabilities;
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::with_capabilities(
        MockBehavior::Accept,
        Capabilities {
            takedown: false,
            ..Default::default()
        },
    );
    let err = execution::takedown_release(&pool, ctx.org, &mock, ctx.package_id, "mockdsp")
        .await
        .unwrap_err();
    assert!(matches!(err, audeniq_core::error::Error::Gated), "{err:?}");
    assert!(
        mock.received().is_empty(),
        "gated capability never reaches the wire"
    );
}

/// The operations dispatcher wiring: the durable READY_FOR_DELIVERY handoff
/// already enqueued delivery.enqueue in the same transaction that flipped the
/// release status, so the dispatcher only has to run it — it fans out
/// delivery.send jobs that the existing claim/execute loop drives to DELIVERED.
#[sqlx::test]
async fn dsp_ops_dispatcher_end_to_end(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    // No manual enqueue: the handoff did it. Assert the job exists and is queued.
    let queued: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations.jobs WHERE queue='delivery' AND kind='delivery.enqueue' AND status='QUEUED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(queued, 1);
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.enqueue").await,
        "SUCCEEDED"
    );
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.send").await,
        "SUCCEEDED"
    );
    let mut c = authed(&pool, ctx.org).await;
    let status: String = sqlx::query_scalar(
        "SELECT status FROM execution.delivery_jobs WHERE package_id=$1 AND partner_id='mockdsp'",
    )
    .bind(ctx.package_id)
    .fetch_one(&mut *c)
    .await
    .unwrap();
    assert_eq!(status, "DELIVERED");
    let _ = dyn_store;
}

/// Worker-role RLS integration: the delivery path (E-0 enqueue, claim,
/// E-2/E-3 send) runs as the non-owner `audeniq_worker` role with the deploy
/// grants applied, and RLS isolates tenants. This mirrors the production
/// runtime shape; the DSP-0x tests above run as the table owner.
#[sqlx::test]
async fn dsp_worker_role_rls_delivery(pool: PgPool) {
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
    let worker_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE audeniq_worker").execute(c).await?;
                Ok(())
            })
        })
        .connect_with((*pool.connect_options()).clone())
        .await
        .unwrap();

    let ctx = ready_package(&pool).await;

    // E-0 + claim + send, all as the worker role.
    let (job_ids, org) = execution::enqueue_delivery_jobs(&worker_pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(job_ids.len(), 1, "one eligible DSP");
    assert_eq!(org, ctx.org);
    let job = execution::claim_delivery_job(&worker_pool, org, "mockdsp", "rls-worker", 60)
        .await
        .unwrap()
        .expect("delivery job claimed");
    let mock = MockDsp::new(MockBehavior::Accept);
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    let status = execution::run_delivery(&worker_pool, &dyn_store, &mock, &job)
        .await
        .unwrap();
    assert_eq!(status, "DELIVERED");

    // Tenant isolation under the worker role: another org's job is invisible.
    let other_org = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.orgs(id, name, kind) VALUES($1,'other','LABEL')")
        .bind(other_org)
        .execute(&pool)
        .await
        .unwrap();
    let other_job = Uuid::new_v4();
    // FORCE RLS applies even to the owner: authorize the insert's org in a
    // transaction-local setting so the pooled connection isn't polluted.
    let mut otx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,true)")
        .bind(other_org.to_string())
        .execute(&mut *otx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO execution.delivery_jobs(id, org_id, package_id, partner_id) VALUES($1,$2,$3,'mockdsp')",
    )
    .bind(other_job)
    .bind(other_org)
    .bind(ctx.package_id)
    .execute(&mut *otx)
    .await
    .unwrap();
    otx.commit().await.unwrap();
    let mut wc = worker_pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,false)")
        .bind(ctx.org.to_string())
        .execute(&mut *wc)
        .await
        .unwrap();
    let visible: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM execution.delivery_jobs")
        .fetch_one(&mut *wc)
        .await
        .unwrap();
    assert_eq!(visible, 1, "worker sees only its own org's delivery jobs");
    let seen: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM execution.delivery_jobs WHERE id=$1)")
            .bind(other_job)
            .fetch_one(&mut *wc)
            .await
            .unwrap();
    assert!(!seen, "cross-org delivery job must be invisible");
}

/// Routing prefers the partner-specific DDEX interchange message over the
/// synthetic preparation envelope when one was persisted for the package+DSP.
#[sqlx::test]
async fn dsp_routing_prefers_partner_ddex_message(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let ddex_xml = "<ern:NewReleaseMessage xmlns:ern=\"http://ddex.net/xml/ern/382\">partner-ddex</ern:NewReleaseMessage>";
    let ddex_sha = format!("{:x}", sha2::Sha256::digest(ddex_xml.as_bytes()));
    let mut c = authed(&pool, ctx.org).await;
    sqlx::query("INSERT INTO distribution.ddex_messages(package_id,org_id,dsp_id,sender_name,sender_dpid,recipient_name,recipient_dpid,ern_xml,ern_sha256) VALUES($1,$2,$3,'s','SENDER-DPID-1','MockDSP','TESTDPID-MOCKDSP-0001',$4,$5)")
        .bind(ctx.package_id).bind(ctx.org).bind(mock_dsp).bind(ddex_xml).bind(&ddex_sha)
        .execute(&mut *c).await.unwrap();
    drop(c);
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_ids, org) = execution::enqueue_delivery_jobs(&pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(job_ids.len(), 1);
    let job = execution::claim_delivery_job(&pool, org, "mockdsp", "route-test", 60)
        .await
        .unwrap()
        .expect("delivery job claimed");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    let status = execution::run_delivery(&pool, &dyn_store, &mock, &job)
        .await
        .unwrap();
    assert_eq!(status, "DELIVERED");
    let got = mock.received();
    assert_eq!(got.len(), 1);
    assert_eq!(
        got[0].ern_sha256, ddex_sha,
        "wire must carry the partner DDEX message, not the synthetic envelope"
    );
}

/// A real transport with no persisted DDEX message fails closed: the
/// synthetic preparation envelope must never reach a real partner's wire.
#[sqlx::test]
async fn dsp_routing_fail_closed_without_ddex_message(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    sqlx::query(
        "UPDATE execution.adapter_profiles SET transport='sftp' WHERE partner_id='mockdsp'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_ids, org) = execution::enqueue_delivery_jobs(&pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(job_ids.len(), 1);
    let job = execution::claim_delivery_job(&pool, org, "mockdsp", "route-test", 60)
        .await
        .unwrap()
        .expect("delivery job claimed");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    let err = execution::run_delivery(&pool, &dyn_store, &mock, &job)
        .await
        .unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("EXECUTION_DDEX_MESSAGE_MISSING"), "{msg}");
    assert!(mock.received().is_empty(), "nothing reached the wire");
}

/// Activation model: a CONTRACTED profile with delivery_enabled=true but no
/// contract route must not enqueue a delivery job. delivery_enabled alone
/// is only the operator kill-switch; the contract route (route enabled +
/// endpoint ACTIVE + non-revoked contract revision) is the eligibility
/// proof. The MOCK profile in the same route plan still enqueues.
///
/// Note: preparation_artifacts is immutable by trigger (the frozen package
/// must never change), so this test disables the trigger to craft the
/// route plan, then re-enables it. The Stage 2 path is covered by
/// stage2_contracted_profile_not_eligible_without_contract, which goes
/// through the real pipeline.
#[sqlx::test]
async fn contracted_profile_cannot_enqueue_without_contract(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    let contracted_dsp = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO execution.adapter_profiles(partner_id, display_name, profile_version, dsp_id, delivery_enabled, transport, activation_kind)
         VALUES('contracted-test','Contracted Test Partner','1',$1,true,'sftp','CONTRACTED')",
    )
    .bind(contracted_dsp)
    .execute(&pool)
    .await
    .unwrap();
    // Route plan names both DSPs; only the MOCK one may produce a job.
    let route_plan = serde_json::json!([
        {"scope": {"dsp_id": mock_dsp.to_string()}},
        {"scope": {"dsp_id": contracted_dsp.to_string()}},
    ]);
    sqlx::query("ALTER TABLE distribution.preparation_artifacts DISABLE TRIGGER preparation_artifacts_immutable")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE distribution.preparation_artifacts SET route_plan=$1 WHERE package_id=$2")
        .bind(&route_plan)
        .bind(ctx.package_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE distribution.preparation_artifacts ENABLE TRIGGER preparation_artifacts_immutable")
        .execute(&pool)
        .await
        .unwrap();
    let (job_ids, org) = execution::enqueue_delivery_jobs(&pool, ctx.package_id)
        .await
        .unwrap();
    assert_eq!(org, ctx.org);
    assert_eq!(job_ids.len(), 1, "only the MOCK DSP enqueues");
    // delivery_jobs is FORCE RLS: read through an authorized connection.
    let mut c = authed(&pool, ctx.org).await;
    let partners: Vec<String> =
        sqlx::query_scalar("SELECT partner_id FROM execution.delivery_jobs WHERE package_id=$1")
            .bind(ctx.package_id)
            .fetch_all(&mut *c)
            .await
            .unwrap();
    assert_eq!(partners, vec!["mockdsp".to_string()]);
}

/// Sandbox end-to-end inspection of the distribution system.
///
/// Runs the FULL automatic pipeline
///   submit -> stage1 -> stage2 -> prepare_release -> delivery.enqueue
///   -> delivery.send -> MockDSP wire + ACK
/// against a real PostgreSQL database, with background worker loops running
/// the same claim/execute code as the audeniq-worker binary.
///
/// This is NOT a sqlx::test: it connects to an existing sandbox database
/// (migrations applied via audeniq-migrate, grants applied). It seeds its
/// own org/user/release, so reruns are safe.
///
/// Run:
///   SANDBOX_DATABASE_URL=postgres://f2test:<pw>@localhost/audeniq_sandbox \
///     cargo test -p audeniq-core --test execution sandbox_full_distribution_run \
///     -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn sandbox_full_distribution_run() {
    use std::time::{Duration, Instant};
    let db_url = std::env::var("SANDBOX_DATABASE_URL")
        .expect("set SANDBOX_DATABASE_URL to a migrated sandbox database");
    let pool = PgPool::connect(&db_url).await.expect("sandbox DB connect");
    database::MIGRATOR.run(&pool).await.expect("migrations");
    println!("[sandbox] connected, migrations ok");

    // ---- seed (same path as the unit fixtures) ----
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    // Sandbox reruns share one database while each run creates a new org:
    // the S2_CATALOG_IDENTIFIERS cross-org duplicate check would flag a
    // rerun, so this run gets unique audio bytes / UPC / ISRC.
    let run_tag = Uuid::new_v4().as_u128();
    let mut wav = wav_bytes().to_vec();
    wav.extend_from_slice(&run_tag.to_le_bytes());
    let asset = register_asset(&pool, &store, &u, "t.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    add_preparation_supplements(&pool, &store, &u, release).await;
    let upc_base = format!("{:011}", run_tag % 100_000_000_000u128);
    let mut sum = 0u32;
    for (i, b) in upc_base.bytes().enumerate() {
        let d = (b - b'0') as u32;
        sum += if i % 2 == 0 { 3 * d } else { d };
    }
    let upc = format!("{upc_base}{}", (10 - sum % 10) % 10);
    let isrc = format!("USSBX{:07}", run_tag % 10_000_000u128);
    sqlx::query("UPDATE catalog.releases SET upc=$1, row_version=row_version+1 WHERE id=$2")
        .bind(&upc)
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc=$1 WHERE release_id=$2")
        .bind(&isrc)
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE identity.orgs SET ddex_sender_dpid='TESTDPID-SANDBOX-0001' WHERE id=$1")
        .bind(u.org)
        .execute(&pool)
        .await
        .unwrap();
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(&pool)
        .await
        .unwrap();
    let key = format!("k-sandbox-{}", Uuid::new_v4());
    consent_and_submit(&app, &u, release, &key).await;
    println!("[sandbox] seeded release {release} (org {})", u.org);

    // ---- background worker loops: same claim/execute code as audeniq-worker ----
    for queue in ["qc", "rights", "distribution", "delivery"] {
        let pool = pool.clone();
        let store = store.clone();
        tokio::spawn(async move {
            let dyn_store: Arc<dyn ObjectStore> = store;
            loop {
                match operations::claim(&pool, queue, "sandbox-worker", 60).await {
                    Ok(Some(job)) => {
                        if let Err(e) = operations::execute(&pool, &dyn_store, &job).await {
                            eprintln!("[sandbox][{queue}] job {} error: {e:?}", job.id);
                            let _ = operations::fail(&pool, &job, false, "INTERNAL_HANDLER_ERROR")
                                .await;
                        }
                    }
                    Ok(None) => tokio::time::sleep(Duration::from_millis(300)).await,
                    Err(e) => {
                        eprintln!("[sandbox][{queue}] claim error: {e:?}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }

    let t0 = Instant::now();
    // ---- stage 1: release reaches READY_FOR_DELIVERY via the worker ----
    let deadline = Duration::from_secs(180);
    loop {
        if release_status(&pool, release).await == "READY_FOR_DELIVERY" {
            break;
        }
        if t0.elapsed() > deadline {
            panic!("[sandbox] TIMEOUT waiting for READY_FOR_DELIVERY");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    println!(
        "[sandbox] READY_FOR_DELIVERY after {:.1}s",
        t0.elapsed().as_secs_f32()
    );

    // ---- stage 2: DDEX interchange artifact persisted per partner ----
    let mut c = authed(&pool, u.org).await;
    let (xml, sha, sender, recipient): (String, String, String, String) = sqlx::query_as(
        "SELECT m.ern_xml, m.ern_sha256, m.sender_dpid, m.recipient_dpid
         FROM distribution.ddex_messages m
         JOIN distribution.distribution_packages dp ON dp.id=m.package_id
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         WHERE cr.release_id=$1",
    )
    .bind(release)
    .fetch_one(&mut *c)
    .await
    .expect("ddex_messages row");
    assert_eq!(sender, "TESTDPID-SANDBOX-0001");
    assert_eq!(recipient, "TESTDPID-MOCKDSP-0001");
    assert_eq!(sha, sha256_hex(xml.as_bytes()));
    println!("[sandbox] ddex_messages: 1 row, sha256 {sha} (verified)");

    // ---- stage 3: delivery.send runs the MockDSP wire path via the worker ----
    let t1 = Instant::now();
    loop {
        let st: Option<String> = sqlx::query_scalar(
            "SELECT status FROM execution.delivery_jobs WHERE org_id=$1 AND partner_id='mockdsp'",
        )
        .bind(u.org)
        .fetch_optional(&mut *c)
        .await
        .unwrap();
        if st.as_deref() == Some("DELIVERED") {
            break;
        }
        match &st {
            Some(s) if s == "FAILED" || s == "DEAD_LETTER" => {
                panic!("[sandbox] delivery job went {s}");
            }
            _ => {}
        }
        if t1.elapsed() > deadline {
            panic!("[sandbox] TIMEOUT waiting for DELIVERED (last: {st:?})");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    println!(
        "[sandbox] DELIVERED after {:.1}s",
        t1.elapsed().as_secs_f32()
    );
    let attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution.delivery_attempts a
         JOIN execution.delivery_jobs j ON j.id=a.job_id
         WHERE j.org_id=$1 AND j.partner_id='mockdsp'",
    )
    .bind(u.org)
    .fetch_one(&mut *c)
    .await
    .unwrap();
    println!("[sandbox] delivery_attempts: {attempts} wire attempt(s)");

    // ---- stage 4: live state ----
    let live: Option<String> = sqlx::query_scalar(
        "SELECT live_status FROM execution.live_bindings WHERE org_id=$1 AND partner_id='mockdsp'",
    )
    .bind(u.org)
    .fetch_optional(&mut *c)
    .await
    .unwrap();
    let poll_jobs: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM operations.jobs WHERE kind='delivery.poll'")
            .fetch_one(&pool)
            .await
            .unwrap();
    println!("[sandbox] live_bindings status: {live:?}");
    println!("[sandbox] delivery.poll jobs ever enqueued: {poll_jobs}");
    println!("[sandbox] DONE in {:.1}s total", t0.elapsed().as_secs_f32());
}

fn mp3_bytes() -> Vec<u8> {
    // Same musical content as wav_bytes (440Hz sine, 32s) but re-encoded as
    // MP3: different bytes, same music. This is the fingerprint-gap probe.
    static ONCE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let dir = std::env::temp_dir().join("audeniq-f5-shared");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("t32.mp3");
        if !out.exists() {
            let st = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=32",
                    "-ar",
                    "44100",
                    "-ac",
                    "2",
                    "-c:a",
                    "libmp3lame",
                    "-b:a",
                    "128k",
                ])
                .arg(&out)
                .status()
                .expect("ffmpeg runs");
            assert!(st.success());
        }
        std::fs::read(&out).unwrap()
    })
    .clone()
}

/// Seed one submittable release for adversarial scenarios, with run-unique
/// UPC/ISRC. Audio bytes are used as-is: callers must pass unique bytes per
/// scenario (except the deliberate byte-identical copy) so the cross-org
/// S2_CATALOG_IDENTIFIERS check does not flag unrelated scenarios.
async fn adversarial_seed(
    app: &Router,
    pool: &PgPool,
    store: &Arc<FileStore>,
    audio: &[u8],
    track_title: &str,
) -> (User, Uuid) {
    let u = user(app).await;
    let asset = register_asset(pool, store, &u, "t.wav", audio).await;
    let release = build_submittable(app, pool, &u, asset).await;
    add_preparation_supplements(pool, store, &u, release).await;
    let tag = Uuid::new_v4().as_u128();
    let upc_base = format!("{:011}", tag % 100_000_000_000u128);
    let mut sum = 0u32;
    for (i, b) in upc_base.bytes().enumerate() {
        let d = (b - b'0') as u32;
        sum += if i % 2 == 0 { 3 * d } else { d };
    }
    let upc = format!("{upc_base}{}", (10 - sum % 10) % 10);
    let isrc = format!("USSBX{:07}", tag % 10_000_000u128);
    sqlx::query("UPDATE catalog.releases SET upc=$1, row_version=row_version+1 WHERE id=$2")
        .bind(&upc)
        .bind(release)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc=$1, title=$2 WHERE release_id=$3")
        .bind(&isrc)
        .bind(track_title)
        .bind(release)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE identity.orgs SET ddex_sender_dpid='TESTDPID-SANDBOX-0001' WHERE id=$1")
        .bind(u.org)
        .execute(pool)
        .await
        .unwrap();
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(pool)
        .await
        .unwrap();
    (u, release)
}

async fn wait_release_status(
    pool: &PgPool,
    release: Uuid,
    want: &[&str],
    timeout_secs: u64,
) -> String {
    let deadline = std::time::Duration::from_secs(timeout_secs);
    let t0 = std::time::Instant::now();
    loop {
        let s = release_status(pool, release).await;
        if want.contains(&s.as_str()) {
            return s;
        }
        if t0.elapsed() > deadline {
            panic!("TIMEOUT waiting for {want:?}, last status: {s}");
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Adversarial submission scenarios (red-team inspection).
///
/// Each scenario drives a realistic abuse case through the real pipeline and
/// records whether the system catches it:
///   S1: byte-identical re-upload of another org's released audio (plagiarism)
///   S2: same music re-encoded as MP3 (fingerprint-gap probe)
///   S3: minor who declares minority at consent (must be hard-blocked)
///   S4: minor who lies (minority_declared=false) (age-verification gap probe)
///   S5: cover song, "(Cover)" in title but undeclared (tripwire probe)
///   S6: explicit lyrics + parental advisory (special-flag probe)
///   S7: declared cover via is_cover=true (special-flag probe)
///   S8: submit without rights/adult declarations (must be rejected)
///   S9: UPC already claimed by another org's live release (duplicate probe)
///
/// Run: SANDBOX_DATABASE_URL=... cargo test -p audeniq-core --test execution
///   sandbox_adversarial_submissions -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn sandbox_adversarial_submissions() {
    let db_url = std::env::var("SANDBOX_DATABASE_URL")
        .expect("set SANDBOX_DATABASE_URL to a migrated sandbox database");
    let pool = PgPool::connect(&db_url).await.expect("sandbox DB connect");
    database::MIGRATOR.run(&pool).await.expect("migrations");
    let (app, store) = app(pool.clone()).await;
    for queue in ["qc", "rights", "distribution", "delivery"] {
        let pool = pool.clone();
        let store = store.clone();
        tokio::spawn(async move {
            let dyn_store: Arc<dyn ObjectStore> = store;
            loop {
                match operations::claim(&pool, queue, "sandbox-worker", 60).await {
                    Ok(Some(job)) => {
                        if operations::execute(&pool, &dyn_store, &job).await.is_err() {
                            let _ = operations::fail(&pool, &job, false, "INTERNAL_HANDLER_ERROR")
                                .await;
                        }
                    }
                    Ok(None) => tokio::time::sleep(std::time::Duration::from_millis(300)).await,
                    Err(_) => tokio::time::sleep(std::time::Duration::from_secs(1)).await,
                }
            }
        });
    }

    // S1: the "original" release goes live first...
    fn unique_audio(base: &[u8]) -> Vec<u8> {
        let mut v = base.to_vec();
        v.extend_from_slice(&Uuid::new_v4().as_u128().to_le_bytes());
        v
    }
    let base_wav = wav_bytes().to_vec();
    let audio_s1a = unique_audio(&base_wav);
    let (_u1, r1) = adversarial_seed(&app, &pool, &store, &audio_s1a, "Original Song").await;
    consent_and_submit(&app, &_u1, r1, &format!("k-adv1-{}", Uuid::new_v4())).await;
    assert_eq!(
        wait_release_status(&pool, r1, &["READY_FOR_DELIVERY"], 180).await,
        "READY_FOR_DELIVERY"
    );
    println!("[adv] S1a original released: READY_FOR_DELIVERY");

    // ...then a different org re-uploads the exact same bytes as their own.
    let (u2, r2) = adversarial_seed(&app, &pool, &store, &audio_s1a, "My Original Song").await;
    consent_and_submit(&app, &u2, r2, &format!("k-adv2-{}", Uuid::new_v4())).await;
    let s2 = wait_release_status(&pool, r2, &["STAGE2_REVIEW", "READY_FOR_DELIVERY"], 180).await;
    println!("[adv] S1b byte-identical re-upload -> {s2}");
    assert_eq!(
        s2, "STAGE2_REVIEW",
        "byte-identical plagiarism must be caught"
    );

    // S2: same music, re-encoded (different bytes) — the fingerprint gap.
    // (Trailing tag bytes keep reruns unique; ffprobe/QC read the frames.)
    let mut mp3 = mp3_bytes();
    mp3.extend_from_slice(&Uuid::new_v4().as_u128().to_le_bytes());
    assert_ne!(sha256_hex(&audio_s1a), sha256_hex(&mp3));
    let (u3, r3) = adversarial_seed(&app, &pool, &store, &mp3, "Totally New Song").await;
    consent_and_submit(&app, &u3, r3, &format!("k-adv3-{}", Uuid::new_v4())).await;
    let s3 = wait_release_status(&pool, r3, &["STAGE2_REVIEW", "READY_FOR_DELIVERY"], 180).await;
    println!("[adv] S2 re-encoded same music -> {s3} (gap if READY_FOR_DELIVERY)");

    // S3: minor declares minority at consent -> hard block.
    let (u4, r4) =
        adversarial_seed(&app, &pool, &store, &unique_audio(&base_wav), "Kid Song").await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{r4}/consents", u4.org),
        json!({"parties":[{"party_id":u4.party,"role":"ARTIST"}],"minority_declared":true}),
        Some(&u4),
    )
    .await;
    println!("[adv] S3 minority-declared consent -> HTTP {s} {v}");
    assert_ne!(s, StatusCode::OK, "declared minority must be blocked");

    // S4: minor lies (minority_declared=false) -> no age verification exists.
    consent_and_submit(&app, &u4, r4, &format!("k-adv4-{}", Uuid::new_v4())).await;
    let s4 = wait_release_status(&pool, r4, &["STAGE2_REVIEW", "READY_FOR_DELIVERY"], 180).await;
    println!("[adv] S4 undeclared minor -> {s4} (gap if READY_FOR_DELIVERY)");

    // S5: cover song, honestly titled but undeclared -> tripwire review.
    let (u5, r5) = adversarial_seed(
        &app,
        &pool,
        &store,
        &unique_audio(&base_wav),
        "Blinding Lights (Cover)",
    )
    .await;
    consent_and_submit(&app, &u5, r5, &format!("k-adv5-{}", Uuid::new_v4())).await;
    let s5 = wait_release_status(&pool, r5, &["STAGE2_REVIEW", "READY_FOR_DELIVERY"], 180).await;
    println!("[adv] S5 undeclared cover title -> {s5} (must be STAGE2_REVIEW)");
    assert_eq!(
        s5, "STAGE2_REVIEW",
        "undeclared cover title must trip review"
    );

    // S6: explicit lyrics + parental advisory -> special-flag review.
    let (u6, r6) = adversarial_seed(
        &app,
        &pool,
        &store,
        &unique_audio(&base_wav),
        "Explicit Song",
    )
    .await;
    sqlx::query("UPDATE catalog.tracks SET lyrics='explicit lyrics here', parental_advisory=true WHERE release_id=$1")
        .bind(r6)
        .execute(&pool)
        .await
        .unwrap();
    consent_and_submit(&app, &u6, r6, &format!("k-adv6-{}", Uuid::new_v4())).await;
    let s6 = wait_release_status(&pool, r6, &["STAGE2_REVIEW", "READY_FOR_DELIVERY"], 180).await;
    println!("[adv] S6 parental advisory -> {s6} (must be STAGE2_REVIEW)");
    assert_eq!(s6, "STAGE2_REVIEW", "explicit content must route to review");

    // S7: declared cover (is_cover=true) -> special-flag review.
    let (u7, r7) = adversarial_seed(
        &app,
        &pool,
        &store,
        &unique_audio(&base_wav),
        "Declared Cover Song",
    )
    .await;
    consent_and_submit_decl(
        &app,
        &u7,
        r7,
        &format!("k-adv7-{}", Uuid::new_v4()),
        json!({"is_cover": true}),
    )
    .await;
    let s7 = wait_release_status(&pool, r7, &["STAGE2_REVIEW", "READY_FOR_DELIVERY"], 180).await;
    println!("[adv] S7 declared cover -> {s7} (must be STAGE2_REVIEW)");
    assert_eq!(s7, "STAGE2_REVIEW", "declared cover must route to review");

    // S8: missing rights/adult declarations -> submit rejected.
    let (u8, r8) = adversarial_seed(
        &app,
        &pool,
        &store,
        &unique_audio(&base_wav),
        "No Declaration Song",
    )
    .await;
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{r8}/consents", u8.org),
        json!({"parties":[{"party_id":u8.party,"role":"ARTIST"}],"minority_declared":false}),
        Some(&u8),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{r8}/submit", u8.org),
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":format!("k-adv8-{}", Uuid::new_v4()),
            "declarations":{"rights_confirmed":false,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        Some(&u8),
    )
    .await;
    println!("[adv] S8 missing rights declaration -> HTTP {s} {v}");
    assert_ne!(
        s,
        StatusCode::OK,
        "submit without rights confirmation must be rejected"
    );

    // S9: UPC claimed by another org's live release -> duplicate review.
    let (u9, r9) = adversarial_seed(
        &app,
        &pool,
        &store,
        &unique_audio(&base_wav),
        "UPC Clash Song",
    )
    .await;
    let stolen_upc: String = sqlx::query_scalar("SELECT upc FROM catalog.releases WHERE id=$1")
        .bind(r1)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.releases SET upc=$1, row_version=row_version+1 WHERE id=$2")
        .bind(&stolen_upc)
        .bind(r9)
        .execute(&pool)
        .await
        .unwrap();
    consent_and_submit(&app, &u9, r9, &format!("k-adv9-{}", Uuid::new_v4())).await;
    let s9 = wait_release_status(&pool, r9, &["STAGE2_REVIEW", "READY_FOR_DELIVERY"], 180).await;
    println!("[adv] S9 duplicate UPC -> {s9} (must be STAGE2_REVIEW)");
    assert_eq!(s9, "STAGE2_REVIEW", "duplicate UPC must route to review");

    println!("[adv] DONE");
}

/// E-4 wiring: delivery.send on DELIVERED schedules the first delayed
/// delivery.poll; while the partner reports ingesting, each poll re-queues
/// the next; a poll at the cap schedules nothing further (reconcile owns it).
#[sqlx::test]
async fn dsp_poll_chain_schedules_and_caps(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.enqueue").await,
        "SUCCEEDED"
    );
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.send").await,
        "SUCCEEDED"
    );
    // First poll scheduled, delayed ~1h, poll_no=0.
    let (poll_no, delayed): (i32, bool) = sqlx::query_as(
        "SELECT (payload->>'poll_no')::int, run_at > now() FROM operations.jobs
         WHERE queue='delivery' AND kind='delivery.poll' AND status='QUEUED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(poll_no, 0);
    assert!(delayed, "first poll must be delayed, not immediate");
    // Force it due and run: the Accept mock keeps reporting ingesting on the
    // submission-inquiry path, so the next poll must be queued.
    sqlx::query(
        "UPDATE operations.jobs SET run_at=now() WHERE queue='delivery' AND kind='delivery.poll'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.poll").await,
        "SUCCEEDED"
    );
    let next: i32 = sqlx::query_scalar(
        "SELECT (payload->>'poll_no')::int FROM operations.jobs
         WHERE queue='delivery' AND kind='delivery.poll' AND status='QUEUED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(next, 1);
    let mut c = authed(&pool, ctx.org).await;
    let live: String = sqlx::query_scalar(
        "SELECT live_status FROM execution.live_bindings WHERE package_id=$1 AND partner_id='mockdsp'",
    )
    .bind(ctx.package_id)
    .fetch_one(&mut *c)
    .await
    .unwrap();
    assert_eq!(live, "INGESTING");
    // At the cap, no further poll is scheduled.
    sqlx::query(
        "UPDATE operations.jobs SET payload=jsonb_set(payload,'{poll_no}','56'), run_at=now()
         WHERE queue='delivery' AND kind='delivery.poll' AND status='QUEUED'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.poll").await,
        "SUCCEEDED"
    );
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations.jobs WHERE queue='delivery' AND kind='delivery.poll' AND status='QUEUED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0, "poll chain must stop at the cap");
}

/// Route taxonomy: merlin (aggregator) and limbo-upstream (upstream) profiles
/// exist as CONTRACTED + delivery_enabled=false placeholders. They document
/// the multi-path model and can never reach the wire until F6 registers a
/// real contract; E-0 only enqueues for profiles with a dsp_id mapping.
#[sqlx::test]
async fn route_taxonomy_placeholders_cannot_send(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let rows: Vec<(String, String, String, bool)> = sqlx::query_as(
        "SELECT partner_id, route_kind, activation_kind, delivery_enabled
         FROM execution.adapter_profiles WHERE partner_id IN ('merlin','limbo-upstream')
         ORDER BY partner_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].1, "upstream");
    assert_eq!(rows[1].1, "aggregator");
    for (pid, _route, activation, enabled) in &rows {
        assert_eq!(activation, "CONTRACTED", "{pid}");
        assert!(
            !enabled,
            "{pid} must not be delivery-enabled without a contract"
        );
    }
    // No dsp_id mapping: E-0's eligible-partner lookup can never select them.
    let mapped: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM execution.adapter_profiles WHERE partner_id IN ('merlin','limbo-upstream') AND dsp_id IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(mapped, 0);
    // send_or_publish capability off: even a hand-built job fails closed.
    let caps: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT capabilities FROM execution.adapter_profiles WHERE partner_id IN ('merlin','limbo-upstream')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    for c in caps {
        assert_eq!(c["send_or_publish"], serde_json::Value::Bool(false));
    }
}

/// Routing engine: fail-closed per-DSP route decisions.
/// - no profile -> NO_ROUTE/NO_PROFILE
/// - CONTRACTED profile without a contract route -> NO_ROUTE/NO_CONTRACT_ROUTE
/// - MOCK direct profile (enabled + send_or_publish) -> ROUTABLE direct
/// - aggregator with explicit route_coverage -> ROUTABLE aggregator
/// - upstream WITHOUT coverage -> NO_ROUTE (coverage never assumed)
/// - upstream WITH coverage -> ROUTABLE upstream
/// - direct beats aggregator beats upstream
/// - placeholder flipped to delivery_enabled but send_or_publish=false ->
///   NO_ROUTE/ADAPTER_CANNOT_SEND (never reaches the wire)
/// - duplicate DSP ids decided once; decisions persist idempotently.
#[sqlx::test]
async fn route_decisions_without_contracts(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    use audeniq_core::routing::{self, RouteKind};
    let org = Uuid::new_v4();
    let dsp_no_profile = Uuid::new_v4();
    let dsp_contracted = Uuid::new_v4();
    let dsp_mock = Uuid::new_v4();
    let dsp_agg = Uuid::new_v4();
    let dsp_up_nocov = Uuid::new_v4();
    let dsp_up_cov = Uuid::new_v4();
    let dsp_both = Uuid::new_v4();
    let dsp_placeholder = Uuid::new_v4();

    let caps = |sendable: bool| serde_json::json!({"send_or_publish": sendable}).to_string();
    let ins = |pid: &str,
               dsp: Option<Uuid>,
               enabled: bool,
               act: &str,
               kind: &str,
               sendable: bool| {
        let (pool, dsp, caps) = (pool.clone(), dsp, caps(sendable));
        let (pid, act, kind) = (pid.to_string(), act.to_string(), kind.to_string());
        async move {
            sqlx::query("INSERT INTO execution.adapter_profiles(partner_id,display_name,profile_version,dsp_id,delivery_enabled,activation_kind,route_kind,capabilities) VALUES($1,'T','1',$2,$3,$4,$5,$6::jsonb)")
                .bind(pid).bind(dsp).bind(enabled).bind(act).bind(kind).bind(caps)
                .execute(&pool).await.unwrap();
        }
    };
    // CONTRACTED direct profile, enabled, but no contract route exists.
    ins(
        "p-contracted",
        Some(dsp_contracted),
        true,
        "CONTRACTED",
        "direct",
        true,
    )
    .await;
    // MOCK direct profile: always sendable.
    ins("p-mock", Some(dsp_mock), true, "MOCK", "direct", true).await;
    // Aggregator covering dsp_agg (MOCK => sendable).
    ins("p-agg", None, true, "MOCK", "aggregator", true).await;
    sqlx::query("INSERT INTO execution.route_coverage(partner_id,dsp_id) VALUES('p-agg',$1)")
        .bind(dsp_agg)
        .execute(&pool)
        .await
        .unwrap();
    // Upstream, sendable, but covering only dsp_up_cov.
    ins("p-up", None, true, "MOCK", "upstream", true).await;
    sqlx::query("INSERT INTO execution.route_coverage(partner_id,dsp_id) VALUES('p-up',$1)")
        .bind(dsp_up_cov)
        .execute(&pool)
        .await
        .unwrap();
    // dsp_both: direct MOCK + aggregator MOCK covering -> direct wins.
    ins(
        "p-both-direct",
        Some(dsp_both),
        true,
        "MOCK",
        "direct",
        true,
    )
    .await;
    sqlx::query("INSERT INTO execution.route_coverage(partner_id,dsp_id) VALUES('p-agg',$1)")
        .bind(dsp_both)
        .execute(&pool)
        .await
        .unwrap();
    // Placeholder trap: enabled but the adapter declares it cannot send.
    ins(
        "p-trap",
        Some(dsp_placeholder),
        true,
        "MOCK",
        "direct",
        false,
    )
    .await;
    // Seeded placeholders (merlin/limbo-upstream) stay CONTRACTED + disabled.

    let dsps = vec![
        dsp_no_profile,
        dsp_contracted,
        dsp_mock,
        dsp_agg,
        dsp_up_nocov,
        dsp_up_cov,
        dsp_both,
        dsp_placeholder,
        dsp_mock, // duplicate: decided once
    ];
    let decisions = routing::decide_routes(&pool, org, &dsps).await.unwrap();
    assert_eq!(decisions.len(), 8);
    let by_dsp: std::collections::HashMap<_, _> = decisions.iter().map(|d| (d.dsp_id, d)).collect();

    let d = by_dsp[&dsp_no_profile];
    assert!(
        !d.routable && d.reason == "NO_PROFILE" && d.route_kind.is_none(),
        "{d:?}"
    );

    let d = by_dsp[&dsp_contracted];
    assert!(!d.routable && d.reason == "NO_CONTRACT_ROUTE", "{d:?}");

    let d = by_dsp[&dsp_mock];
    assert!(
        d.routable && d.route_kind == Some(RouteKind::Direct),
        "{d:?}"
    );
    assert_eq!(d.partner_id.as_deref(), Some("p-mock"));

    let d = by_dsp[&dsp_agg];
    assert!(
        d.routable && d.route_kind == Some(RouteKind::Aggregator),
        "{d:?}"
    );
    assert_eq!(d.partner_id.as_deref(), Some("p-agg"));

    let d = by_dsp[&dsp_up_nocov];
    assert!(!d.routable && d.reason == "NO_PROFILE", "{d:?}");

    let d = by_dsp[&dsp_up_cov];
    assert!(
        d.routable && d.route_kind == Some(RouteKind::Upstream),
        "{d:?}"
    );
    assert_eq!(d.partner_id.as_deref(), Some("p-up"));

    let d = by_dsp[&dsp_both];
    assert!(
        d.routable && d.route_kind == Some(RouteKind::Direct),
        "{d:?}"
    );
    assert_eq!(d.partner_id.as_deref(), Some("p-both-direct"));

    let d = by_dsp[&dsp_placeholder];
    assert!(!d.routable && d.reason == "ADAPTER_CANNOT_SEND", "{d:?}");

    // Decisions persist per package and are re-readable.
    let package = Uuid::new_v4();
    routing::record_route_decisions(&pool, org, package, &decisions)
        .await
        .unwrap();
    let back = routing::get_route_decision(&pool, org, package, dsp_mock)
        .await
        .unwrap()
        .unwrap();
    assert!(back.routable && back.route_kind == Some(RouteKind::Direct));
    assert_eq!(back.partner_id.as_deref(), Some("p-mock"));
    let no_route = routing::get_route_decision(&pool, org, package, dsp_no_profile)
        .await
        .unwrap()
        .unwrap();
    assert!(!no_route.routable && no_route.route_kind.is_none());
    // Idempotent re-record: still one row per dsp.
    routing::record_route_decisions(&pool, org, package, &decisions)
        .await
        .unwrap();
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM execution.route_decisions WHERE package_id=$1")
            .bind(package)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 8);
}

/// Malformed ACK payloads are rejected without touching delivery state.
/// The partner's garbage must not corrupt our side.
#[sqlx::test]
async fn dsp_xx_malformed_ack_rejected(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Accept);

    // Not JSON at all.
    let err = execution::ingest_ack(&pool, ctx.org, &mock, b"not json{{{")
        .await
        .unwrap_err();
    assert!(matches!(err, audeniq_core::error::Error::Invalid));

    // Valid JSON, missing event_id.
    let err = execution::ingest_ack(
        &pool,
        ctx.org,
        &mock,
        br#"{"type":"accepted","partner_message_id":"x"}"#,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, audeniq_core::error::Error::Invalid));

    // Valid JSON, unknown event type.
    let err = execution::ingest_ack(
        &pool,
        ctx.org,
        &mock,
        br#"{"event_id":"e1","type":"frobnicated"}"#,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, audeniq_core::error::Error::Invalid));

    // Valid JSON, accepted but missing partner_message_id.
    let err = execution::ingest_ack(
        &pool,
        ctx.org,
        &mock,
        br#"{"event_id":"e2","type":"accepted"}"#,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, audeniq_core::error::Error::Invalid));
}

/// End-to-end: a release comes in through the API, flows through the
/// internal pipeline (Stage 1 QC -> Stage 2 rights -> DDEX preparation),
/// and is virtually delivered to MockDSP until the partner reports LIVE.
///
/// This is the single test that proves the whole machine works: intake,
/// internal processing, and (virtual) distribution.
#[sqlx::test]
async fn e2e_intake_to_mockdsp_live(pool: PgPool) {
    // ---- 1. INTAKE: release conditions come in via the API ----
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let wav = wav_bytes();
    let asset = register_asset(&pool, &store, &u, "single.wav", wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    add_preparation_supplements(&pool, &store, &u, release).await;
    let revision_id =
        consent_and_submit(&app, &u, release, &format!("k-e2e-{}", Uuid::new_v4())).await;

    // Pin the seeded MockDSP profile to a fixed DSP id BEFORE stage2 runs.
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(&pool)
        .await
        .unwrap();

    // ---- 2. INTERNAL: Stage 1 (QC) ----
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    // QC actually ran and passed: the fingerprint and similarity checks
    // are present, and the audio was not flagged.
    let qc_statuses: Vec<(String, String)> = {
        let mut c = authed(&pool, u.org).await;
        sqlx::query_as(
            "SELECT check_code, status FROM operations.check_results WHERE revision_id=$1 ORDER BY check_code",
        )
        .bind(revision_id)
        .fetch_all(&mut *c)
        .await
        .unwrap()
    };
    assert!(
        qc_statuses
            .iter()
            .any(|(c, s)| c == "AUDIO_FINGERPRINT_FAILED" && s == "PASS"),
        "fingerprint computed: {qc_statuses:?}"
    );
    assert!(
        !qc_statuses
            .iter()
            .any(|(_, s)| s == "BLOCKED" || s == "CORRECTION_REQUIRED"),
        "no QC blockers: {qc_statuses:?}"
    );

    // ---- 3. INTERNAL: Stage 2 (rights review) ----
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );

    // ---- 4. INTERNAL: DDEX preparation ----
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "READY_FOR_DELIVERY");
    // A real ERN package was built and frozen.
    let package_id: Uuid = sqlx::query_scalar(
        "SELECT dp.id FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         WHERE cr.release_id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    let ern_len: i32 = sqlx::query_scalar(
        "SELECT length(ern_xml) FROM distribution.preparation_artifacts WHERE package_id=$1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ern_len > 1000, "ERN XML generated ({ern_len} bytes)");

    // ---- 5. VIRTUAL DELIVERY via MockDSP ----
    let ctx = ReadyCtx {
        org: u.org,
        release,
        package_id,
        store,
    };
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "DELIVERED");
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "DELIVERED");
    assert_eq!(attempt_outcome(&pool, ctx.org, job_id).await, "ACCEPTED");
    // Exactly one wire send: the idempotency key did its job.
    assert_eq!(mock.received().len(), 1);
    assert_eq!(
        live_status(&pool, ctx.org, ctx.package_id).await,
        "INGESTING"
    );

    // ---- 6. Partner reports LIVE via webhook ----
    let pmid: String = {
        let mut c = authed(&pool, ctx.org).await;
        sqlx::query_scalar(
            "SELECT partner_message_id FROM execution.delivery_attempts WHERE job_id=$1",
        )
        .bind(job_id)
        .fetch_one(&mut *c)
        .await
        .unwrap()
    };
    let payload = mock.emit_webhook(&pmid, "live");
    assert_eq!(
        execution::ingest_ack(&pool, ctx.org, &mock, &payload)
            .await
            .unwrap(),
        "APPLIED"
    );
    assert_eq!(live_status(&pool, ctx.org, ctx.package_id).await, "LIVE");
}

/// Timing shootout: a normal song vs a problematic song (10s audio,
/// under the 30s minimum) through the real pipeline.
///
/// Reports wall-clock time per stage and verifies the problematic song
/// is actually caught at Stage 1 while the normal song reaches LIVE.
#[sqlx::test]
async fn e2e_timing_normal_vs_problematic(pool: PgPool) {
    use std::time::Instant;

    fn short_wav_bytes() -> Vec<u8> {
        let dir = std::env::temp_dir().join("audeniq-f5-shared");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("t10.wav");
        if !out.exists() {
            let st = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:duration=10",
                    "-ar",
                    "48000",
                    "-ac",
                    "2",
                    "-c:a",
                    "pcm_s16le",
                ])
                .arg(&out)
                .status()
                .expect("ffmpeg runs");
            assert!(st.success());
        }
        std::fs::read(&out).unwrap()
    }

    let mut report: Vec<(String, u128)> = Vec::new();
    // ---- INTAKE: normal song ----
    let t = Instant::now();
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(&pool)
        .await
        .unwrap();
    let normal_asset = register_asset(&pool, &store, &u, "normal.wav", wav_bytes()).await;
    let normal_release = build_submittable(&app, &pool, &u, normal_asset).await;
    add_preparation_supplements(&pool, &store, &u, normal_release).await;
    let normal_rev = consent_and_submit(
        &app,
        &u,
        normal_release,
        &format!("k-timing-normal-{}", Uuid::new_v4()),
    )
    .await;
    report.push(("intake/normal".into(), t.elapsed().as_millis()));

    // ---- INTAKE: problematic song (10s audio) ----
    let t = Instant::now();
    let bad_asset = register_asset(&pool, &store, &u, "short.wav", &short_wav_bytes()).await;
    let bad_release = build_submittable(&app, &pool, &u, bad_asset).await;
    add_preparation_supplements(&pool, &store, &u, bad_release).await;
    let bad_rev = consent_and_submit(
        &app,
        &u,
        bad_release,
        &format!("k-timing-bad-{}", Uuid::new_v4()),
    )
    .await;
    report.push(("intake/problematic".into(), t.elapsed().as_millis()));

    // ---- STAGE 1: normal ----
    let t = Instant::now();
    let s1_normal = run_one(&pool, &store, "qc", "stage1").await;
    report.push(("stage1/normal".into(), t.elapsed().as_millis()));
    assert_eq!(s1_normal, "SUCCEEDED", "normal song passes QC");

    // ---- STAGE 1: problematic ----
    // Note: the stage1 JOB succeeds (it ran); the RELEASE is what gets
    // gated. A problematic release must land in STAGE1_CORRECTION and
    // stage2 must never be queued for it.
    let t = Instant::now();
    let s1_bad_job = run_one(&pool, &store, "qc", "stage1").await;
    report.push(("stage1/problematic".into(), t.elapsed().as_millis()));
    assert_eq!(s1_bad_job, "SUCCEEDED", "stage1 job runs to completion");
    assert_eq!(
        release_status(&pool, bad_release).await,
        "STAGE1_CORRECTION",
        "problematic song must be gated at STAGE1_CORRECTION"
    );
    // Stage2 was never queued for the bad release: only one stage2 job
    // exists in the whole DB (the normal song's).
    let stage2_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM operations.jobs WHERE kind='stage2'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stage2_count, 1, "stage2 queued only for the normal song");

    // Verify the problem was actually caught: AUDIO_TOO_SHORT flagged.
    let bad_checks: Vec<(String, String)> = {
        let mut c = authed(&pool, u.org).await;
        sqlx::query_as(
            "SELECT check_code, status FROM operations.check_results WHERE revision_id=$1",
        )
        .bind(bad_rev)
        .fetch_all(&mut *c)
        .await
        .unwrap()
    };
    let too_short = bad_checks
        .iter()
        .find(|(c, _)| c == "AUDIO_TOO_SHORT")
        .expect("AUDIO_TOO_SHORT check ran");
    assert_eq!(
        too_short.1, "CORRECTION_REQUIRED",
        "10s audio flagged: {bad_checks:?}"
    );
    // And the normal song had no such flag.
    let normal_checks: Vec<(String, String)> = {
        let mut c = authed(&pool, u.org).await;
        sqlx::query_as(
            "SELECT check_code, status FROM operations.check_results WHERE revision_id=$1",
        )
        .bind(normal_rev)
        .fetch_all(&mut *c)
        .await
        .unwrap()
    };
    assert!(
        normal_checks
            .iter()
            .all(|(_, s)| s == "PASS" || s == "REVIEW_REQUIRED"),
        "normal song: no blockers: {normal_checks:?}"
    );

    // ---- STAGE 2 + PREPARE + DELIVERY: normal only ----
    let t = Instant::now();
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    report.push(("stage2/normal".into(), t.elapsed().as_millis()));

    let t = Instant::now();
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    report.push(("prepare/normal".into(), t.elapsed().as_millis()));
    assert_eq!(
        release_status(&pool, normal_release).await,
        "READY_FOR_DELIVERY"
    );

    let package_id: Uuid = sqlx::query_scalar(
        "SELECT dp.id FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         WHERE cr.release_id=$1",
    )
    .bind(normal_release)
    .fetch_one(&pool)
    .await
    .unwrap();
    let ctx = ReadyCtx {
        org: u.org,
        release: normal_release,
        package_id,
        store,
    };
    let t = Instant::now();
    let mock = MockDsp::new(MockBehavior::Accept);
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    report.push(("delivery/normal".into(), t.elapsed().as_millis()));
    assert_eq!(status, "DELIVERED");
    let pmid: String = {
        let mut c = authed(&pool, ctx.org).await;
        sqlx::query_scalar(
            "SELECT partner_message_id FROM execution.delivery_attempts WHERE job_id=$1",
        )
        .bind(job_id)
        .fetch_one(&mut *c)
        .await
        .unwrap()
    };
    let payload = mock.emit_webhook(&pmid, "live");
    assert_eq!(
        execution::ingest_ack(&pool, ctx.org, &mock, &payload)
            .await
            .unwrap(),
        "APPLIED"
    );
    assert_eq!(live_status(&pool, ctx.org, ctx.package_id).await, "LIVE");

    // ---- TIMING REPORT ----
    println!("\n=== TIMING REPORT (normal vs problematic) ===");
    for (label, ms) in &report {
        println!("  {label:<22} {ms}ms");
    }
    let total: u128 = report.iter().map(|(_, ms)| ms).sum();
    println!("  ------------------------------");
    println!("  total                  {total}ms");
    println!("=== normal song: LIVE | problematic song: blocked at stage1 (AUDIO_TOO_SHORT) ===\n");
}

/// Stress test: 100 releases at once via API (3.5min audio).
/// - 50 normal: valid 3.5min audio, clean metadata -> expect STAGE1_PASSED
/// - 25 problematic: 3.5min 22kHz audio -> expect STAGE1_CORRECTION (AUDIO_SAMPLE_RATE_LOW)
/// - 15 ambiguous: valid audio, title with version info -> REVIEW_REQUIRED (not blocked)
/// - 10 duplicate: byte-identical to normal songs -> REVIEW_REQUIRED (similar)
///
/// Verifies: no panics, all 300 processed, correct gating per category.
#[sqlx::test]
async fn stress_300_mixed_releases(pool: PgPool) {
    use std::time::Instant;
    let t_all = Instant::now();

    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(&pool)
        .await
        .unwrap();

    let normal_bytes = long_wav_bytes();
    // Problematic: 3.5min but 22.05kHz sample rate (below 44.1kHz minimum).
    let bad_bytes = long_lowrate_wav_bytes();

    // Helper to submit one release. Returns (release_id, revision_id).
    #[allow(clippy::too_many_arguments)]
    async fn submit_one(
        app: &axum::Router,
        pool: &PgPool,
        store: &std::sync::Arc<crate::FileStore>,
        u: &User,
        audio: &[u8],
        name: &str,
        title: &str,
        isrc_suffix: u32,
        tag: &str,
    ) -> (Uuid, Uuid) {
        let asset = register_asset(pool, store, u, name, audio).await;
        let release = create_release(app, u).await;
        let artist = create_artist(app, u).await;
        sqlx::query("UPDATE catalog.releases SET draft = draft || '{\"release_date\":\"2027-03-01\"}'::jsonb, row_version = row_version + 1 WHERE id=$1")
            .bind(release).execute(pool).await.unwrap();
        let rv = row_version(pool, release).await;
        let (s, v) = call(
            app, "POST",
            &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
            serde_json::json!({"title":title,"disc_number":1,"track_number":1,"artist_id":artist,"asset_id":asset,"row_version":rv}),
            Some(u),
        ).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let track = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
        // Unique ISRC per track.
        let isrc = format!("USTST{:07}", isrc_suffix);
        sqlx::query("UPDATE catalog.tracks SET isrc=$1 WHERE id=$2")
            .bind(&isrc)
            .bind(track)
            .execute(pool)
            .await
            .unwrap();
        let rv = row_version(pool, release).await;
        let (s, v) = call(
            app, "PUT",
            &format!("/api/orgs/{}/releases/{release}/tracks/{track}/credits", u.org),
            serde_json::json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"},{"party_id":u.party,"role":"COMPOSER"}]}),
            Some(u),
        ).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        add_preparation_supplements(pool, store, u, release).await;
        let rev = consent_and_submit(
            app,
            u,
            release,
            &format!("k-stress-{tag}-{}", Uuid::new_v4()),
        )
        .await;
        (release, rev)
    }

    // ---- INTAKE: 300 releases ----
    let t = Instant::now();
    let mut releases: Vec<(Uuid, Uuid, &'static str)> = Vec::with_capacity(100);
    let mut isrc_counter = 0u32;

    // 50 normal
    for i in 0..50 {
        isrc_counter += 1;
        let (r, rev) = submit_one(
            &app,
            &pool,
            &store,
            &u,
            &normal_bytes,
            &format!("n{i}.wav"),
            &format!("Normal Track {i}"),
            isrc_counter,
            "normal",
        )
        .await;
        releases.push((r, rev, "normal"));
        if i % 50 == 49 {
            println!("  intake: {} / 100", releases.len());
        }
    }
    // 25 problematic (3.5min but low sample rate)
    for i in 0..25 {
        isrc_counter += 1;
        let (r, rev) = submit_one(
            &app,
            &pool,
            &store,
            &u,
            &bad_bytes,
            &format!("p{i}.wav"),
            &format!("LowRate Track {i}"),
            isrc_counter,
            "bad",
        )
        .await;
        releases.push((r, rev, "problematic"));
        if i % 20 == 19 {
            println!("  intake: {} / 100", releases.len());
        }
    }
    // 15 ambiguous (version info in title)
    for i in 0..15 {
        isrc_counter += 1;
        let (r, rev) = submit_one(
            &app,
            &pool,
            &store,
            &u,
            &normal_bytes,
            &format!("a{i}.wav"),
            &format!("Ambiguous Track {i} (Remix)"),
            isrc_counter,
            "amb",
        )
        .await;
        releases.push((r, rev, "ambiguous"));
    }
    // 10 duplicate (same bytes as normal #0)
    for i in 0..10 {
        isrc_counter += 1;
        let (r, rev) = submit_one(
            &app,
            &pool,
            &store,
            &u,
            &normal_bytes,
            &format!("d{i}.wav"),
            &format!("Duplicate Track {i}"),
            isrc_counter,
            "dup",
        )
        .await;
        releases.push((r, rev, "duplicate"));
    }
    println!(
        "  intake done: {} releases in {}ms",
        releases.len(),
        t.elapsed().as_millis()
    );

    // ---- STAGE 1: process all ----
    let t = Instant::now();
    let mut stage1_done = 0;
    loop {
        let job = operations::claim(&pool, "qc", "stress-worker", 60)
            .await
            .unwrap();
        match job {
            None => break,
            Some(j) => {
                assert_eq!(j.kind, "stage1");
                let dyn_store: std::sync::Arc<dyn crate::ObjectStore> = store.clone();
                // Must not panic on any input.
                let res = operations::execute(&pool, &dyn_store, &j).await;
                assert!(res.is_ok(), "stage1 panicked/failed on job {}", j.id);
                stage1_done += 1;
                if stage1_done % 50 == 0 {
                    println!("  stage1: {stage1_done} / 100");
                }
            }
        }
    }
    println!(
        "  stage1 done: {stage1_done} jobs in {}ms",
        t.elapsed().as_millis()
    );
    assert_eq!(stage1_done, 100, "all 100 stage1 jobs processed");

    // ---- VERIFY: gating per category ----
    let mut counts: std::collections::HashMap<(String, String), i32> = Default::default();
    for (release, _rev, cat) in &releases {
        let status = release_status(&pool, *release).await;
        *counts.entry((cat.to_string(), status)).or_insert(0) += 1;
    }
    println!("\n=== STRESS RESULT (100 releases, 3.5min audio) ===");
    let mut keys: Vec<_> = counts.keys().collect();
    keys.sort();
    for k in keys {
        println!("  {} / {} : {}", k.0, k.1, counts[k]);
    }

    // Assertions: each category landed where it should.
    let get = |cat: &str, status: &str| -> i32 {
        *counts
            .get(&(cat.to_string(), status.to_string()))
            .unwrap_or(&0)
    };
    assert_eq!(get("normal", "STAGE1_PASSED"), 50, "all normal passed");
    assert_eq!(
        get("problematic", "STAGE1_CORRECTION"),
        25,
        "all problematic blocked"
    );
    // Ambiguous and duplicate get REVIEW_REQUIRED checks but are not blocked.
    assert_eq!(
        get("ambiguous", "STAGE1_PASSED"),
        50,
        "ambiguous not blocked"
    );
    assert_eq!(
        get("duplicate", "STAGE1_PASSED"),
        30,
        "duplicate not blocked at stage1"
    );

    // Verify the specific flags fired.
    let amb_review: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT cr.revision_id) FROM operations.check_results cr
         JOIN (SELECT id AS rev_id FROM catalog.application_revisions WHERE release_id = ANY($1)) ar ON ar.rev_id = cr.revision_id
         WHERE cr.check_code='TRACK_TITLE_HAS_VERSION_INFO' AND cr.status='REVIEW_REQUIRED'",
    )
    .bind(releases.iter().filter(|(_,_,c)| *c=="ambiguous").map(|(r,_,_)| *r).collect::<Vec<Uuid>>())
    .fetch_one(&pool)
    .await
    .unwrap_or(0);
    println!("  ambiguous with REVIEW_REQUIRED flag: {amb_review} / 50");

    println!("  total wall time: {}ms", t_all.elapsed().as_millis());
    println!("=== no panics, all 100 processed ===\n");
}

// ---------------------------------------------------------------------------
// Sandbox round 2: delivery lease vs worker crash / shutdown
// ---------------------------------------------------------------------------

async fn delivery_job_for(pool: &PgPool, ctx: &ReadyCtx) -> Uuid {
    let mut c = authed(pool, ctx.org).await;
    sqlx::query_scalar(
        "SELECT id FROM execution.delivery_jobs WHERE package_id=$1 AND partner_id='mockdsp'",
    )
    .bind(ctx.package_id)
    .fetch_one(&mut *c)
    .await
    .unwrap()
}

async fn attempts_for(pool: &PgPool, org: Uuid, job: Uuid) -> i64 {
    let mut c = authed(pool, org).await;
    sqlx::query_scalar("SELECT count(*) FROM execution.delivery_attempts WHERE job_id=$1")
        .bind(job)
        .fetch_one(&mut *c)
        .await
        .unwrap()
}

/// A worker killed while holding the delivery lease (short lease). The send
/// job used to return without finishing, burning an attempt per job lease
/// until DEAD_LETTER while the delivery job stayed LEASED. Now it waits out
/// the lease without consuming attempts and delivers exactly once.
#[sqlx::test]
async fn delivery_send_waits_out_crashed_lease_and_delivers_once(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.enqueue").await,
        "SUCCEEDED"
    );
    let djob = delivery_job_for(&pool, &ctx).await;
    // The crashed holder: leased, then never heard from again.
    assert!(
        execution::lease_delivery_job(&pool, djob, ctx.org, "crashed-worker", 2)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.send").await,
        "QUEUED",
        "parked behind the live lease"
    );
    let (attempts, err): (i32, Option<String>) = sqlx::query_as(
        "SELECT attempts, last_error FROM operations.jobs WHERE kind='delivery.send'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attempts, 0, "waiting for a lease consumes no attempt");
    assert_eq!(err.as_deref(), Some("DELIVERY_LEASE_HELD"));
    tokio::time::sleep(std::time::Duration::from_millis(3200)).await;
    sqlx::query("UPDATE operations.jobs SET run_at=now() WHERE kind='delivery.send'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.send").await,
        "SUCCEEDED"
    );
    assert_eq!(job_status(&pool, ctx.org, djob).await, "DELIVERED");
    assert_eq!(
        attempts_for(&pool, ctx.org, djob).await,
        1,
        "sent exactly once"
    );
}

/// The state the old code left behind (delivery job LEASED with an expired
/// lease, its send job dead-lettered) is repaired by reconcile, and the
/// repaired send delivers exactly once; a second sweep does nothing.
#[sqlx::test]
async fn reconcile_repairs_stalled_leased_delivery(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.enqueue").await,
        "SUCCEEDED"
    );
    let djob = delivery_job_for(&pool, &ctx).await;
    execution::lease_delivery_job(&pool, djob, ctx.org, "crashed-worker", 1)
        .await
        .unwrap()
        .unwrap();
    sqlx::query("UPDATE operations.jobs SET status='DEAD_LETTER', dead_lettered_at=now(), last_error='LEASE_EXPIRED' WHERE kind='delivery.send'")
        .execute(&pool)
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    execution::reconcile(&pool, 3600).await.unwrap();
    assert_eq!(job_status(&pool, ctx.org, djob).await, "QUEUED");
    let live: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.jobs WHERE kind='delivery.send' AND status='QUEUED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(live, 1, "one fresh send job");
    assert_eq!(
        run_one(&pool, &ctx.store, "delivery", "delivery.send").await,
        "SUCCEEDED"
    );
    assert_eq!(job_status(&pool, ctx.org, djob).await, "DELIVERED");
    assert_eq!(
        attempts_for(&pool, ctx.org, djob).await,
        1,
        "sent exactly once"
    );
    execution::reconcile(&pool, 3600).await.unwrap();
    let sends: i64 =
        sqlx::query_scalar("SELECT count(*) FROM operations.jobs WHERE kind='delivery.send'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sends, 2, "no further send enqueued for a delivered job");
}

/// Graceful shutdown hands an unfinished job back at once, without
/// consuming the attempt, and a stale token cannot release someone else's.
#[sqlx::test]
async fn shutdown_release_requeues_without_burning_an_attempt(pool: PgPool) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    operations::enqueue(&mut tx, "qc", "noop.test", &json!({}), "k-release", None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let job = operations::claim(&pool, "qc", "w1", 300)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !operations::release_lease(&pool, job.id, Uuid::new_v4())
            .await
            .unwrap()
    );
    assert!(
        operations::release_lease(&pool, job.id, job.token)
            .await
            .unwrap()
    );
    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status, attempts FROM operations.jobs WHERE id=$1")
            .bind(job.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempts), ("QUEUED", 0));
    let again = operations::claim(&pool, "qc", "w2", 300)
        .await
        .unwrap()
        .expect("immediately claimable by another worker");
    assert_eq!(again.id, job.id);
}

/// DSP failure injection: a partner outage (refused before processing) is
/// retried with a new attempt and delivers exactly once when the partner
/// recovers; nothing is ever parked as SENT_UNKNOWN for a definite refusal.
#[sqlx::test]
async fn dsp_unavailable_is_retried_and_delivers_once(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Unavailable { remaining: 2 });
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "RETRY");
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "QUEUED");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    for expected in ["RETRY", "DELIVERED"] {
        let job = execution::lease_delivery_job(&pool, job_id, ctx.org, "retry-worker", 60)
            .await
            .unwrap()
            .expect("re-leasable after a retryable refusal");
        let status = execution::run_delivery(&pool, &dyn_store, &mock, &job)
            .await
            .unwrap();
        assert_eq!(status, expected);
    }
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "DELIVERED");
    let mut c = authed(&pool, ctx.org).await;
    let accepted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM execution.delivery_attempts WHERE job_id=$1 AND outcome='ACCEPTED'",
    )
    .bind(job_id)
    .fetch_one(&mut *c)
    .await
    .unwrap();
    assert_eq!(accepted, 1, "accepted exactly once");
    for (key, count) in mock.key_counts() {
        assert_eq!(count, 1, "key {key} used once");
    }
    assert!(!case_exists(&pool, ctx.org, job_id, "SENT_UNKNOWN").await);
}

/// A partner that stays down exhausts the delivery job's attempts: it ends
/// DEAD_LETTER (visible), never a silent LEASED/QUEUED limbo.
#[sqlx::test]
async fn dsp_unavailable_exhausts_to_dead_letter(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let mock = MockDsp::new(MockBehavior::Unavailable { remaining: 100 });
    let (job_id, status) = run_send(&pool, &ctx, &mock).await;
    assert_eq!(status, "RETRY");
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    while let Some(job) = execution::lease_delivery_job(&pool, job_id, ctx.org, "retry-worker", 60)
        .await
        .unwrap()
    {
        let status = execution::run_delivery(&pool, &dyn_store, &mock, &job)
            .await
            .unwrap();
        assert_eq!(status, "RETRY");
    }
    assert_eq!(
        execution::delivery_lease_blocked(&pool, ctx.org, job_id)
            .await
            .unwrap(),
        execution::LeaseBlocked::Exhausted
    );
    assert_eq!(job_status(&pool, ctx.org, job_id).await, "DEAD_LETTER");
}
