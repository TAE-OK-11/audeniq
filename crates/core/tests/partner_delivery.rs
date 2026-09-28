//! Real partner delivery (F6 wiring): the DDEX file-drop adapter end to end
//! against a local drop directory, ACK files, partner webhooks through the
//! API, and the cross-release LIVE correlation regression.
//!
//! Separate test binary: it sets process-wide partner config env vars and
//! shares one object store, because the worker's adapter registry is a
//! process-wide cache (as in production, one storage per process).
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
    Arc, OnceLock,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-only-service-secret-32-characters";
const ORIGIN: &str = "http://localhost:5173";
const SIG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
const HOOK_SECRET: &str = "partner-webhook-test-secret";
const MOCK_DSP: &str = "11111111-1111-1111-1111-111111111111";

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

async fn row_version(pool: &PgPool, release: Uuid) -> i64 {
    sqlx::query_scalar("SELECT row_version FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn run_one(pool: &PgPool, store: &Arc<FileStore>, queue: &str, kind: &str) -> String {
    // Other jobs queued earlier in the same queue (e.g. the first release's
    // delivery.stage) run first, as a worker would run them.
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    loop {
        let job = operations::claim(pool, queue, "test-worker", 60)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("{kind} job queued"));
        operations::execute(pool, &dyn_store, &job).await.unwrap();
        if job.kind == kind {
            return sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
                .bind(job.id)
                .fetch_one(pool)
                .await
                .unwrap();
        }
    }
}

/// Put the release's distribution agreement in SIGNED (or back in REVIEW).
async fn set_agreement(pool: &PgPool, org: Uuid, release: Uuid, signed: bool) -> Uuid {
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
async fn authed(pool: &PgPool, org: Uuid) -> sqlx::pool::PoolConnection<sqlx::Postgres> {
    let mut c = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,false)")
        .bind(org.to_string())
        .execute(&mut *c)
        .await
        .unwrap();
    c
}

fn cover_png() -> &'static [u8] {
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

/// Process-wide partner environment: config dir with `mockdsp.json`
/// (DDEX batch profile over a local drop), the drop root, the webhook
/// secret. Returns the drop root.
fn partner_env() -> &'static std::path::PathBuf {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let base = std::env::temp_dir().join(format!("audeniq-partner-{}", Uuid::new_v4()));
        let config = base.join("config");
        let drop = base.join("drop");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(drop.join("acks")).unwrap();
        std::fs::write(
            config.join("mockdsp.json"),
            json!({
                "partner_id": "mockdsp",
                "adapter": "ddex",
                "transport": {"kind": "local", "root": drop},
                "ddex": {"choreography": "batch", "ack_dir": "acks"},
                "webhook": {"secret": {"env": "AUDENIQ_TEST_PARTNER_HOOK_SECRET"}}
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            config.join("D-5.json"),
            json!({"partner_id": "D-5", "adapter": "ddex",
                   "transport": {"kind": "local", "root": drop},
                   "ddex": {"ack_dir": "acks"}})
            .to_string(),
        )
        .unwrap();
        // SAFETY: set once, before any adapter/registry reads them, from a
        // OnceLock initializer; these variables are only read by this binary.
        unsafe {
            std::env::set_var("AUDENIQ_ALLOW_LOCAL_PARTNER_TRANSPORT", "true");
            std::env::set_var("PARTNER_CONFIG_DIR", &config);
            std::env::set_var("AUDENIQ_TEST_PARTNER_HOOK_SECRET", HOOK_SECRET);
            // This binary exercises the post-launch path; the pre-launch
            // lock itself is covered in tests/partner_onboarding.rs.
            std::env::set_var("DSP_LIVE_TRANSMISSION", "enabled");
        }
        drop
    })
}

fn shared_store() -> Arc<FileStore> {
    static S: OnceLock<Arc<FileStore>> = OnceLock::new();
    S.get_or_init(|| Arc::new(FileStore::default())).clone()
}

