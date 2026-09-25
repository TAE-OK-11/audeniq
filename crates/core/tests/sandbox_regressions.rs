//! Regressions for the sandbox distribution test findings (2026-09).
//!
//! Unlike the older suites, nothing here writes `catalog.assets.sha256`
//! directly: audio goes through the real upload -> PUT -> complete API path,
//! and the API and worker run as the split `audeniq_api` / `audeniq_worker`
//! roles after `deploy/grants.sql`, exactly like production. Only fields that
//! have no API yet (UPC, ISRC, cover-art binding) are set through the owner
//! connection.
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
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "test-only-service-secret-32-characters";
const ORIGIN: &str = "http://localhost:5173";

struct Obj {
    bytes: Vec<u8>,
    content_type: String,
    nonce: String,
}

/// S3 stand-in: presigned PUTs are "performed" by `client_put`, which
/// enforces the signed content type / nonce like a real presigned URL.
#[derive(Default)]
struct MemStore {
    objects: Mutex<BTreeMap<String, Obj>>,
    fail_get: AtomicBool,
}
impl MemStore {
    async fn client_put(&self, grant: &Value, key: &str, bytes: &[u8]) {
        let h = &grant["headers"];
        self.objects.lock().await.insert(
            key.into(),
            Obj {
                bytes: bytes.to_vec(),
                content_type: h["content-type"].as_str().unwrap().into(),
                nonce: h["x-amz-meta-upload-nonce"].as_str().unwrap().into(),
            },
        );
    }
}
#[async_trait]
impl ObjectStore for MemStore {
    async fn presign_put(
        &self,
        key: &str,
        size: i64,
        mime: &str,
        nonce: &str,
        expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        Ok(UploadGrant {
            url: format!("https://mem.invalid/{key}"),
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
        Ok(self.objects.lock().await.get(key).map(|o| ObjectMeta {
            size: o.bytes.len() as i64,
            content_type: o.content_type.clone(),
            nonce: o.nonce.clone(),
            etag: hex::encode(sha2::Sha256::digest(&o.bytes)),
        }))
    }
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()> {
        let mut m = self.objects.lock().await;
        let o = m.get(source).ok_or(Error::Storage)?;
        if hex::encode(sha2::Sha256::digest(&o.bytes)) != etag {
            return Err(Error::Conflict);
        }
        let copy = Obj {
            bytes: o.bytes.clone(),
            content_type: o.content_type.clone(),
            nonce: o.nonce.clone(),
        };
        m.insert(target.into(), copy);
        Ok(())
    }
    async fn delete(&self, key: &str) -> Result<()> {
        self.objects.lock().await.remove(key);
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        if self.fail_get.load(Ordering::SeqCst) {
            return Err(Error::Storage);
        }
        self.objects
            .lock()
            .await
            .get(key)
            .map(|o| o.bytes.clone())
            .ok_or(Error::Storage)
    }
}

struct Env {
    owner: PgPool,
    worker: PgPool,
    api: Router,
    store: Arc<MemStore>,
}

async fn role_pool(owner: &PgPool, role: &'static str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(3)
        .after_connect(move |c, _| {
            Box::pin(async move {
                sqlx::query(&format!("SET ROLE {role}")).execute(c).await?;
                Ok(())
            })
        })
        .connect_with((*owner.connect_options()).clone())
        .await
        .unwrap()
}

/// Migrate, apply deploy/grants.sql, and build the API on an `audeniq_api`
/// pool and a worker pool as `audeniq_worker`.
async fn env(owner: PgPool) -> Env {
    database::MIGRATOR.run(&owner).await.unwrap();
    // Roles are cluster-wide and other test binaries create them too.
    for role in ["audeniq_api", "audeniq_worker"] {
        sqlx::raw_sql(&format!(
            "DO $$ BEGIN CREATE ROLE {role} NOLOGIN; \
             EXCEPTION WHEN duplicate_object OR unique_violation THEN NULL; END $$;"
        ))
        .execute(&owner)
        .await
        .unwrap();
    }
    sqlx::raw_sql(include_str!("../../../deploy/grants.sql"))
        .execute(&owner)
        .await
        .unwrap();
    let api_pool = role_pool(&owner, "audeniq_api").await;
    let worker = role_pool(&owner, "audeniq_worker").await;
    let store = Arc::new(MemStore::default());
    let state = AppState::new(
        api_pool,
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
    Env {
        owner,
        worker,
        api: router(state),
        store,
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
) -> (StatusCode, Value, Option<String>) {
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
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

/// `call` with extra request headers (client IP, oversized headers).
async fn call_with(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    user: Option<&User>,
    extra: &[(&str, String)],
) -> (StatusCode, Value, Option<String>) {
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
    for (k, v) in extra {
        b = b.header(*k, v);
    }
    let r = app
        .clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let cookie = r
        .headers()
        .get("set-cookie")
        .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string());
    let bytes = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookie,
    )
}

async fn ok(app: &Router, method: &str, path: &str, body: Value, u: &User) -> Value {
    let (s, v, _) = call(app, method, path, body, Some(u)).await;
    assert_eq!(s, StatusCode::OK, "{method} {path}: {v}");
    v
}

async fn user(app: &Router) -> User {
    let email = format!("{}@example.test", Uuid::new_v4());
    let credentials = json!({"email":email,"password":"Long-test-password-123!"});
    let (s, r, _) = call(app, "POST", "/api/auth/register", credentials.clone(), None).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (s, l, cookie) = call(app, "POST", "/api/auth/login", credentials, None).await;
    assert_eq!(s, StatusCode::OK, "{l}");
    User {
        org: Uuid::parse_str(r["org_id"].as_str().unwrap()).unwrap(),
        party: Uuid::parse_str(r["party_id"].as_str().unwrap()).unwrap(),
        cookie: cookie.unwrap(),
        csrf: l["csrf_token"].as_str().unwrap().into(),
    }
}

struct TmpDir(PathBuf);
impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn tmpdir() -> TmpDir {
    let d = std::env::temp_dir().join(format!("audeniq-sbx-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    TmpDir(d)
}

fn ffmpeg(dir: &Path, name: &str, input: &[&str], output: &[&str]) -> Vec<u8> {
    let out = dir.join(name);
    let st = std::process::Command::new("ffmpeg")
        .args(["-y", "-v", "error"])
        .args(input)
        .args(output)
        .arg(&out)
        .status()
        .expect("ffmpeg runs");
    assert!(st.success());
    std::fs::read(&out).unwrap()
}

fn good_wav(dir: &Path, name: &str, freq: u32) -> Vec<u8> {
    wav_secs(dir, name, freq, 32)
}

fn wav_secs(dir: &Path, name: &str, freq: u32, secs: u32) -> Vec<u8> {
    ffmpeg(
        dir,
        name,
        &[
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={freq}:duration={secs}"),
        ],
        &[
            "-ar",
            "48000",
            "-ac",
            "2",
            "-filter:a",
            "volume=8dB",
            "-c:a",
            "pcm_s16le",
        ],
    )
}

fn cover_png(dir: &Path) -> Vec<u8> {
    ffmpeg(
        dir,
        "cover.png",
        &["-f", "lavfi", "-i", "testsrc=size=3000x3000:duration=1"],
        &["-frames:v", "1"],
    )
}

/// Studio's upload flow: issue grant -> PUT bytes -> complete.
async fn upload(e: &Env, u: &User, kind: &str, content_type: &str, bytes: &[u8]) -> Uuid {
    let v = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{}/uploads", u.org),
        json!({"kind":kind,"size_bytes":bytes.len(),"content_type":content_type}),
        u,
    )
    .await;
    let key = v["expected_key"].as_str().unwrap();
    e.store.client_put(&v["grant"], key, bytes).await;
    let done = ok(
        &e.api,
        "POST",
        &format!(
            "/api/orgs/{}/uploads/{}/complete",
            u.org,
            v["upload_session_id"].as_str().unwrap()
        ),
        json!({"asset_id":v["asset_id"],"expected_key":key}),
        u,
    )
    .await;
    let sha = hex::encode(sha2::Sha256::digest(bytes));
    assert_eq!(done["sha256"], sha.as_str(), "{done}");
    // Round 2: the quarantine copy is deleted once the upload is registered.
    assert!(
        e.store.objects.lock().await.get(key).is_none(),
        "quarantine object removed after complete"
    );
    let asset = Uuid::parse_str(v["asset_id"].as_str().unwrap()).unwrap();
    let stored: Option<String> =
        sqlx::query_scalar("SELECT sha256 FROM catalog.assets WHERE id=$1")
            .bind(asset)
            .fetch_one(&e.owner)
            .await
            .unwrap();
    assert_eq!(
        stored.as_deref(),
        Some(sha.as_str()),
        "complete persists the hash"
    );
    asset
}

async fn row_version(e: &Env, release: Uuid) -> i64 {
    sqlx::query_scalar("SELECT row_version FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&e.owner)
        .await
        .unwrap()
}

async fn release_status(e: &Env, release: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(&e.owner)
        .await
        .unwrap()
}

/// A complete single built through the API (plus UPC/ISRC/artwork, which
/// have no API yet). Returns (release, track, artist).
async fn build_release(e: &Env, u: &User, audio: Uuid, cover: Uuid) -> (Uuid, Uuid, Uuid) {
    let o = u.org;
    let artist = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/artists"),
        json!({"name":"Sandbox Artist"}),
        u,
    )
    .await;
    let artist = Uuid::parse_str(artist["id"].as_str().unwrap()).unwrap();
    let profile = json!({"release_date":"2027-03-01","p_line":"℗ 2027 Sandbox","c_line":"© 2027 Sandbox","language":"ko","artist":"Sandbox Artist"});
    let rel = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases"),
        json!({"name":"Sandbox Song","release_type":"SINGLE","profile":profile}),
        u,
    )
    .await;
    let release = Uuid::parse_str(rel["id"].as_str().unwrap()).unwrap();
    let rv = row_version(e, release).await;
    let t = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases/{release}/tracks"),
        json!({"title":"Sandbox Song","disc_number":1,"track_number":1,"artist_id":artist,"asset_id":audio,"row_version":rv}),
        u,
    )
    .await;
    let track = Uuid::parse_str(t["id"].as_str().unwrap()).unwrap();
    let rv = row_version(e, release).await;
    ok(
        &e.api,
        "PUT",
        &format!("/api/orgs/{o}/releases/{release}/tracks/{track}/credits"),
        json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"},{"party_id":u.party,"role":"COMPOSER"}]}),
        u,
    )
    .await;
    sqlx::query("UPDATE catalog.releases SET upc='036000291452', artwork_asset_id=$1, row_version=row_version+1 WHERE id=$2")
        .bind(cover)
        .bind(release)
        .execute(&e.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc='USABC2600001' WHERE release_id=$1")
        .bind(release)
        .execute(&e.owner)
        .await
        .unwrap();
    (release, track, artist)
}

async fn consent_and_submit(e: &Env, u: &User, release: Uuid, key: &str) -> Uuid {
    let o = u.org;
    let c = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases/{release}/consents"),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":false}),
        u,
    )
    .await;
    let v = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases/{release}/submit"),
        json!({"consent_id":c["consent_id"],"minority_declared":false,"idempotency_key":key,"declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        u,
    )
    .await;
    Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap()
}

/// One worker cycle (as audeniq_worker); returns the job's resulting status.
async fn run_one(e: &Env, queue: &str, kind: &str) -> String {
    let job = operations::claim(&e.worker, queue, "sandbox-worker", 60)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{kind} job queued on {queue}"));
    assert_eq!(job.kind, kind);
    let store: Arc<dyn ObjectStore> = e.store.clone();
    operations::execute(&e.worker, &store, &job).await.unwrap();
    sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
        .bind(job.id)
        .fetch_one(&e.owner)
        .await
        .unwrap()
}

async fn submission(e: &Env, u: &User, release: Uuid) -> Value {
    ok(
        &e.api,
        "GET",
        &format!("/api/orgs/{}/releases/{release}/submission", u.org),
        json!({}),
        u,
    )
    .await
}

/// P0-1 + P0-2: the real upload API path persists the hash, Stage 1 actually
/// runs audio QC on it, and the whole API/worker pipeline works under the
/// production split roles (consent used to 500 on consent_packages; Stage 1
/// hit permission denied on allowed_transitions).
#[sqlx::test]
async fn real_upload_path_reaches_ready_for_delivery_under_split_roles(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, _, _) = build_release(&e, &u, audio, cover).await;
    let pre = ok(
        &e.api,
        "GET",
        &format!("/api/orgs/{}/releases/{release}/presubmit", u.org),
        json!({}),
        &u,
    )
    .await;
    assert!(pre["ready_to_submit"].as_bool().unwrap(), "{pre}");
    let revision = consent_and_submit(&e, &u, release, "sbx-happy").await;

    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&e, release).await, "STAGE1_PASSED");
    // Audio QC really ran: every audio check has a PASS row for this revision.
    let audio_pass: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT check_code) FROM operations.check_results WHERE revision_id=$1 AND status='PASS' AND check_code = ANY($2)",
    )
    .bind(revision)
    .bind(audeniq_core::qc::AUDIO_CHECK_CODES.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    .fetch_one(&e.owner)
    .await
    .unwrap();
    assert_eq!(
        audio_pass as usize,
        audeniq_core::qc::AUDIO_CHECK_CODES.len()
    );

    assert_eq!(run_one(&e, "rights", "stage2").await, "SUCCEEDED");
    assert_eq!(release_status(&e, release).await, "STAGE2_PASSED");
    assert_eq!(
        run_one(&e, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&e, release).await, "READY_FOR_DELIVERY");
    let s = submission(&e, &u, release).await;
    assert_eq!(s["status"], "READY_FOR_DELIVERY", "{s}");
}

