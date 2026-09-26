//! Album-scale distribution timing test (manual run, not part of CI gates):
//! 10 FLAC tracks x 3:30 (210s) through the real pipeline — QC (stage1) ->
//! rights (stage2) -> prepare_release (canonical snapshot + frozen package +
//! DDEX ERN per DSP) — with per-phase wall-clock timings printed at the end.
//!
//! Run with:
//!   cargo test -p audeniq-core --test album_flac10 -- --nocapture
use async_trait::async_trait;
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database, ddex_xsd,
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
    path::{Path, PathBuf},
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
const TRACKS: usize = 10;
const TRACK_SECS: u32 = 210; // 3:30

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

/// 3:30 stereo FLAC master, distinct pitch per track so every file differs.
fn make_flac(dir: &Path, idx: usize) -> Vec<u8> {
    let out = dir.join(format!("track{idx:02}.flac"));
    let freq = 220 + (idx as u32) * 55;
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

async fn create_album(app: &Router, u: &User) -> Uuid {
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/releases", u.org),
        json!({"name":"Timing Test Album","release_type":"ALBUM"}),
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
        json!({"name":"Timing Artist"}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}

async fn row_version(pool: &PgPool, release: Uuid) -> i64 {
    sqlx::query_scalar("SELECT row_version FROM catalog.releases WHERE id=$1")
        .bind(release)
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

async fn run_one(pool: &PgPool, store: &Arc<FileStore>, queue: &str, kind: &str) -> String {
    let job = operations::claim(pool, queue, "test-worker", 600)
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
    let d = std::env::temp_dir().join(format!("audeniq-album10-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[sqlx::test]
async fn album_10x_flac_330_distribution_timing(pool: PgPool) {
    let t_total = Instant::now();
    let mut phases: Vec<(&str, std::time::Duration)> = vec![];

    let t = Instant::now();
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    phases.push(("app+user setup", t.elapsed()));

    // 1. Fixture generation: 10 x 3:30 FLAC.
    let t = Instant::now();
    let dir = tmpdir();
    let mut flacs: Vec<Vec<u8>> = Vec::with_capacity(TRACKS);
    for i in 0..TRACKS {
        flacs.push(make_flac(&dir, i));
    }
    let total_mb: f64 = flacs.iter().map(|b| b.len()).sum::<usize>() as f64 / 1_048_576.0;
    phases.push(("ffmpeg 10x FLAC 3:30", t.elapsed()));

    // 2. Register 10 assets.
    let t = Instant::now();
    let mut assets = Vec::with_capacity(TRACKS);
    for (i, bytes) in flacs.iter().enumerate() {
        assets.push(register_asset(&pool, &store, &u, &format!("track{i:02}.flac"), bytes).await);
    }
    // Drop the raw bytes; the store holds its own copies.
    drop(flacs);
    phases.push(("register 10 assets", t.elapsed()));

    // 3. Build the album: release + 10 tracks + credits + UPC/artwork/ISRCs.
    let t = Instant::now();
    let release = create_album(&app, &u).await;
    let artist = create_artist(&app, &u).await;
    sqlx::query("UPDATE catalog.releases SET draft = draft || '{\"release_date\":\"2027-06-01\",\"language\":\"ko\",\"artist\":\"Timing Artist\",\"p_line\":\"P 2027 Timing Label\",\"c_line\":\"C 2027 Timing Label\"}'::jsonb, row_version = row_version + 1 WHERE id=$1")
        .bind(release).execute(&pool).await.unwrap();
    for (i, asset) in assets.iter().enumerate() {
        let n = i + 1;
        let rv = row_version(&pool, release).await;
        let (s, v) = call(
            &app,
            "POST",
            &format!("/api/orgs/{}/releases/{release}/tracks", u.org),
            json!({"title":format!("Track {n:02}"),"disc_number":1,"track_number":n,"artist_id":artist,"asset_id":asset,"row_version":rv}),
            Some(&u),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "track {n}: {v}");
        let track = Uuid::parse_str(v["id"].as_str().unwrap()).unwrap();
        let rv = row_version(&pool, release).await;
        let (s, v) = call(
            &app,
            "PUT",
            &format!("/api/orgs/{}/releases/{release}/tracks/{track}/credits", u.org),
            json!({"row_version":rv,"credits":[{"party_id":u.party,"role":"ARTIST"},{"party_id":u.party,"role":"COMPOSER"}]}),
            Some(&u),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "credits {n}: {v}");
        let isrc = format!("USABC26{n:05}");
        sqlx::query("UPDATE catalog.tracks SET isrc=$1 WHERE id=$2")
            .bind(&isrc)
            .bind(track)
            .execute(&pool)
            .await
            .unwrap();
    }
    // Cover artwork + UPC.
    let art_id = Uuid::new_v4();
    let art_key = format!("registered/{}/cover.png", u.org);
    let art_bytes = cover_png();
    store
        .files
        .lock()
        .await
        .insert(art_key.clone(), (art_bytes.to_vec(), "image/png".into()));
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(u.org)
        .bind(art_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,sha256,state) VALUES($1,$2,'IMAGE',$3,$4,'image/png',$5,'REGISTERED')")
        .bind(art_id).bind(u.org).bind(&art_key).bind(art_bytes.len() as i64).bind(sha256_hex(art_bytes))
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE catalog.releases SET upc='036000291452', artwork_asset_id=$1, row_version = row_version + 1 WHERE id=$2")
        .bind(art_id).bind(release).execute(&pool).await.unwrap();
    // Sender DPID + mock DSP pin so a real DDEX ERN is persisted per DSP.
    sqlx::query("UPDATE identity.orgs SET ddex_sender_dpid='TESTDPID-SENDER-0001' WHERE id=$1")
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
    phases.push(("build album (10 tracks+credits+ids)", t.elapsed()));

    // 4. Consent + submit.
    let t = Instant::now();
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/consents", u.org),
        json!({"parties":[{"party_id":u.party,"role":"ARTIST"}],"minority_declared":false}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let consent_id = Uuid::parse_str(v["consent_id"].as_str().unwrap()).unwrap();
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/releases/{release}/submit", u.org),
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":"k-album10-flac","declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let revision_id = Uuid::parse_str(v["revision_id"].as_str().unwrap()).unwrap();
    phases.push(("consent+submit", t.elapsed()));

    // 5. Stage 1 QC (ffprobe + full decode analysis per track).
    let t = Instant::now();
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    phases.push(("stage1 QC (10 tracks)", t.elapsed()));
    // QC persisted measured specs?
    let specs: Vec<(Option<f64>, Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT duration_secs, sample_rate, channels FROM catalog.assets WHERE id = ANY($1) ORDER BY object_key",
    )
    .bind(&assets)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(specs.len(), TRACKS);
    for (dur, sr, ch) in &specs {
        let d = dur.expect("qc persisted duration");
        assert!((d - 210.0).abs() < 1.0, "duration ~210s, got {d}");
        assert_eq!(*sr, Some(44100));
        assert_eq!(*ch, Some(2));
    }

    // 6. Stage 2 rights.
    let t = Instant::now();
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_PASSED");
    phases.push(("stage2 rights", t.elapsed()));

    // 7. prepare_release: canonical snapshot + freeze + DDEX ERN.
    let t = Instant::now();
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    phases.push(("prepare_release (canonical+freeze+DDEX)", t.elapsed()));
    assert_eq!(release_status(&pool, release).await, "READY_FOR_DELIVERY");

    // 8. Album-level assertions.
    let t = Instant::now();
    let track_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM catalog.tracks WHERE release_id=$1")
            .bind(release)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(track_count, TRACKS as i64);
    let mut authed = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id',$1,false)")
        .bind(u.org.to_string())
        .execute(&mut *authed)
        .await
        .unwrap();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT kind, identifier FROM distribution.identifier_assignments WHERE release_id=$1 ORDER BY kind, identifier",
    )
    .bind(release)
    .fetch_all(&mut *authed)
    .await
    .unwrap();
    assert_eq!(rows.len(), TRACKS + 1, "UPC + 10 ISRCs");
    assert!(rows.iter().any(|(k, v)| k == "UPC" && v == "036000291452"));
    for n in 1..=TRACKS {
        let isrc = format!("USABC26{n:05}");
        assert!(
            rows.iter().any(|(k, v)| k == "ISRC" && v == &isrc),
            "ledger has {isrc}"
        );
    }
    let package_id: Uuid = sqlx::query_scalar(
        "SELECT package_id FROM distribution.preparation_artifacts WHERE release_id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (xml, sha, sender_dpid, recipient_dpid): (String, String, String, String) =
        sqlx::query_as(
            "SELECT ern_xml, ern_sha256, sender_dpid, recipient_dpid FROM distribution.ddex_messages WHERE package_id=$1 AND dsp_id=$2",
        )
        .bind(package_id)
        .bind(mock_dsp)
        .fetch_one(&mut *authed)
        .await
        .unwrap();
    assert_eq!(sender_dpid, "TESTDPID-SENDER-0001");
    assert_eq!(recipient_dpid, "TESTDPID-MOCKDSP-0001");
    assert!(xml.contains("<ern:NewReleaseMessage"));
    assert!(xml.contains("036000291452"), "UPC in ERN");
    for n in 1..=TRACKS {
        let isrc = format!("USABC26{n:05}");
        assert!(xml.contains(&isrc), "ISRC {isrc} in ERN");
    }
    assert_eq!(sha, sha256_hex(xml.as_bytes()));
    let ern_kb = xml.len() as f64 / 1024.0;
    // DDEX XSD validation of the album ERN (fail-closed like the pipeline).
    let xsd_ok = ddex_xsd::validate_ern_382_xml(&xml).is_ok();
    phases.push(("album assertions + ERN XSD check", t.elapsed()));

    eprintln!("\n===== ALBUM DISTRIBUTION TIMING (10 x FLAC 3:30, {total_mb:.1} MB) =====");
    for (name, d) in &phases {
        eprintln!("  {:42} {:>8.1?}", name, d);
    }
    eprintln!("  {:42} {:>8.1?}", "TOTAL", t_total.elapsed());
    eprintln!("  ERN XML size: {ern_kb:.1} KB, XSD valid: {xsd_ok}");
    eprintln!("  revision: {revision_id}");
    eprintln!("============================================================\n");
}

/// A real 3000x3000 PNG cover: Stage 1 QCs the release artwork (size,
/// square), so a fake header no longer passes. Generated once per binary.
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
