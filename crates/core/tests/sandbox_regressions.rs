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
    ffmpeg(
        dir,
        name,
        &[
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency={freq}:duration=32"),
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