/// P0-1 guard: an asset that somehow has no verified hash cannot be
/// submitted (it would silently skip audio QC).
#[sqlx::test]
async fn unverified_audio_cannot_be_submitted(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, _, _) = build_release(&e, &u, audio, cover).await;
    sqlx::query("UPDATE catalog.assets SET sha256=NULL WHERE id=$1")
        .bind(audio)
        .execute(&e.owner)
        .await
        .unwrap();
    let o = u.org;
    let pre = ok(
        &e.api,
        "GET",
        &format!("/api/orgs/{o}/releases/{release}/presubmit"),
        json!({}),
        &u,
    )
    .await;
    assert!(!pre["ready_to_submit"].as_bool().unwrap(), "{pre}");
    assert!(
        pre["gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "AUDIO_NOT_VERIFIED"),
        "{pre}"
    );
    let c = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases/{release}/consents"),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":false}),
        &u,
    )
    .await;
    let (s, v, _) = call(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases/{release}/submit"),
        json!({"consent_id":c["consent_id"],"minority_declared":false,"idempotency_key":"unverified","declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(v["error"]["code"], "AUDIO_NOT_VERIFIED");
}

/// P1-4: a Stage 1 job that exhausts its retries no longer leaves the
/// release stuck in STAGE1_RUNNING. It lands in STAGE1_CORRECTION with a
/// visible reason, the artist can replace the audio, and resubmission works.
#[sqlx::test]
async fn exhausted_stage1_is_recoverable_by_replacing_audio(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, track, artist) = build_release(&e, &u, audio, cover).await;
    let revision = consent_and_submit(&e, &u, release, "sbx-giveup-1").await;

    // Final attempt, and the analyzer cannot fetch the object.
    sqlx::query("UPDATE operations.jobs SET attempts=max_attempts-1 WHERE kind='stage1' AND pinned_revision_id=$1")
        .bind(revision)
        .execute(&e.owner)
        .await
        .unwrap();
    e.store.fail_get.store(true, Ordering::SeqCst);
    assert_eq!(run_one(&e, "qc", "stage1").await, "DEAD_LETTER");
    assert_eq!(release_status(&e, release).await, "STAGE1_CORRECTION");
    let s = submission(&e, &u, release).await;
    assert_eq!(s["status"], "STAGE1_CORRECTION", "{s}");
    let failed = s["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check_code"] == audeniq_core::submission::QC_ANALYSIS_FAILED)
        .unwrap_or_else(|| panic!("give-up reason visible: {s}"));
    assert_eq!(failed["status"], "CORRECTION_REQUIRED");
    assert!(!failed["detail"].as_str().unwrap_or("").is_empty());
    e.store.fail_get.store(false, Ordering::SeqCst);

    // Replace the audio while in correction (used to be 409), then resubmit.
    let new_audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "b.wav", 523),
    )
    .await;
    let rv = row_version(&e, release).await;
    ok(
        &e.api,
        "PUT",
        &format!("/api/orgs/{}/releases/{release}/tracks/{track}", u.org),
        json!({"title":"Sandbox Song","disc_number":1,"track_number":1,"artist_id":artist,"asset_id":new_audio,"row_version":rv}),
        &u,
    )
    .await;
    consent_and_submit(&e, &u, release, "sbx-giveup-2").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&e, release).await, "STAGE1_PASSED");
}

