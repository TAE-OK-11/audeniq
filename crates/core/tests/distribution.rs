//! F4 Stage 3 prep integration tests (Muse portion): the `prepare_release`
//! durable job builds the canonical snapshot, freezes the distribution
//! package, and moves the release to READY_FOR_DELIVERY. Real Postgres,
//! full worker pipeline stage1 -> stage2 -> prepare_release.
use async_trait::async_trait;
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database, distribution,
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

fn make_good_wav(dir: &std::path::Path) -> Vec<u8> {
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

/// Run one worker cycle over the given queue; returns the job's terminal status.
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

fn tmpdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("audeniq-f4-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn release_status(pool: &PgPool, release: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM catalog.releases WHERE id=$1")
        .bind(release)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// UPC + cover artwork + ISRC + release metadata: DDEX ERN and the
/// preflight checks need all of them on the canonical snapshot.
async fn add_preparation_supplements(
    pool: &PgPool,
    store: &Arc<FileStore>,
    u: &User,
    release: Uuid,
) -> String {
    // UPC + cover artwork: DDEX ERN needs both on the canonical snapshot.
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
    art_key
}

/// Seed a second release/track/revision in the same org and claim `isrc` on
/// its track through the real ledger path, so the release under test hits a
/// cross-target identifier conflict in its worker.
async fn seed_conflicting_isrc(pool: &PgPool, u: &User, isrc: &str) {
    let org = u.org;
    let release = Uuid::new_v4();
    let revision = Uuid::new_v4();
    let track = Uuid::new_v4();
    let artist = Uuid::new_v4();
    let user_id: Uuid = sqlx::query_scalar("SELECT id FROM identity.users WHERE party_id=$1")
        .bind(u.party)
        .fetch_one(pool)
        .await
        .unwrap();
    for (id, kind) in [(release, "release"), (artist, "artist")] {
        sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,$3)")
            .bind(org)
            .bind(id)
            .bind(kind)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO catalog.releases(id,org_id,title,release_type) VALUES($1,$2,'Other','SINGLE')",
    )
    .bind(release)
    .bind(org)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO catalog.artists(id,org_id,name) VALUES($1,$2,'Other')")
        .bind(artist)
        .bind(org)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.tracks(id,org_id,release_id,title,disc_number,track_number,artist_id) VALUES($1,$2,$3,'Other',1,1,$4)")
        .bind(track).bind(org).bind(release).bind(artist).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO catalog.application_revisions(id,org_id,release_id,revision,body,body_hash,consent_package_hash,created_by) VALUES($1,$2,$3,1,'{}',$4,$4,$5)")
        .bind(revision).bind(org).bind(release).bind("a".repeat(64)).bind(user_id).execute(pool).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,false)")
        .bind(org.to_string())
        .execute(&mut *conn)
        .await
        .unwrap();
    audeniq_core::identifiers::record_existing(
        &mut conn,
        &audeniq_core::identifiers::ExistingAssignment {
            org_id: org,
            release_id: release,
            track_id: Some(track),
            revision_id: revision,
            kind: audeniq_core::identifiers::IdentifierKind::Isrc,
            value: isrc,
        },
    )
    .await
    .unwrap();
}