async fn app(pool: PgPool) -> (Router, Arc<FileStore>) {
    partner_env();
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = shared_store();
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

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

async fn register_asset(pool: &PgPool, store: &FileStore, u: &User, bytes: &[u8]) -> Uuid {
    let org = u.org;
    let id = Uuid::new_v4();
    let key = format!("registered/{org}/{id}/t.wav");
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

async fn build_submittable(app: &Router, pool: &PgPool, u: &User, asset: Uuid) -> Uuid {
    let release = create_release(app, u).await;
    let artist = create_artist(app, u).await;
    sqlx::query("UPDATE catalog.releases SET draft = draft || '{\"release_date\":\"2027-03-01\",\"genre\":\"K-Pop\"}'::jsonb, row_version = row_version + 1 WHERE id=$1")
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
        &format!("/api/orgs/{}/releases/{release}/tracks/{track}/credits", u.org),
        json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"},{"party_id":u.party,"role":"COMPOSER"}]}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    release
}

async fn consent_and_submit(app: &Router, u: &User, release: Uuid) {
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/consents", u.org),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":false}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = v["consent_id"].as_str().unwrap().to_string();
    let decl = json!({"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false});
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/submit", u.org),
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":format!("k-{}", Uuid::new_v4()),"declarations":decl}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

fn unique_upc() -> (String, String) {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let n = NEXT.fetch_add(1, Ordering::SeqCst) + (std::process::id() % 1000) * 1000;
    let body = format!("037{n:08}");
    let sum: u32 = body
        .chars()
        .enumerate()
        .map(|(i, c)| c.to_digit(10).unwrap() * if i % 2 == 0 { 3 } else { 1 })
        .sum();
    (
        format!("{body}{}", (10 - sum % 10) % 10),
        format!("USPDL26{:05}", n % 100_000),
    )
}

async fn add_supplements(pool: &PgPool, store: &FileStore, u: &User, release: Uuid) -> String {
    let art_id = Uuid::new_v4();
    let art_key = format!("registered/{}/cover-{}.png", u.org, art_id);
    let art_bytes = cover_png();
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
    let (upc, isrc) = unique_upc();
    sqlx::query("UPDATE catalog.releases SET upc=$3, artwork_asset_id=$1, draft = draft || '{\"language\":\"ko\",\"artist\":\"Test Artist\",\"p_line\":\"P 2027 Test Label\",\"c_line\":\"C 2027 Test Label\"}'::jsonb, row_version = row_version + 1 WHERE id=$2")
        .bind(art_id).bind(release).bind(&upc)
        .execute(pool).await.unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc=$2 WHERE release_id=$1")
        .bind(release)
        .bind(&isrc)
        .execute(pool)
        .await
        .unwrap();
    upc
}

struct Ready {
    org: Uuid,
    package_id: Uuid,
    upc: String,
}

/// Full pipeline to READY_FOR_DELIVERY for `u`, then the real DDEX message
/// for the (mock) DSP is stored as the wire artifact and the profile is
/// switched to the DDEX transport, so the worker's registry uses the file
/// drop adapter from `mockdsp.json`.
async fn ready_for(app: &Router, pool: &PgPool, store: &Arc<FileStore>, u: &User) -> Ready {
    ready_with(app, pool, store, u, wav_bytes()).await
}

/// A distinct 32 s programme (noise-modulated chord) so a second release of
/// the same org is not held as a duplicate master.
fn other_wav() -> &'static [u8] {
    static ONCE: OnceLock<Vec<u8>> = OnceLock::new();
    ONCE.get_or_init(|| {
        let out = std::env::temp_dir().join(format!("audeniq-pd-other-{}.wav", std::process::id()));
        let st = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anoisesrc=d=33:c=pink:a=0.3:seed=7",
                "-ar",
                "44100",
                "-ac",
                "2",
                "-c:a",
                "pcm_s16le",
                "-f",
                "wav",
            ])
            .arg(&out)
            .status()
            .expect("ffmpeg runs");
        assert!(st.success());
        std::fs::read(&out).unwrap()
    })
}

async fn ready_with(
    app: &Router,
    pool: &PgPool,
    store: &Arc<FileStore>,
    u: &User,
    audio: &[u8],
) -> Ready {
    ready_opts(app, pool, store, u, audio, true).await
}

/// `ddex = false`: a partner-feed partner (Korean services) — no DDEX
/// message is stored and the profile uses the `partner` transport.
async fn ready_opts(
    app: &Router,
    pool: &PgPool,
    store: &Arc<FileStore>,
    u: &User,
    audio: &[u8],
    ddex: bool,
) -> Ready {
    let asset = register_asset(pool, store, u, audio).await;
    let release = build_submittable(app, pool, u, asset).await;
    let upc = add_supplements(pool, store, u, release).await;
    consent_and_submit(app, u, release).await;
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(Uuid::parse_str(MOCK_DSP).unwrap())
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(run_one(pool, store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(run_one(pool, store, "rights", "stage2").await, "SUCCEEDED");
    assert_eq!(
        run_one(pool, store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    let (package_id, canonical_id, body): (Uuid, Uuid, Value) = sqlx::query_as(
        "SELECT dp.id, cr.id, cr.body FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         WHERE cr.release_id=$1",
    )
    .bind(release)
    .fetch_one(pool)
    .await
    .unwrap();
    set_agreement(pool, u.org, release, true).await;
    // The real ERN this DSP receives (what staging writes once DPIDs exist).
    let canonical: audeniq_core::distribution::CanonicalRelease =
        serde_json::from_value(body).unwrap();
    let prepared = audeniq_core::preparation_model::PreparedRelease::from_canonical(
        pool,
        canonical_id,
        &canonical,
    )
    .await
    .unwrap();
    let config = audeniq_core::ddex_ern::DdexErnConfig {
        message_id: format!("AUDENIQ-{package_id}"),
        message_thread_id: None,
        message_sub_type: audeniq_core::ddex_ern::MessageSubType::Initial,
        created_at: "2026-09-28T00:00:00Z".into(),
        sender_name: "Audeniq".into(),
        sender_party_id: Some("TESTDPID-SENDER-0001".into()),
        sent_on_behalf_of: None,
        recipient_name: "MockDSP".into(),
        recipient_party_id: Some("TESTDPID-MOCKDSP-0001".into()),
        deal_start_date: "2027-03-01".into(),
        takedown_date: None,
    };
    let xml = audeniq_core::ddex_ern::generate_ddex_ern_382(&prepared, &config).unwrap();
    if !ddex {
        sqlx::query(
            "UPDATE execution.adapter_profiles SET transport='partner' WHERE partner_id='mockdsp'",
        )
        .execute(pool)
        .await
        .unwrap();
        return Ready {
            org: u.org,
            package_id,
            upc,
        };
    }
    let mut c = authed(pool, u.org).await;
    sqlx::query("INSERT INTO distribution.ddex_messages(package_id,org_id,dsp_id,sender_name,sender_dpid,recipient_name,recipient_dpid,ern_xml,ern_sha256) VALUES($1,$2,$3,'Audeniq','TESTDPID-SENDER-0001','MockDSP','TESTDPID-MOCKDSP-0001',$4,$5)")
        .bind(package_id).bind(u.org).bind(Uuid::parse_str(MOCK_DSP).unwrap()).bind(&xml).bind(sha256_hex(xml.as_bytes()))
        .execute(&mut *c).await.unwrap();
    drop(c);
    sqlx::query(
        "UPDATE execution.adapter_profiles SET transport='ddex' WHERE partner_id='mockdsp'",
    )
    .execute(pool)
    .await
    .unwrap();
    Ready {
        org: u.org,
        package_id,
        upc,
    }
}

async fn registry_adapter(pool: &PgPool, store: &Arc<FileStore>) -> Arc<dyn execution::DspAdapter> {
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    let reg = audeniq_core::partners::registry(
        pool,
        &dyn_store,
        Arc::new(MockDsp::new(MockBehavior::Accept)),
    )
    .await
    .unwrap();
    reg.get("mockdsp").expect("mockdsp adapter")
}

/// E-0 + claim + E-3 through the registry's adapter.
async fn send(
    pool: &PgPool,
    store: &Arc<FileStore>,
    r: &Ready,
) -> (Uuid, String, Arc<dyn execution::DspAdapter>) {
    let (jobs, org) = execution::enqueue_delivery_jobs(pool, r.package_id)
        .await
        .unwrap();
    assert_eq!(jobs.len(), 1);
    let job = execution::claim_delivery_job(pool, org, "mockdsp", "pd-worker", 60)
        .await
        .unwrap()
        .expect("claimed");
    let adapter = registry_adapter(pool, store).await;
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    let status = execution::run_delivery(pool, &dyn_store, adapter.as_ref(), &job)
        .await
        .unwrap();
    (job.id, status, adapter)
}

async fn pmid_of(pool: &PgPool, org: Uuid, job: Uuid) -> String {
    let mut c = authed(pool, org).await;
    sqlx::query_scalar(
        "SELECT partner_message_id FROM execution.delivery_attempts WHERE job_id=$1 ORDER BY attempt_no DESC LIMIT 1",
    )
    .bind(job)
    .fetch_one(&mut *c)
    .await
    .unwrap()
}

async fn scalar_as_org(pool: &PgPool, org: Uuid, sql: &'static str, id: Uuid) -> String {
    let mut c = authed(pool, org).await;
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut *c)
        .await
        .unwrap()
}

#[sqlx::test]
async fn ddex_batch_drop_delivers_exact_files_then_reads_ack(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let r = ready_for(&app, &pool, &store, &u).await;
    let (job, status, adapter) = send(&pool, &store, &r).await;
    assert_eq!(status, "DELIVERED");

    // The submission id was recorded before the upload: <batch>/<UPC>.
    let pmid = pmid_of(&pool, r.org, job).await;
    let (batch, upc) = pmid.split_once('/').unwrap();
    assert_eq!(upc, r.upc);
    assert_eq!(batch.len(), 17);
    let root = partner_env();
    let folder = root.join(batch).join(upc);
    let xml = std::fs::read_to_string(folder.join(format!("{upc}.xml"))).unwrap();
    assert!(xml.contains("NewReleaseMessage"));
    // Every resource the ERN names is there, byte-identical to the pin.
    let audio = std::fs::read(folder.join(format!("{upc}_01_001.wav"))).unwrap();
    assert_eq!(sha256_hex(&audio), sha256_hex(wav_bytes()));
    let cover = std::fs::read(folder.join(format!("{upc}.png"))).unwrap();
    assert_eq!(sha256_hex(&cover), sha256_hex(cover_png()));
    // Completion marker last, at the batch level; no partial files left.
    assert!(
        root.join(batch)
            .join(format!("BatchComplete_{batch}.xml"))
            .exists()
    );
    let leftovers: Vec<_> = std::fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with("part"))
        .collect();
    assert!(leftovers.is_empty());

    // No ACK yet: still ingesting, no evidence recorded.
    let s = execution::poll_live(&pool, r.org, adapter.as_ref(), r.package_id, "mockdsp")
        .await
        .unwrap();
    assert_eq!(s, "INGESTING");
    // The partner drops a DDEX ACK naming the folder.
    std::fs::write(
        root.join("acks").join(format!("ACK_{upc}_{batch}.xml")),
        format!(
            "<ernc:FtpAcknowledgementMessage xmlns:ernc=\"http://ddex.net/xml/ern-c/15\"><AcknowledgedFile><FileName>{upc}.xml</FileName><FilePath>/{batch}/{upc}/</FilePath></AcknowledgedFile><MessageStatus>FileOK</MessageStatus></ernc:FtpAcknowledgementMessage>"
        ),
    )
    .unwrap();
    assert!(matches!(
        adapter.inquire_submission(&pmid).await.unwrap(),
        execution::InquiryOutcome::Accepted { partner_message_id } if partner_message_id == pmid
    ));
    let s = execution::poll_live(&pool, r.org, adapter.as_ref(), r.package_id, "mockdsp")
        .await
        .unwrap();
    assert_eq!(s, "INGESTING", "explicit live policy: an ACK is not LIVE");
    let ack = scalar_as_org(
        &pool,
        r.org,
        "SELECT COALESCE(response->>'ack_outcome','') FROM execution.delivery_attempts WHERE job_id=$1",
        job,
    )
    .await;
    assert_eq!(ack, "ACCEPTED", "the polled ACK is receipt evidence");
    // The reconciler no longer opens MISSING_ACK for this delivery.
    execution::reconcile(&pool, 0).await.unwrap();
    let missing = scalar_as_org(
        &pool,
        r.org,
        "SELECT count(*)::text FROM execution.reconciliation_cases WHERE job_id=$1 AND kind='MISSING_ACK'",
        job,
    )
    .await;
    assert_eq!(missing, "0");
    // Staff evidence makes it LIVE.
    assert_eq!(
        execution::record_manual_live(&pool, r.package_id, "mockdsp", Some("SP-ALBUM-1"), None)
            .await
            .unwrap(),
        "LIVE"
    );
    let live = scalar_as_org(
        &pool,
        r.org,
        "SELECT live_status FROM execution.live_bindings WHERE package_id=$1",
        r.package_id,
    )
    .await;
    assert_eq!(live, "LIVE");

    // Takedown: a DDEX takedown message (deal end date) into a new folder.
    let t = execution::takedown_release(&pool, r.org, adapter.as_ref(), r.package_id, "mockdsp")
        .await
        .unwrap();
    assert_eq!(t, "TAKEDOWN_REQUESTED");
    let takedown_xml = walk(root)
        .into_iter()
        .filter(|p| p.to_string_lossy().ends_with(&format!("{upc}.xml")))
        .map(|p| std::fs::read_to_string(p).unwrap())
        .find(|x| x.contains("<EndDate>"))
        .expect("takedown message delivered");
    assert!(takedown_xml.contains("UpdateMessage"));
    assert!(takedown_xml.contains(&format!(
        "<MessageThreadId>AUDENIQ-{}</MessageThreadId>",
        r.package_id
    )));
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[sqlx::test]
async fn ddex_ack_rejection_fails_the_job(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let r = ready_for(&app, &pool, &store, &u).await;
    let (job, status, adapter) = send(&pool, &store, &r).await;
    assert_eq!(status, "DELIVERED");
    let pmid = pmid_of(&pool, r.org, job).await;
    let (batch, upc) = pmid.split_once('/').unwrap();
    std::fs::write(
        partner_env().join(batch).join(upc).join("ACK_result.xml"),
        "<Ack><MessageStatus>FileOK</MessageStatus><FileStatus>ResourceCorrupt</FileStatus><ErrorText>bad flac</ErrorText></Ack>",
    )
    .unwrap();
    let s = execution::poll_live(&pool, r.org, adapter.as_ref(), r.package_id, "mockdsp")
        .await
        .unwrap();
    assert_eq!(s, "REJECTED");
    let st = scalar_as_org(
        &pool,
        r.org,
        "SELECT status FROM execution.delivery_jobs WHERE id=$1",
        job,
    )
    .await;
    assert_eq!(st, "FAILED");
    let err = scalar_as_org(
        &pool,
        r.org,
        "SELECT last_error FROM execution.delivery_jobs WHERE id=$1",
        job,
    )
    .await;
    assert!(err.contains("ResourceCorrupt"), "{err}");
    let cases = scalar_as_org(
        &pool,
        r.org,
        "SELECT count(*)::text FROM execution.reconciliation_cases WHERE job_id=$1 AND kind='PARTNER_REJECTED' AND status='OPEN'",
        job,
    )
    .await;
    assert_eq!(cases, "1", "a polled rejection opens a case for staff");
}

fn signed(body: &str, ts: i64) -> String {
    let mut msg = format!("{ts}.").into_bytes();
    msg.extend_from_slice(body.as_bytes());
    audeniq_core::partners::http::hmac_hex(HOOK_SECRET.as_bytes(), &msg)
}

async fn hook(app: &Router, body: &str, sig: &str, ts: i64) -> (StatusCode, Value) {
    // No Origin: server-to-server, authenticated by the signature only.
    let req = Request::builder()
        .method("POST")
        .uri("/api/partner-hooks/mockdsp")
        .header("x-audeniq-service", SECRET)
        .header("content-type", "application/json")
        .header("x-signature", sig)
        .header("x-timestamp", ts.to_string())
        .body(Body::from(body.to_string()))
        .unwrap();
    let r = app.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Two releases of one org delivered to the same partner: a signed LIVE
/// webhook for one of them flips exactly that one (it used to flip every
/// INGESTING binding of the partner), duplicates and forgeries do nothing.
#[sqlx::test]
async fn signed_webhook_marks_only_its_own_release_live(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = ready_for(&app, &pool, &store, &u).await;
    let (job_a, s, _) = send(&pool, &store, &a).await;
    assert_eq!(s, "DELIVERED");
    let b = ready_with(&app, &pool, &store, &u, other_wav()).await;
    let (_job_b, s, _) = send(&pool, &store, &b).await;
    assert_eq!(s, "DELIVERED");
    let pmid_a = pmid_of(&pool, a.org, job_a).await;

    let body = json!({"event_id":"evt-live-1","type":"live","partner_message_id":pmid_a,"partner_release_id":"SP-REL-A"}).to_string();
    let now = Utc::now().timestamp();
    // Forged signature / stale timestamp: refused, nothing filed.
    let (st, _) = hook(&app, &body, "deadbeef", now).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _) = hook(&app, &body, &signed(&body, now - 3600), now - 3600).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let filed: i64 = sqlx::query_scalar("SELECT count(*) FROM execution.partner_inbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(filed, 0);
    // Valid: filed once, a redelivery is a duplicate.
    let (st, v) = hook(&app, &body, &signed(&body, now), now).await;
    assert_eq!(st, StatusCode::ACCEPTED, "{v}");
    let (st, v) = hook(&app, &body, &signed(&body, now), now).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["duplicate"], true);
    assert_eq!(
        run_one(&pool, &store, "delivery", "delivery.ack").await,
        "SUCCEEDED"
    );
    let result: String = sqlx::query_scalar("SELECT result FROM execution.partner_inbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(result, "APPLIED");
    let live_a = scalar_as_org(
        &pool,
        a.org,
        "SELECT live_status FROM execution.live_bindings WHERE package_id=$1",
        a.package_id,
    )
    .await;
    let live_b = scalar_as_org(
        &pool,
        b.org,
        "SELECT live_status FROM execution.live_bindings WHERE package_id=$1",
        b.package_id,
    )
    .await;
    assert_eq!(live_a, "LIVE");
    assert_eq!(live_b, "INGESTING", "the other release must not be flipped");
    let prid = scalar_as_org(
        &pool,
        a.org,
        "SELECT partner_release_id FROM execution.live_bindings WHERE package_id=$1",
        a.package_id,
    )
    .await;
    assert_eq!(prid, "SP-REL-A");
    // The same event applied again through the ACK path is ignored.
    let event = execution::AckEvent::Live {
        event_id: "evt-live-1".into(),
        partner_release_id: "SP-REL-A".into(),
        partner_message_id: Some(pmid_a.clone()),
    };
    assert_eq!(
        execution::apply_ack_event(&pool, a.org, "mockdsp", &event)
            .await
            .unwrap(),
        "DUPLICATE_IGNORED"
    );
    // An event naming nothing we sent is UNMATCHED and changes nothing.
    let stray = execution::AckEvent::Live {
        event_id: "evt-stray".into(),
        partner_release_id: "SP-UNKNOWN".into(),
        partner_message_id: None,
    };
    assert_eq!(
        execution::apply_ack_event(&pool, b.org, "mockdsp", &stray)
            .await
            .unwrap(),
        "UNMATCHED"
    );
}

async fn claim(pool: &PgPool, r: &Ready) -> execution::DeliveryJob {
    let (jobs, org) = execution::enqueue_delivery_jobs(pool, r.package_id)
        .await
        .unwrap();
    assert_eq!(jobs.len(), 1);
    execution::claim_delivery_job(pool, org, "mockdsp", "pd-worker", 60)
        .await
        .unwrap()
        .expect("claimed")
}

/// Korean-service feed: audio + cover under their ERN names, manifest.json
/// and metadata.csv (19금 flag, credits, identifiers), marker last; the
/// partner's JSON result file is read back.
#[sqlx::test]
async fn partner_spec_feed_writes_manifest_csv_and_reads_result(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    // No DDEX message exists for a partner-feed DSP: the send must not
    // require one (it used to fail with EXECUTION_DDEX_MESSAGE_MISSING).
    let r = ready_opts(&app, &pool, &store, &u, wav_bytes(), false).await;
    let root = std::env::temp_dir().join(format!("audeniq-kr-{}", Uuid::new_v4()));
    std::fs::create_dir_all(root.join("results")).unwrap();
    let cfg = audeniq_core::partner_config::PartnerConfig::parse(
        &json!({"partner_id":"mockdsp","adapter":"partner_spec",
                "transport":{"kind":"local","root":root},
                "partner_spec":{"result_dir":"results","field_map":{"isrc":"ISRC"}}})
        .to_string(),
    )
    .unwrap();
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    let adapter = audeniq_core::partners::build_adapter(
        cfg,
        execution::Capabilities::from_json(
            &json!({"validate_package":true,"prepare_transfer":true,
            "send_or_publish":true,"inquire_submission":true,"parse_ack":true}),
        ),
        dyn_store.clone(),
    )
    .unwrap();
    let job = claim(&pool, &r).await;
    let status = execution::run_delivery(&pool, &dyn_store, adapter.as_ref(), &job)
        .await
        .unwrap();
    assert_eq!(status, "DELIVERED");
    let pmid = pmid_of(&pool, r.org, job.id).await;
    assert!(pmid.starts_with(&format!("{}_", r.upc)), "{pmid}");
    let folder = root.join(&pmid);
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(folder.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["upc"], r.upc);
    assert_eq!(manifest["genre"], "K-Pop");
    assert_eq!(manifest["adult_only"], false);
    assert!(
        manifest["tracks"][0]["ISRC"]
            .as_str()
            .unwrap()
            .starts_with("USPDL26")
    );
    assert_eq!(
        manifest["tracks"][0]["file"]["name"],
        format!("{}_01_001.wav", r.upc)
    );
    let csv = std::fs::read_to_string(folder.join("metadata.csv")).unwrap();
    assert!(csv.starts_with('\u{feff}'));
    assert!(csv.lines().nth(1).unwrap().contains(&r.upc));
    assert!(folder.join(format!("{}_01_001.wav", r.upc)).exists());
    assert!(folder.join(format!("{pmid}.complete")).exists());
    assert_eq!(
        adapter.inquire_submission(&pmid).await.unwrap(),
        execution::InquiryOutcome::StillUnknown
    );
    std::fs::write(
        root.join("results").join(format!("{pmid}.result.json")),
        json!({"delivery_id": pmid, "status": "rejected", "code": "COVER_TEXT"}).to_string(),
    )
    .unwrap();
    assert_eq!(
        adapter.inquire_submission(&pmid).await.unwrap(),
        execution::InquiryOutcome::Rejected {
            code: "COVER_TEXT".into()
        }
    );
}

#[derive(Default)]
struct FakeApi {
    created: std::sync::Mutex<Vec<(String, Value)>>,
    files: std::sync::Mutex<Vec<(String, String)>>,
    committed: AtomicUsize,
    fail_commit: std::sync::atomic::AtomicBool,
}

async fn fake_partner_api(state: Arc<FakeApi>) -> String {
    use axum::{
        extract::{Path, State},
        routing::{get, post, put},
    };
    async fn create(
        State(s): State<Arc<FakeApi>>,
        h: axum::http::HeaderMap,
        body: axum::body::Bytes,
    ) -> (StatusCode, axum::Json<Value>) {
        assert_eq!(h["authorization"], "Bearer partner-token");
        let key = h["idempotency-key"].to_str().unwrap().to_string();
        s.created
            .lock()
            .unwrap()
            .push((key, serde_json::from_slice(&body).unwrap()));
        (StatusCode::CREATED, axum::Json(json!({"id": "sub-1"})))
    }
    async fn file(
        State(s): State<Arc<FakeApi>>,
        Path((id, name)): Path<(String, String)>,
        body: axum::body::Bytes,
    ) -> StatusCode {
        assert_eq!(id, "sub-1");
        s.files
            .lock()
            .unwrap()
            .push((name, hex::encode(sha2::Sha256::digest(&body))));
        StatusCode::NO_CONTENT
    }
    async fn commit(State(s): State<Arc<FakeApi>>) -> StatusCode {
        if s.fail_commit.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE;
        }
        s.committed.fetch_add(1, Ordering::SeqCst);
        StatusCode::ACCEPTED
    }
    async fn status(State(s): State<Arc<FakeApi>>) -> axum::Json<Value> {
        if s.committed.load(Ordering::SeqCst) > 0 {
            axum::Json(json!({"id":"sub-1","status":"live","release_id":"KR-9"}))
        } else {
            axum::Json(json!({"id":"sub-1","status":"processing"}))
        }
    }
    let app = axum::Router::new()
        .route("/v1/releases", post(create))
        .route("/v1/releases/{id}/files/{name}", put(file))
        .route("/v1/releases/{id}/commit", post(commit))
        .route("/v1/releases/{id}", get(status))
        .layer(axum::extract::DefaultBodyLimit::disable())
        .with_state(state);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{addr}")
}

/// REST partner: create (with our idempotency key) → files → commit; a
/// commit refused with 503 is a safe retry (nothing processed), the retry
/// delivers, and status polling reports LIVE with the partner's id.
#[sqlx::test]
async fn http_api_partner_create_upload_commit_then_live(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let r = ready_for(&app, &pool, &store, &u).await;
    let fake = Arc::new(FakeApi::default());
    let base = fake_partner_api(fake.clone()).await;
    // SAFETY: test-local token variable, read only by this adapter.
    unsafe { std::env::set_var("AUDENIQ_TEST_PARTNER_TOKEN", "partner-token") };
    let cfg = audeniq_core::partner_config::PartnerConfig::parse(
        &json!({"partner_id":"mockdsp","adapter":"http_api",
                "http_api":{"base_url": base, "auth":{"kind":"bearer","token":{"env":"AUDENIQ_TEST_PARTNER_TOKEN"}}}})
        .to_string(),
    )
    .unwrap();
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    let adapter = audeniq_core::partners::build_adapter(
        cfg,
        execution::Capabilities::from_json(&json!({"validate_package":true,"prepare_transfer":true,
            "send_or_publish":true,"inquire_submission":true,"parse_ack":true,"get_release_status":true})),
        dyn_store.clone(),
    )
    .unwrap();
    fake.fail_commit.store(true, Ordering::SeqCst);
    let job = claim(&pool, &r).await;
    let status = execution::run_delivery(&pool, &dyn_store, adapter.as_ref(), &job)
        .await
        .unwrap();
    let resp = scalar_as_org(
        &pool,
        r.org,
        "SELECT response::text FROM execution.delivery_attempts WHERE job_id=$1",
        job.id,
    )
    .await;
    assert_eq!(
        status, "RETRY",
        "503 on commit: nothing processed, retry ({resp})"
    );
    fake.fail_commit.store(false, Ordering::SeqCst);
    let job = execution::lease_delivery_job(&pool, job.id, r.org, "pd-worker", 60)
        .await
        .unwrap()
        .expect("re-leased");
    let status = execution::run_delivery(&pool, &dyn_store, adapter.as_ref(), &job)
        .await
        .unwrap();
    assert_eq!(status, "DELIVERED");
    assert_eq!(fake.committed.load(Ordering::SeqCst), 1);
    let created = fake.created.lock().unwrap().clone();
    assert_eq!(created.len(), 2);
    assert_ne!(created[0].0, created[1].0, "each attempt has its own key");
    assert_eq!(created[1].1["upc"], r.upc);
    let files = fake.files.lock().unwrap().clone();
    assert!(
        files
            .iter()
            .any(|(n, h)| n == &format!("{}_01_001.wav", r.upc) && h == &sha256_hex(wav_bytes()))
    );
    let s = execution::poll_live(&pool, r.org, adapter.as_ref(), r.package_id, "mockdsp")
        .await
        .unwrap();
    assert_eq!(s, "LIVE");
    let prid = scalar_as_org(
        &pool,
        r.org,
        "SELECT partner_release_id FROM execution.live_bindings WHERE package_id=$1",
        r.package_id,
    )
    .await;
    assert_eq!(prid, "KR-9");
}

/// Contracted DSP onboarding: go-live is refused while any requirement is
/// missing; once complete the DSP is routable for every org (distributor
/// contract, no per-org route plan) and visible to artists; suspend closes
/// it immediately.
#[sqlx::test]
async fn onboarding_complete_makes_contracted_dsp_routable_until_suspended(pool: PgPool) {
    use audeniq_core::{partner_admin as pa, partner_onboarding as onb, routing};
    let (app, _store) = app(pool.clone()).await;
    let u = user(&app).await;
    let d5 = audeniq_core::dsp_registry::Dsp::from_code("D-5")
        .unwrap()
        .uuid();
    let route = |org| {
        let pool = pool.clone();
        async move { routing::decide_routes(&pool, org, &[d5]).await.unwrap()[0].clone() }
    };
    assert!(!route(u.org).await.routable);
    let err = pa::go_live(&pool, "ops", "D-5").await.unwrap_err();
    assert!(
        matches!(err, Error::PolicyGate("PARTNER_CANNOT_SEND")),
        "{err:?}"
    );
    pa::set_capabilities(
        &pool,
        "ops",
        "D-5",
        &json!({"send_or_publish":true,"inquire_submission":true,"parse_ack":true}),
    )
    .await
    .unwrap();
    assert!(
        pa::set_capabilities(&pool, "ops", "D-5", &json!({"teleport":true}))
            .await
            .is_err()
    );
    let err = pa::go_live(&pool, "ops", "D-5").await.unwrap_err();
    assert!(
        matches!(err, Error::PolicyGate("PARTNER_NOT_READY_FOR_LIVE")),
        "{err:?}"
    );
    assert!(
        pa::set_recipient_dpid(&pool, "ops", "D-5", "PADPIDA-bad")
            .await
            .is_err()
    );
    pa::set_recipient_dpid(&pool, "ops", "D-5", "PADPIDA2011021601U")
        .await
        .unwrap();
    onb::register_endpoint(&pool, "D-5", "sftp://sftp.partner.example:22/inbox")
        .await
        .unwrap();
    assert!(
        onb::register_endpoint(&pool, "D-5", "sftp://user@host/x")
            .await
            .is_err()
    );
    assert!(
        onb::register_endpoint(&pool, "D-5", "ftp://host/x")
            .await
            .is_err()
    );
    onb::record_credential_stored(&pool, "D-5", "sftp_key")
        .await
        .unwrap();
    onb::record_test_ern_validated(&pool, "D-5").await.unwrap();
    assert!(
        pa::test_ack(
            &pool,
            "ops",
            "D-5",
            b"<Ack><MessageStatus>processing</MessageStatus></Ack>"
        )
        .await
        .is_err()
    );
    pa::test_ack(
        &pool,
        "ops",
        "D-5",
        b"<Ack><MessageStatus>FileOK</MessageStatus></Ack>",
    )
    .await
    .unwrap();
    let gaps = onb::status(&pool, "D-5").await.unwrap().gaps;
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    pa::contract(&pool, "ops", "D-5", "DSA-2026-SPOTIFY-001")
        .await
        .unwrap();
    let live = pa::go_live(&pool, "ops", "D-5").await.unwrap();
    assert_eq!(live["platform_contract_live"], true);
    let d = route(u.org).await;
    assert!(d.routable, "{d:?}");
    assert_eq!(d.partner_id.as_deref(), Some("D-5"));
    // A different org needs no route rows of its own.
    let other = user(&app).await;
    assert!(route(other.org).await.routable);
    assert!(
        routing::public_routes(&pool, other.org, &[d5])
            .await
            .unwrap()[0]
            .routable
    );
    // Operators need a name on every change.
    assert!(pa::suspend(&pool, "", "D-5", "x").await.is_err());
    assert!(
        route(u.org).await.routable,
        "a refused change changes nothing"
    );
    pa::suspend(&pool, "ops", "D-5", "partner maintenance")
        .await
        .unwrap();
    let d = route(u.org).await;
    assert!(!d.routable);
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events WHERE action LIKE 'partner.%' AND reason_code LIKE 'OPERATOR:ops D-5%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(audits >= 6, "{audits}");
}