/// P1-4: a worker that dies mid-analysis on the last attempt (lease expiry)
/// also surfaces the failure on the release instead of stranding it.
#[sqlx::test]
async fn lease_expiry_dead_letter_surfaces_on_release(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, _, _) = build_release(&e, &u, audio, cover).await;
    let revision = consent_and_submit(&e, &u, release, "sbx-lease").await;
    sqlx::query("UPDATE operations.jobs SET attempts=max_attempts-1 WHERE kind='stage1' AND pinned_revision_id=$1")
        .bind(revision)
        .execute(&e.owner)
        .await
        .unwrap();
    let job = operations::claim(&e.worker, "qc", "crashing-worker", 60)
        .await
        .unwrap()
        .expect("stage1 queued");
    // The worker "crashes": its lease runs out without a heartbeat.
    sqlx::query(
        "UPDATE operations.jobs SET lease_until=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(job.id)
    .execute(&e.owner)
    .await
    .unwrap();
    // The next claim on the queue reclaims (and here dead-letters) it.
    assert!(
        operations::claim(&e.worker, "qc", "next-worker", 60)
            .await
            .unwrap()
            .is_none()
    );
    let status: String = sqlx::query_scalar("SELECT status FROM operations.jobs WHERE id=$1")
        .bind(job.id)
        .fetch_one(&e.owner)
        .await
        .unwrap();
    assert_eq!(status, "DEAD_LETTER");
    assert_eq!(release_status(&e, release).await, "STAGE1_CORRECTION");
}