#[sqlx::test]
async fn prepare_release_happy_path_freezes_package(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    let art_key = add_preparation_supplements(&pool, &store, &u, release).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-f4-happy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_PASSED");

    // Stage 3 prep: the parked job is now a real handler.
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "READY_FOR_DELIVERY");

    // Durable handoff: prepare_release enqueues delivery.enqueue for the
    // frozen package in the same transaction as READY_FOR_DELIVERY.
    let package_id: Uuid = sqlx::query_scalar(
        "SELECT package_id FROM distribution.preparation_artifacts WHERE release_id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (kind, payload): (String, Value) = sqlx::query_as(
        "SELECT kind, payload FROM operations.jobs WHERE queue='delivery' AND kind='delivery.enqueue'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kind, "delivery.enqueue");
    assert_eq!(payload["package_id"], Value::String(package_id.to_string()));

    // Canonical snapshot pins the Stage 2 outputs.
    let vp_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (canonical_id, canonical_hash, body): (Uuid, String, Value) = sqlx::query_as(
        "SELECT id, canonical_hash, body FROM distribution.canonical_releases WHERE verification_package_id=$1",
    )
    .bind(vp_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(canonical_hash.chars().all(|c| c.is_ascii_hexdigit()) && canonical_hash.len() == 64);
    let vp_hash: String = sqlx::query_scalar(
        "SELECT package_hash FROM distribution.verification_packages WHERE id=$1",
    )
    .bind(vp_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(body["verification_package_hash"], Value::String(vp_hash));
    assert_eq!(body["revision_id"], Value::String(revision_id.to_string()));
    assert_eq!(body["release_id"], Value::String(release.to_string()));
    assert!(!body["tracks"].as_array().unwrap().is_empty());
    assert_eq!(body["schema_version"], Value::from(2));
    assert_eq!(body["upc"], Value::String("036000291452".into()));
    assert_eq!(body["artwork"]["object_key"], Value::String(art_key));
    assert_eq!(
        body["artwork"]["content_type"],
        Value::String("image/png".into())
    );
    let t0 = &body["tracks"].as_array().unwrap()[0];
    assert!(
        t0["asset_object_key"]
            .as_str()
            .unwrap()
            .contains("good.wav")
    );
    // The frozen package content-addresses the snapshot.
    let (package_id, package_hash, pbody, pstatus): (Uuid, String, Value, String) =
        sqlx::query_as(
            "SELECT id, package_hash, body, status FROM distribution.distribution_packages WHERE canonical_release_id=$1",
        )
        .bind(canonical_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(package_hash.chars().all(|c| c.is_ascii_hexdigit()) && package_hash.len() == 64);
    assert_eq!(pstatus, "PREPARED");
    assert_eq!(pbody["canonical_hash"], Value::String(canonical_hash));
    assert_eq!(
        pbody["canonical_release_id"],
        Value::String(canonical_id.to_string())
    );
    // Preparation artifacts: one append-only row per frozen package with the
    // ERN hash, preflight report and route plan.
    let (ern_sha, preflight, route): (String, Value, Value) = sqlx::query_as(
        "SELECT ern_sha256, preflight_report, route_plan FROM distribution.preparation_artifacts WHERE package_id=$1",
    )
    .bind(package_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(ern_sha.chars().all(|c| c.is_ascii_hexdigit()) && ern_sha.len() == 64);
    for check in ["xml", "metadata", "files", "rights"] {
        assert_eq!(preflight[check], Value::String("Pass".into()), "{check}");
    }
    assert!(
        route
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["delivery_enabled"] == Value::Bool(false))
    );
    // Identifier ledger: the UPC and the track ISRC are recorded, bound to
    // this org/release/revision. The ledger is RLS-protected, so the test
    // authorizes its org on one connection like the worker does.
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,false)")
        .bind(u.org.to_string())
        .execute(&mut *conn)
        .await
        .unwrap();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT kind, identifier FROM distribution.identifier_assignments WHERE release_id=$1 ORDER BY kind",
    )
    .bind(release)
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("ISRC".to_string(), "USABC2600001".to_string()),
            ("UPC".to_string(), "036000291452".to_string()),
        ]
    );
}

#[sqlx::test]
async fn prepare_release_identifier_conflict_dead_letters(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    add_preparation_supplements(&pool, &store, &u, release).await;
    // Another release in the same org already owns this ISRC.
    seed_conflicting_isrc(&pool, &u, "USABC2600001").await;
    let _revision_id = consent_and_submit(&app, &u, release, "k-f4-conflict").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_PASSED");

    // The conflict is permanent: no retry can fix a cross-target claim, so the
    // job dead-letters and the release never reaches READY_FOR_DELIVERY.
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "DEAD_LETTER"
    );
    let err: Option<String> = sqlx::query_scalar(
        "SELECT last_error FROM operations.jobs WHERE kind='prepare_release' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(err.unwrap().contains("IDENTIFIER_CONFLICT"));
    assert_eq!(release_status(&pool, release).await, "STAGE3_PREPARING");
}

#[sqlx::test]
async fn freeze_package_is_idempotent(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-f4-idem").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    let vp_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();

    let canonical = distribution::build_canonical(&pool, vp_id).await.unwrap();
    let first = distribution::freeze_package(&pool, &canonical)
        .await
        .unwrap();
    let second = distribution::freeze_package(&pool, &canonical)
        .await
        .unwrap();
    assert_eq!(first, second, "same canonical snapshot -> same package row");
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM distribution.distribution_packages dp JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id WHERE cr.verification_package_id=$1",
    )
    .bind(vp_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}
