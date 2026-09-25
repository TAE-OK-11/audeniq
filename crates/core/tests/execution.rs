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
use std::{
    collections::BTreeMap,
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

#[derive(Default)]
struct FileStore {
    files: Mutex<BTreeMap<String, (Vec<u8>, String)>>,
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
        Ok(self.files.lock().await.get(key).map(|(b, ct)| ObjectMeta {
            size: b.len() as i64,
            content_type: ct.clone(),
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
            .map(|(b, _)| b.clone())
            .ok_or(Error::Storage)
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
    store
        .files
        .lock()
        .await
        .insert(key.clone(), (bytes.to_vec(), "audio/wav".into()));
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
        json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"}]}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    release
}

async fn consent_and_submit(app: &Router, u: &User, release: Uuid, key: &str) -> Uuid {
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
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/submit", u.org),
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":key}),
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
    let art_key = format!("registered/{}/cover.png", u.org);
    let art_bytes = b"\x89PNGfake";
    store
        .files
        .lock()
        .await
        .insert(art_key.clone(), (art_bytes.to_vec(), "image/png".into()));
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(u.org)
        .bind(art_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,sha256,state) VALUES($1,$2,'IMAGE',$3,$4,'image/png',$5,'REGISTERED')")
        .bind(art_id).bind(u.org).bind(&art_key).bind(art_bytes.len() as i64).bind(sha256_hex(art_bytes))
        .execute(pool).await.unwrap();
    sqlx::query("UPDATE catalog.releases SET upc='036000291452', artwork_asset_id=$1, draft = draft || '{\"language\":\"ko\",\"artist\":\"Test Artist\",\"p_line\":\"P 2027 Test Label\",\"c_line\":\"C 2027 Test Label\"}'::jsonb, row_version = row_version + 1 WHERE id=$2")
        .bind(art_id)
        .bind(release)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc='USABC2600001' WHERE release_id=$1")
        .bind(release)
        .execute(pool)
        .await
        .unwrap();
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

/// The operations dispatcher wiring: delivery.enqueue fans out delivery.send
/// jobs that the existing claim/execute loop drives to DELIVERED.
#[sqlx::test]
async fn dsp_ops_dispatcher_end_to_end(pool: PgPool) {
    let ctx = ready_package(&pool).await;
    let dyn_store: Arc<dyn ObjectStore> = ctx.store.clone();
    // Enqueue the fan-out job directly (the API layer would do this when a
    // package becomes READY_FOR_DELIVERY).
    {
        let mut tx = pool.begin().await.unwrap();
        operations::enqueue(
            &mut tx,
            "delivery",
            "delivery.enqueue",
            &json!({"package_id": ctx.package_id}),
            &format!("delivery.enqueue:{}", ctx.package_id),
            None,
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
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