/// P1-5: bad audio is stopped in Stage 1 with a correction the artist can
/// act on (float WAV here; the other defects are unit-tested in qc.rs).
#[sqlx::test]
async fn float_wav_goes_to_correction(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let float = ffmpeg(
        &dir.0,
        "float.wav",
        &["-f", "lavfi", "-i", "sine=frequency=440:duration=32"],
        &[
            "-ar",
            "48000",
            "-ac",
            "2",
            "-filter:a",
            "volume=8dB",
            "-c:a",
            "pcm_f32le",
        ],
    );
    let audio = upload(&e, &u, "AUDIO", "audio/wav", &float).await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, _, _) = build_release(&e, &u, audio, cover).await;
    consent_and_submit(&e, &u, release, "sbx-float").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&e, release).await, "STAGE1_CORRECTION");
    let s = submission(&e, &u, release).await;
    assert!(
        s["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["check_code"] == "AUDIO_SAMPLE_FORMAT_UNSUPPORTED"
                && c["status"] == "CORRECTION_REQUIRED"),
        "{s}"
    );
}

/// Same for Stage 2: an exhausted rights job parks the release in
/// STAGE2_REVIEW (human queue) instead of stranding it in STAGE2_RUNNING,
/// which is where all 25 sandbox uploads ended up.
#[sqlx::test]
async fn exhausted_stage2_goes_to_review(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, _, _) = build_release(&e, &u, audio, cover).await;
    let revision = consent_and_submit(&e, &u, release, "sbx-s2").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    sqlx::query("UPDATE operations.jobs SET attempts=max_attempts-1 WHERE kind='stage2' AND pinned_revision_id=$1")
        .bind(revision)
        .execute(&e.owner)
        .await
        .unwrap();
    let job = operations::claim(&e.worker, "rights", "crashing-worker", 60)
        .await
        .unwrap()
        .expect("stage2 queued");
    assert_eq!(job.kind, "stage2");
    sqlx::query(
        "UPDATE operations.jobs SET lease_until=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(job.id)
    .execute(&e.owner)
    .await
    .unwrap();
    assert!(
        operations::claim(&e.worker, "rights", "next-worker", 60)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(release_status(&e, release).await, "STAGE2_REVIEW");
}

// ---------------------------------------------------------------------------
// Round 2 (findings_round2.md)
// ---------------------------------------------------------------------------

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

async fn check_status(e: &Env, revision: Uuid, check: &str) -> String {
    sqlx::query_scalar(
        "SELECT status FROM operations.check_results WHERE revision_id=$1 AND check_code=$2 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(revision)
    .bind(check)
    .fetch_one(&e.owner)
    .await
    .unwrap_or_else(|_| panic!("{check} recorded"))
}

/// P0: masters above 64 MiB died in Stage 3 preflight (PREFLIGHT_FAILED x5 ->
/// STAGE3_PREPARING forever). A 6.5-minute 48 kHz stereo 16-bit WAV (~75 MB)
/// must reach READY_FOR_DELIVERY.
#[sqlx::test]
async fn master_over_64_mib_reaches_ready_for_delivery(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let wav = wav_secs(&dir.0, "long.wav", 440, 390);
    assert!(
        wav.len() > 64 * 1024 * 1024,
        "fixture is {} bytes",
        wav.len()
    );
    let audio = upload(&e, &u, "AUDIO", "audio/wav", &wav).await;
    drop(wav);
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, _, _) = build_release(&e, &u, audio, cover).await;
    consent_and_submit(&e, &u, release, "sbx-large").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&e, release).await, "STAGE1_PASSED");
    assert_eq!(run_one(&e, "rights", "stage2").await, "SUCCEEDED");
    assert_eq!(
        run_one(&e, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&e, release).await, "READY_FOR_DELIVERY");
}

