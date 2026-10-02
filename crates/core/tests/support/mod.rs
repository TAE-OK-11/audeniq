//! Helpers shared by the pipeline integration suites (stage1/2, distribution,
//! execution, partner delivery, album FLAC): API calls as a signed-in user,
//! disk-backed (`FileStore`) and in-memory (`MemStore`) object stores, and
//! small DB/fixture readers. A suite that
//! needs a different variant defines its own, which shadows the glob import.
#![allow(dead_code)]
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
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

pub const SECRET: &str = "test-only-service-secret-32-characters";
pub const ORIGIN: &str = "http://localhost:5173";
pub const SIG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
pub struct FileStore {
    pub dir: std::path::PathBuf,
    pub get_calls: AtomicUsize,
}
impl Drop for FileStore {
    fn drop(&mut self) {
        // Each instance owns a unique disposable directory. Retaining every
        // master across suites otherwise fills the test host's disk.
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl FileStore {
    pub fn path_for(&self, key: &str) -> std::path::PathBuf {
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
    pub async fn put(&self, key: &str, bytes: &[u8], content_type: &str) {
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
#[derive(Clone)]
pub struct User {
    pub user: Uuid,
    pub org: Uuid,
    pub party: Uuid,
    pub cookie: String,
    pub csrf: String,
}
/// Like `send`, but a staff release decision first claims the review for the
/// deciding reviewer when nobody holds it (decisions are claimer-only; the
/// claim rules themselves are tested with `send`).
pub async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    user: Option<&User>,
) -> (StatusCode, Value) {
    if method == "POST"
        && path.starts_with("/api/staff/releases/")
        && path.ends_with("/decision")
        && user.is_some()
    {
        let claim = path.trim_end_matches("/decision").to_string() + "/claim";
        let _ = send(app, "POST", &claim, json!({}), user).await;
    }
    send(app, method, path, body, user).await
}

pub async fn send(
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
pub async fn user(app: &Router) -> User {
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
        user: Uuid::parse_str(r["user_id"].as_str().unwrap()).unwrap(),
        org: Uuid::parse_str(r["org_id"].as_str().unwrap()).unwrap(),
        party: Uuid::parse_str(r["party_id"].as_str().unwrap()).unwrap(),
        cookie,
        csrf: l["csrf_token"].as_str().unwrap().into(),
    }
}
pub async fn create_release(app: &Router, u: &User) -> Uuid {
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
pub async fn create_artist(app: &Router, u: &User) -> Uuid {
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
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}
pub fn wav_bytes() -> &'static [u8] {
    static ONCE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        // QC flags audio under 30s as suspiciously short (MIN_AUDIO_SECS):
        // the shared fixture is 32s like the F4 suite. Generated once per
        // test binary; each test still gets its own database.
        let dir = std::env::temp_dir().join("audeniq-f5-shared");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("t32.wav");
        if !out.exists() {
            // Nextest runs test binaries in separate processes. Never expose a
            // partially written shared fixture to another process.
            let tmp = dir.join(format!("t32.{}.tmp", std::process::id()));
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
                    "-f",
                    "wav",
                ])
                .arg(&tmp)
                .status()
                .expect("ffmpeg runs");
            assert!(st.success());
            std::fs::rename(&tmp, &out).unwrap();
        }
        std::fs::read(&out).unwrap()
    })
}
pub async fn row_version(pool: &PgPool, release: Uuid) -> i64 {
    sqlx::query_scalar("SELECT row_version FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(pool)
        .await
        .unwrap()
}
pub async fn release_status(pool: &PgPool, release: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(pool)
        .await
        .unwrap()
}
/// Put the release's distribution agreement in SIGNED (or back in REVIEW).
pub async fn set_agreement(pool: &PgPool, org: Uuid, release: Uuid, signed: bool) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO portal.documents(id,org_id,release_id,kind,title,status,signature,signed_at)
         VALUES($1,$2,$3,'AGREEMENT','agreement',
                CASE WHEN $4 THEN 'SIGNED' ELSE 'REVIEW' END,
                CASE WHEN $4 THEN $5 ELSE '' END,
                CASE WHEN $4 THEN now() END)
         ON CONFLICT(org_id,release_id) WHERE kind='AGREEMENT' DO UPDATE
           SET status=EXCLUDED.status, signature=EXCLUDED.signature, signed_at=EXCLUDED.signed_at,
               checked_at=NULL, row_version=portal.documents.row_version+1
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(org)
    .bind(release)
    .bind(signed)
    .bind(SIG)
    .fetch_one(pool)
    .await
    .unwrap()
}
/// Session-authorized connection for the RLS-protected execution tables.
pub async fn authed(pool: &PgPool, org: Uuid) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut c = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,false)")
        .bind(org.to_string())
        .execute(&mut *c)
        .await
        .unwrap();
    c
}
/// A real 3000x3000 PNG cover: Stage 1 QCs the release artwork (size,
/// square), so a fake header no longer passes. Generated once per binary.
pub fn cover_png() -> &'static [u8] {
    static ONCE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let dir = std::env::temp_dir().join("audeniq-cover-fixture");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("cover3000.png");
        if !out.exists() {
            let tmp = dir.join(format!("cover.{}.png", std::process::id()));
            let st = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=0x3355aa:s=3000x3000",
                    "-frames:v",
                    "1",
                ])
                .arg(&tmp)
                .status()
                .expect("ffmpeg runs");
            assert!(st.success(), "ffmpeg generated the cover fixture");
            std::fs::rename(&tmp, &out).unwrap();
        }
        std::fs::read(&out).unwrap()
    })
}
#[derive(Default)]
pub struct MemStore {
    pub files: Mutex<BTreeMap<String, (Vec<u8>, String)>>,
    pub get_calls: AtomicUsize,
}
#[async_trait]
impl ObjectStore for MemStore {
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
pub async fn app(pool: PgPool) -> (Router, Arc<MemStore>) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(MemStore::default());
    let s = AppState::new(
        pool,
        Config {
            database_url: "postgres://f2test:f2test-local-dev-only@localhost/audeniq_f2".into(),
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
pub async fn register_asset(
    pool: &PgPool,
    store: &MemStore,
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
pub fn make_good_wav(dir: &std::path::Path) -> Vec<u8> {
    let out = dir.join("good.wav");
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
pub async fn consent_and_submit(app: &Router, u: &User, release: Uuid, key: &str) -> Uuid {
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
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":key,"declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap()
}
pub async fn build_submittable(app: &Router, pool: &PgPool, u: &User, asset: Uuid) -> Uuid {
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
/// Run one worker cycle over the given queue; returns the job's terminal status.
pub async fn run_one<S: ObjectStore + 'static>(
    pool: &PgPool,
    store: &Arc<S>,
    queue: &str,
    kind: &str,
) -> String {
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
