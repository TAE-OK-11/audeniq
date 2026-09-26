//! Stage 1 throughput check for bulk deliveries (explicit run only):
//! one release with AUDENIQ_BENCH_TRACKS (default 200) distinct 60 s stereo
//! FLAC masters through Stage 1 QC. Prints wall time, time per track and CPU
//! utilisation (worker + ffmpeg children) against the available cores.
//!
//!   cargo test --release -p audeniq-core --test stage1_bulk -- --ignored --nocapture
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
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-only-service-secret-32-characters";
const ORIGIN: &str = "http://localhost:5173";
const TRACK_SECS: u32 = 60;

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
        // Simulated object-store download time (AUDENIQ_TEST_STORAGE_LATENCY_MS).
        if let Some(ms) = std::env::var("AUDENIQ_TEST_STORAGE_LATENCY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }
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
            database_url: "test".into(),
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

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(bytes))
}

/// 60 s stereo FLAC master, distinct pitch per track so every file differs.
fn make_flac(dir: &Path, idx: usize) -> Vec<u8> {
    let out = dir.join(format!("track{idx:04}.flac"));
    let freq = 110 + (idx as u32) * 7;
    let st = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={freq}:duration={TRACK_SECS}"),
            "-ar",
            "44100",
            "-ac",
            "2",
            "-sample_fmt",
            "s16",
            "-c:a",
            "flac",
        ])
        .arg(&out)
        .status()
        .expect("ffmpeg runs");
    assert!(st.success(), "ffmpeg flac encode {idx}");
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
    store
        .files
        .lock()
        .await
        .insert(key.clone(), (bytes.to_vec(), "audio/flac".into()));
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
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,sha256,state) VALUES($1,$2,'AUDIO',$3,$4,'audio/flac',$5,'REGISTERED')")
        .bind(id).bind(org).bind(&key).bind(bytes.len() as i64).bind(sha256_hex(bytes))
        .execute(pool).await.unwrap();
    id
}

fn cpu_secs() -> f64 {
    let mut total = 0.0;
    for who in [libc::RUSAGE_SELF, libc::RUSAGE_CHILDREN] {
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        unsafe { libc::getrusage(who, &mut ru) };
        for t in [ru.ru_utime, ru.ru_stime] {
            total += t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        }
    }
    total
}

#[sqlx::test]
#[ignore]
async fn stage1_bulk_throughput(pool: PgPool) {
    let tracks: usize = std::env::var("AUDENIQ_BENCH_TRACKS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = std::env::temp_dir().join(format!("audeniq-stage1-bulk-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut assets = Vec::with_capacity(tracks);
    for i in 0..tracks {
        let bytes = make_flac(&dir, i);
        assets.push(register_asset(&pool, &store, &u, &format!("t{i}.flac"), &bytes).await);
    }
    let _ = std::fs::remove_dir_all(&dir);
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases", u.org),
        json!({"name":"Bulk","release_type":"ALBUM"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let release = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/artists", u.org),
        json!({"name":"Bulk Artist"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let artist = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
    for (i, asset) in assets.iter().enumerate() {
        sqlx::query("INSERT INTO catalog.tracks(id,org_id,release_id,title,disc_number,track_number,artist_id,asset_id) VALUES($1,$2,$3,$4,1,$5,$6,$7)")
            .bind(Uuid::new_v4()).bind(u.org).bind(release).bind(format!("Track {}", i + 1))
            .bind(i as i32 + 1).bind(artist).bind(asset)
            .execute(&pool).await.unwrap();
    }
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/consents", u.org),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":false}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = v["consent_id"].clone();
    let (s, v) = call(&app, "POST", &format!("/api/orgs/{}/releases/{release}/submit", u.org),
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":"bulk-bench","declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let job = operations::claim(&pool, "qc", "bench", 3600)
        .await
        .unwrap()
        .unwrap();
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    let (cpu0, t0) = (cpu_secs(), Instant::now());
    operations::execute(&pool, &dyn_store, &job).await.unwrap();
    let (cpu, wall) = (cpu_secs() - cpu0, t0.elapsed().as_secs_f64());
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&pool)
        .await
        .unwrap();
    let fixes: Vec<String> = sqlx::query_scalar("SELECT DISTINCT check_code FROM operations.check_results WHERE status IN ('CORRECTION_REQUIRED','BLOCKED','TECHNICAL_RETRY') ORDER BY 1")
        .fetch_all(&pool).await.unwrap();
    println!(
        "stage1 {tracks} x {TRACK_SECS}s FLAC: {wall:.1}s wall, {:.3}s/track, CPU {:.0}% of {cores} cores, release {status} {fixes:?}",
        wall / tracks as f64,
        100.0 * cpu / (wall * cores as f64)
    );
}