/// P1/P2: control characters (BEL/ESC/VT), NUL and bidi overrides were
/// accepted, then failed packaging permanently (or 500 for NUL). They are a
/// 400 at input now.
#[sqlx::test]
async fn control_and_invisible_characters_are_rejected_at_input(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let o = u.org;
    for bad in [
        "Bad\u{7}\u{1b}[31mTitle\u{b}",
        "Nul\u{0}Title",
        "Song \u{202e}gnp.exe",
        "B\u{200b}T\u{200b}S",
    ] {
        let (s, v, _) = call(
            &e.api,
            "POST",
            &format!("/api/orgs/{o}/releases"),
            json!({"name":bad,"release_type":"SINGLE"}),
            Some(&u),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad:?}: {v}");
        assert_eq!(code(&v), "TEXT_INVALID_CHARACTERS", "{v}");
        assert!(v["error"]["message"].as_str().is_some(), "{v}");
    }
    // Hangul, emoji and ZWJ sequences stay allowed.
    ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases"),
        json!({"name":"봄날 🔥 👩\u{200d}🎤","release_type":"SINGLE"}),
        &u,
    )
    .await;
}

/// Impersonation hard block at every input point, including evasion
/// variants, and the non-matches that must keep working.
#[sqlx::test]
async fn protected_artist_names_are_refused_at_input(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let o = u.org;
    let refused = |v: &Value| code(v) == "ARTIST_NAME_PROTECTED";
    for name in [
        "Taylor Swift",
        "TAYLOR SWIFT",
        "T a y l o r  S w i f t",
        "T.aylor Swift",
        "Tay\u{200d}lor Swi\u{200c}ft", // ZWJ/ZWNJ pass text hygiene, not this
        "\u{422}aylor Sw\u{456}ft",     // Cyrillic Т, і
        "\u{ff34}\u{ff41}\u{ff59}\u{ff4c}\u{ff4f}\u{ff52} Swift", // fullwidth
        "DJ Kim (feat. Taylor Swift)",
        "DJ Kim ft. Taylor Swift",
        "DJ Kim x Taylor Swift",
        "Taylor Alison Swift",
    ] {
        let (s, v, _) = call(
            &e.api,
            "POST",
            &format!("/api/orgs/{o}/artists"),
            json!({"name":name}),
            Some(&u),
        )
        .await;
        assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{name:?}: {v}");
        assert!(refused(&v), "{name:?}: {v}");
    }
    // Zero-width space / RTL override: refused either way (text hygiene 400
    // fires first; the protected-name fold would also catch it).
    for name in ["Tay\u{200b}lor Swift", "Taylor\u{202e} Swift"] {
        let (s, v, _) = call(
            &e.api,
            "POST",
            &format!("/api/orgs/{o}/artists"),
            json!({"name":name}),
            Some(&u),
        )
        .await;
        assert!(s.is_client_error(), "{name:?}: {v}");
        assert!(
            matches!(
                code(&v),
                "TEXT_INVALID_CHARACTERS" | "ARTIST_NAME_PROTECTED"
            ),
            "{v}"
        );
    }
    // Unrelated names containing only part of the protected name pass.
    for name in [
        "Taylor",
        "Swift",
        "Taylor Made",
        "The Swift Boys",
        "Taylor Swiftly",
    ] {
        ok(
            &e.api,
            "POST",
            &format!("/api/orgs/{o}/artists"),
            json!({"name":name}),
            &u,
        )
        .await;
    }
    // Featured artist hidden in the release profile.
    let (s, v, _) = call(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases"),
        json!({"name":"Summer","release_type":"SINGLE","profile":{"artist":"DJ Kim","featured_artists":["with Taylor Swift"]}}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(refused(&v), "{v}");
    // Signature phrase in a track title.
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, track, artist) = build_release(&e, &u, audio, cover).await;
    let rv = row_version(&e, release).await;
    let (s, v, _) = call(
        &e.api,
        "PUT",
        &format!("/api/orgs/{o}/releases/{release}/tracks/{track}"),
        json!({"title":"Love Story (Taylor's Version)","disc_number":1,"track_number":1,"artist_id":artist,"asset_id":audio,"row_version":rv}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(refused(&v), "{v}");
    // Credited party carrying the protected name.
    let party = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES($1,$2,'PERSON','taylor  swift')")
        .bind(party)
        .bind(o)
        .execute(&e.owner)
        .await
        .unwrap();
    let rv = row_version(&e, release).await;
    let (s, v, _) = call(
        &e.api,
        "PUT",
        &format!("/api/orgs/{o}/releases/{release}/tracks/{track}/credits"),
        json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"},{"party_id":party,"role":"COMPOSER"}]}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert!(refused(&v), "{v}");
}

/// Submit re-checks the frozen texts (a title written by any other path is
/// refused before a revision exists), and Stage 1 catches names that were
/// added to the list after the draft was written, as a correctable state.
#[sqlx::test]
async fn protected_artist_names_are_rechecked_at_submit_and_stage1(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let o = u.org;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, _, _) = build_release(&e, &u, audio, cover).await;
    // Bypass input validation: write the title directly.
    sqlx::query("UPDATE catalog.tracks SET title='Taylor Swift' WHERE release_id=$1")
        .bind(release)
        .execute(&e.owner)
        .await
        .unwrap();
    let c = ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases/{release}/consents"),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":false}),
        &u,
    )
    .await;
    let (s, v, _) = call(
        &e.api,
        "POST",
        &format!("/api/orgs/{o}/releases/{release}/submit"),
        json!({"consent_id":c["consent_id"],"minority_declared":false,"idempotency_key":"sbx-prot","declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(code(&v), "ARTIST_NAME_PROTECTED");
    assert_eq!(release_status(&e, release).await, "DRAFT");

    // Clean title, submit, then the artist name becomes protected before
    // Stage 1 runs: correction, never a late packaging failure.
    sqlx::query("UPDATE catalog.tracks SET title='Sandbox Song' WHERE release_id=$1")
        .bind(release)
        .execute(&e.owner)
        .await
        .unwrap();
    let revision = consent_and_submit(&e, &u, release, "sbx-prot-2").await;
    sqlx::query("INSERT INTO catalog.protected_artists(name) VALUES('Sandbox Artist')")
        .execute(&e.owner)
        .await
        .unwrap();
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&e, release).await, "STAGE1_CORRECTION");
    assert_eq!(
        check_status(&e, revision, "ARTIST_NAME_PROTECTED").await,
        "CORRECTION_REQUIRED"
    );
}

/// A verified rights holder (explicit exception) is not locked out; the
/// exception is per org and revocable.
#[sqlx::test]
async fn allowlisted_org_may_use_protected_name(pool: PgPool) {
    let e = env(pool).await;
    let label = user(&e.api).await;
    let other = user(&e.api).await;
    sqlx::query(
        "INSERT INTO catalog.protected_artist_exceptions(protected_artist_id,org_id,reason,granted_by)
         SELECT id,$1,'verified label (test)','ops:test' FROM catalog.protected_artists WHERE name='Taylor Swift'",
    )
    .bind(label.org)
    .execute(&e.owner)
    .await
    .unwrap();
    ok(
        &e.api,
        "POST",
        &format!("/api/orgs/{}/artists", label.org),
        json!({"name":"Taylor Swift"}),
        &label,
    )
    .await;
    let (s, v, _) = call(
        &e.api,
        "POST",
        &format!("/api/orgs/{}/artists", other.org),
        json!({"name":"Taylor Swift"}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    sqlx::query("UPDATE catalog.protected_artist_exceptions SET revoked_at=now() WHERE org_id=$1")
        .bind(label.org)
        .execute(&e.owner)
        .await
        .unwrap();
    let (s, _, _) = call(
        &e.api,
        "POST",
        &format!("/api/orgs/{}/artists", label.org),
        json!({"name":"Taylor Swift (Live)"}),
        Some(&label),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}

/// P1: same-org UPC/ISRC reuse passed Stage 1/2 and dead-lettered in
/// packaging. It is a Stage 1 correction now, fixable and resubmittable.
#[sqlx::test]
async fn identifier_in_use_is_a_stage1_correction_and_recoverable(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let a1 = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let a2 = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "b.wav", 1250),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (_first, _, _) = build_release(&e, &u, a1, cover).await;
    // build_release assigns the same UPC/ISRC again.
    let (second, _, _) = build_release(&e, &u, a2, cover).await;
    let revision = consent_and_submit(&e, &u, second, "sbx-ident").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&e, second).await, "STAGE1_CORRECTION");
    assert_eq!(
        check_status(&e, revision, "IDENTIFIER_IN_USE").await,
        "CORRECTION_REQUIRED"
    );
    // Fix the codes (no identifier API yet) and resubmit.
    sqlx::query(
        "UPDATE catalog.releases SET upc='042100005264', row_version=row_version+1 WHERE id=$1",
    )
    .bind(second)
    .execute(&e.owner)
    .await
    .unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc='USABC2600002' WHERE release_id=$1")
        .bind(second)
        .execute(&e.owner)
        .await
        .unwrap();
    let revision = consent_and_submit(&e, &u, second, "sbx-ident-2").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        check_status(&e, revision, "IDENTIFIER_IN_USE").await,
        "PASS"
    );
    assert_ne!(release_status(&e, second).await, "STAGE1_CORRECTION");
}

/// P1: a permanent Stage 3 failure left the release in STAGE3_PREPARING with
/// no way out. STAGE3_CORRECTION is editable and resubmittable (split roles).
#[sqlx::test]
async fn stage3_correction_is_editable_and_resubmittable(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let o = u.org;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (release, track, artist) = build_release(&e, &u, audio, cover).await;
    let revision = consent_and_submit(&e, &u, release, "sbx-s3").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(run_one(&e, "rights", "stage2").await, "SUCCEEDED");
    assert_eq!(release_status(&e, release).await, "STAGE2_PASSED");
    let mut c = e.worker.acquire().await.unwrap();
    assert!(
        operations::stage3_give_up(
            &mut c,
            revision,
            "PREPARE_RELEASE_ERROR:IDENTIFIER_CONFLICT"
        )
        .await
        .unwrap()
    );
    drop(c);
    assert_eq!(release_status(&e, release).await, "STAGE3_CORRECTION");
    let s = submission(&e, &u, release).await;
    assert!(s.to_string().contains("STAGE3_PREPARATION_FAILED"), "{s}");
    let rv = row_version(&e, release).await;
    ok(
        &e.api,
        "PUT",
        &format!("/api/orgs/{o}/releases/{release}/tracks/{track}"),
        json!({"title":"Sandbox Song (fixed)","disc_number":1,"track_number":1,"artist_id":artist,"asset_id":audio,"row_version":rv}),
        &u,
    )
    .await;
    consent_and_submit(&e, &u, release, "sbx-s3-2").await;
    assert_eq!(release_status(&e, release).await, "SUBMITTED");
}

/// P1: `login:{email}` let anyone lock any account. The lockout is per
/// source+account now; the real user from another address still gets in.
#[sqlx::test]
async fn login_lockout_is_per_source_not_per_account(pool: PgPool) {
    let e = env(pool).await;
    let email = format!("{}@example.test", Uuid::new_v4());
    let good = json!({"email":email,"password":"Long-test-password-123!"});
    let bad = json!({"email":email,"password":"Wrong-password-123456"});
    let (s, _, _) = call(&e.api, "POST", "/api/auth/register", good.clone(), None).await;
    assert_eq!(s, StatusCode::OK);
    let attacker = [(
        audeniq_core::auth::CLIENT_IP_HEADER,
        "203.0.113.9".to_string(),
    )];
    for _ in 0..audeniq_core::auth::LOGIN_PER_SOURCE_ACCOUNT {
        let (s, _, _) = call_with(
            &e.api,
            "POST",
            "/api/auth/login",
            bad.clone(),
            None,
            &attacker,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    // The attacker's source is locked, even with the right password.
    let (s, v, _) = call_with(
        &e.api,
        "POST",
        "/api/auth/login",
        good.clone(),
        None,
        &attacker,
    )
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{v}");
    // The owner, from their own address, is not.
    let owner = [(
        audeniq_core::auth::CLIENT_IP_HEADER,
        "198.51.100.20".to_string(),
    )];
    let (s, v, _) = call_with(&e.api, "POST", "/api/auth/login", good, None, &owner).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    // X-Forwarded-For is never trusted as a source.
    assert_eq!(
        audeniq_core::auth::client_ip_key(&{
            let mut h = axum::http::HeaderMap::new();
            h.insert("x-forwarded-for", "1.2.3.4".parse().unwrap());
            h
        }),
        None
    );
}

/// P3: a single 200 KB header was accepted. Oversized headers are a 431.
#[sqlx::test]
async fn oversized_request_headers_get_431(pool: PgPool) {
    let e = env(pool).await;
    let big = [("x-big", "a".repeat(20 * 1024))];
    let (s, v, _) = call_with(&e.api, "GET", "/ready", json!({}), None, &big).await;
    assert_eq!(s, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE, "{v}");
    assert_eq!(code(&v), "REQUEST_HEADERS_TOO_LARGE");
}

/// P2: an already-submitted master attached to a new release with a
/// different ISRC went through unflagged. It is a Stage 1 review now.
#[sqlx::test]
async fn reused_master_under_new_isrc_goes_to_review(pool: PgPool) {
    let e = env(pool).await;
    let u = user(&e.api).await;
    let dir = tmpdir();
    let audio = upload(
        &e,
        &u,
        "AUDIO",
        "audio/wav",
        &good_wav(&dir.0, "a.wav", 440),
    )
    .await;
    let cover = upload(&e, &u, "IMAGE", "image/png", &cover_png(&dir.0)).await;
    let (first, _, _) = build_release(&e, &u, audio, cover).await;
    consent_and_submit(&e, &u, first, "sbx-reuse-1").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    let (second, _, _) = build_release(&e, &u, audio, cover).await;
    sqlx::query(
        "UPDATE catalog.releases SET upc='042100005264', row_version=row_version+1 WHERE id=$1",
    )
    .bind(second)
    .execute(&e.owner)
    .await
    .unwrap();
    sqlx::query("UPDATE catalog.tracks SET isrc='USABC2600003' WHERE release_id=$1")
        .bind(second)
        .execute(&e.owner)
        .await
        .unwrap();
    let revision = consent_and_submit(&e, &u, second, "sbx-reuse-2").await;
    assert_eq!(run_one(&e, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        check_status(&e, revision, "ASSET_REUSED").await,
        "REVIEW_REQUIRED"
    );
}
