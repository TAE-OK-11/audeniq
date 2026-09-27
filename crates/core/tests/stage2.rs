//! F3 Stage 2 review integration tests: real Postgres, worker pipeline from
//! the parked `stage2` job through decision, verification package, and the
//! `prepare_release` handoff.
use async_trait::async_trait;
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    error::{Error, Result},
    operations, review,
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

async fn build_submittable(
    app: &Router,
    pool: &PgPool,
    store: &Arc<FileStore>,
    u: &User,
    asset: Uuid,
) -> Uuid {
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
    // F4 preparation supplements: the merged worker fails closed without
    // UPC, cover artwork, ISRC and release metadata. All stage2 tests need
    // stage1 to reach STAGE1_PASSED.
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
        json!({"consent_id":consent_id,"minority_declared":false,"idempotency_key":key,"declarations":{"rights_confirmed":true,"adult_confirmed":true,"is_cover":false,"is_remix":false,"contains_samples":false,"ai_involved":false,"explicit_content":false}}),
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
    let d = std::env::temp_dir().join(format!("audeniq-f3-{}", Uuid::new_v4()));
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

#[sqlx::test]
async fn stage2_self_rights_holder_passes(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-s2-happy").await;

    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&pool, release).await, "STAGE1_PASSED");

    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_PASSED");

    // Verification package: decision PASS, empty DSP scope (no active routes),
    // pinned commercial split snapshot.
    let pkg: Value = sqlx::query_scalar(
        "SELECT body FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pkg["decision"], "PASS");
    assert_eq!(
        pkg["approved_scope"]["dsp_ids"].as_array().unwrap().len(),
        0
    );
    assert_eq!(pkg["commercial_split_snapshot"]["share_bps"], 10000);
    assert_eq!(pkg["rights_epoch"], 0);

    // F4: the Stage 3 prep handoff is a real handler now. It builds the
    // canonical snapshot, freezes the distribution package, and moves the
    // release to READY_FOR_DELIVERY.
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    let (attempts, status): (i32, String) = sqlx::query_as(
        "SELECT j.attempts, r.status FROM operations.jobs j JOIN catalog.releases r ON r.id=$1 WHERE j.kind='prepare_release'",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attempts, 1, "one claim, succeeded first try");
    assert_eq!(status, "READY_FOR_DELIVERY");
    let package_hash: String = sqlx::query_scalar(
        "SELECT dp.package_hash FROM distribution.distribution_packages dp JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id JOIN distribution.verification_packages vp ON vp.id=cr.verification_package_id WHERE vp.revision_id=$1",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(package_hash.len(), 64);

    // Submission status now exposes the verification package.
    let (s, v) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/releases/{release}/submission", u.org),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["verification_package"]["decision"], "PASS");
}

#[sqlx::test]
async fn stage2_duplicate_sha_in_other_org_is_review(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let a = user(&app).await;
    let b = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    // Same bytes in both orgs -> same SHA-256.
    let asset_a = register_asset(&pool, &store, &a, "good.wav", &wav).await;
    let asset_b = register_asset(&pool, &store, &b, "good.wav", &wav).await;
    let release_a = build_submittable(&app, &pool, &store, &a, asset_a).await;
    consent_and_submit(&app, &a, release_a, "k-s2-dup-a").await;

    // Org B holds the same audio on an active release (direct SQL: the claim
    // check only needs catalog rows, not API flow).
    let rel_b = Uuid::new_v4();
    let artist_b = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'release')")
        .bind(b.org)
        .bind(rel_b)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.releases(id,org_id,title,release_type,status,draft,row_version) VALUES($1,$2,'B','SINGLE','DRAFT','{}',1)")
        .bind(rel_b).bind(b.org).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'artist')")
        .bind(b.org)
        .bind(artist_b)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.artists(id,org_id,name) VALUES($1,$2,'B artist')")
        .bind(artist_b)
        .bind(b.org)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.tracks(id,org_id,release_id,title,disc_number,track_number,artist_id,asset_id) VALUES($1,$2,$3,'B track',1,1,$4,$5)")
        .bind(Uuid::new_v4()).bind(b.org).bind(rel_b).bind(artist_b).bind(asset_b)
        .execute(&pool).await.unwrap();

    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release_a).await, "STAGE2_REVIEW");

    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM operations.check_results WHERE revision_id=(SELECT current_revision_id FROM catalog.releases WHERE id=$1) AND check_code='S2_CATALOG_IDENTIFIERS'",
    )
    .bind(release_a)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(detail.contains("DUPLICATE_CLAIM"), "{detail}");
    // No verification package on REVIEW.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM distribution.verification_packages WHERE revision_id=(SELECT current_revision_id FROM catalog.releases WHERE id=$1)",
    )
    .bind(release_a)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn stage2_far_future_release_date_flagged(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    // A street date two years out is almost always a typo (2037 vs 2027).
    sqlx::query("UPDATE catalog.releases SET draft = draft || jsonb_build_object('release_date', to_char(now() + interval '2 years', 'YYYY-MM-DD')), row_version = row_version + 1 WHERE id=$1")
        .bind(release).execute(&pool).await.unwrap();
    consent_and_submit(&app, &u, release, "k-s2-future").await;

    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&pool, release).await, "STAGE1_PASSED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_REVIEW");

    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM operations.check_results WHERE revision_id=(SELECT current_revision_id FROM catalog.releases WHERE id=$1) AND check_code='S2_RELEASE_DATE_FAR_FUTURE'",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(detail.contains("more than a year"), "{detail}");
}

#[sqlx::test]
async fn stage2_lease_loss_returns_none(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    consent_and_submit(&app, &u, release, "k-s2-lease").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");

    let job = operations::claim(&pool, "rights", "test-worker", 60)
        .await
        .unwrap()
        .expect("stage2 job queued");
    // Forge a job with a wrong lock token: the worker must not decide.
    let forged = operations::Job {
        id: job.id,
        token: Uuid::new_v4(),
        kind: job.kind.clone(),
        payload: job.payload.clone(),
        attempts: job.attempts,
    };
    let out = review::run_stage2(&pool, &forged).await.unwrap();
    assert!(out.is_none());
    assert_eq!(release_status(&pool, release).await, "STAGE1_PASSED");
}

#[sqlx::test]
async fn stage2_override_requires_two_people(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-s2-ovr").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );

    // A second ACTIVE member of the same org (for the two-person rule) who
    // accepted the invitation long enough ago to be an eligible approver.
    let other = user(&app).await;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role,status,accepted_at) VALUES($1,$2,'EDITOR','ACTIVE',now()-interval '4 days')")
        .bind(u.org)
        .bind(other.user)
        .execute(&pool)
        .await
        .unwrap();
    // Ineligible approvers: a VIEWER and a member who joined just now.
    let viewer = user(&app).await;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role,status,accepted_at) VALUES($1,$2,'VIEWER','ACTIVE',now()-interval '30 days')")
        .bind(u.org)
        .bind(viewer.user)
        .execute(&pool)
        .await
        .unwrap();
    let newcomer = user(&app).await;
    sqlx::query("INSERT INTO identity.memberships(org_id,user_id,role,status,accepted_at) VALUES($1,$2,'EDITOR','ACTIVE',now())")
        .bind(u.org)
        .bind(newcomer.user)
        .execute(&pool)
        .await
        .unwrap();
    for (approver, want) in [
        (viewer.user, "APPROVER_ROLE_NOT_ELIGIBLE"),
        (newcomer.user, "APPROVER_TENURE_TOO_SHORT"),
    ] {
        let e = review::record_override(
            &pool,
            review::OverrideRequest {
                org: u.org,
                actor: u.user,
                revision_id,
                check_code: "S2_RIGHTS_SCOPE",
                proposed_status: "PASS",
                reason: "looks fine",
                second_approver: Some(approver),
                senior: true,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(e, Error::PolicyGate(c) if c == want), "{e:?}");
    }

    // Rights-class forced PASS without senior reviewer -> rejected.
    let e = review::record_override(
        &pool,
        review::OverrideRequest {
            org: u.org,
            actor: u.user,
            revision_id,
            check_code: "S2_RIGHTS_SCOPE",
            proposed_status: "PASS",
            reason: "looks fine",
            second_approver: None,
            senior: false,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("SENIOR_REVIEWER_REQUIRED")),
        "{e:?}"
    );

    // Senior but no second approver -> rejected.
    let e = review::record_override(
        &pool,
        review::OverrideRequest {
            org: u.org,
            actor: u.user,
            revision_id,
            check_code: "S2_RIGHTS_SCOPE",
            proposed_status: "PASS",
            reason: "looks fine",
            second_approver: None,
            senior: true,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("SECOND_APPROVER_REQUIRED")),
        "{e:?}"
    );

    // Second approver == actor -> rejected.
    let e = review::record_override(
        &pool,
        review::OverrideRequest {
            org: u.org,
            actor: u.user,
            revision_id,
            check_code: "S2_RIGHTS_SCOPE",
            proposed_status: "PASS",
            reason: "looks fine",
            second_approver: Some(u.user),
            senior: true,
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(e, Error::PolicyGate("SECOND_APPROVER_REQUIRED")),
        "{e:?}"
    );

    // Senior + different active member -> recorded.
    let id = review::record_override(
        &pool,
        review::OverrideRequest {
            org: u.org,
            actor: u.user,
            revision_id,
            check_code: "S2_RIGHTS_SCOPE",
            proposed_status: "PASS",
            reason: "verified grant chain",
            second_approver: Some(other.user),
            senior: true,
        },
    )
    .await
    .unwrap();
    let row: (String, String, Uuid) = sqlx::query_as(
        "SELECT original_status, proposed_status, second_approver_user_id FROM rights.review_overrides WHERE id=$1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "PASS");
    assert_eq!(row.1, "PASS");
    assert_eq!(row.2, other.user);

    // Non-rights-class override needs no second person.
    let id2 = review::record_override(
        &pool,
        review::OverrideRequest {
            org: u.org,
            actor: u.user,
            revision_id,
            check_code: "S2_META_CREDITS",
            proposed_status: "REVIEW_REQUIRED",
            reason: "recheck credits",
            second_approver: None,
            senior: false,
        },
    )
    .await
    .unwrap();
    assert_ne!(id2, id);

    // The original check row is untouched: overrides never mutate history.
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operations.check_results WHERE revision_id=$1 AND check_code='S2_RIGHTS_SCOPE' AND status='PASS'",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test]
async fn stage2_override_api_maps_seniority(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-s2-ovrapi").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );

    // The registering user is OWNER -> senior: non-rights-class override OK.
    let (s, v) = call(
        &app, "POST",
        &format!("/api/orgs/{}/reviews/overrides", u.org),
        json!({"revision_id":revision_id,"check_code":"S2_META_CREDITS","proposed_status":"REVIEW_REQUIRED","reason":"api recheck"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["override_id"].as_str().is_some());

    // Rights-class forced PASS is only a request until a second person
    // approves it from their own session; nothing is overridden yet.
    let (s, v) = call(
        &app, "POST",
        &format!("/api/orgs/{}/reviews/overrides", u.org),
        json!({"revision_id":revision_id,"check_code":"S2_RIGHTS_SCOPE","proposed_status":"PASS","reason":"api force"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["status"], "PENDING_SECOND_APPROVAL");
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM rights.review_overrides WHERE revision_id=$1 AND check_code='S2_RIGHTS_SCOPE'",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
    // Naming a second approver in the request is refused outright.
    let (s, v) = call(
        &app, "POST",
        &format!("/api/orgs/{}/reviews/overrides", u.org),
        json!({"revision_id":revision_id,"check_code":"S2_RIGHTS_SCOPE","proposed_status":"PASS","reason":"api force","second_approver_user_id":Uuid::new_v4()}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(
        v["error"]["code"],
        "SECOND_APPROVER_MUST_APPROVE_IN_OWN_SESSION"
    );
}

/// Activation model at the Stage 2 gate: a CONTRACTED adapter profile with
/// delivery_enabled=true but no contract route must NOT join the eligible
/// DSP set. delivery_enabled alone is only the operator kill-switch. The
/// MOCK profile in the same run stays eligible (control).
#[sqlx::test]
async fn stage2_contracted_profile_not_eligible_without_contract(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-s2-activation").await;

    // MOCK control: pin the seeded MockDSP profile to a fixed DSP id.
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(&pool)
        .await
        .unwrap();
    // Commercial partner with the operator switch on but no contract route.
    let contracted_dsp = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO execution.adapter_profiles(partner_id, display_name, profile_version, dsp_id, delivery_enabled, transport, activation_kind)
         VALUES('contracted-test','Contracted Test Partner','1',$1,true,'sftp','CONTRACTED')",
    )
    .bind(contracted_dsp)
    .execute(&pool)
    .await
    .unwrap();

    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );

    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM operations.check_results WHERE revision_id=$1 AND check_code='S2_DSP_ELIGIBILITY' ORDER BY created_at DESC LIMIT 1",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let parts: Vec<&str> = detail.splitn(2, " | ineligible: ").collect();
    assert_eq!(parts.len(), 2, "eligibility detail shape: {detail}");
    assert!(
        parts[0].contains(&mock_dsp.to_string()),
        "MOCK profile stays eligible: {detail}"
    );
    assert!(
        !parts[0].contains(&contracted_dsp.to_string()),
        "CONTRACTED without contract must not be eligible: {detail}"
    );
    assert!(
        parts[1].contains(&format!("{contracted_dsp}=INELIGIBLE_NO_CONTRACT")),
        "contract bypass is explicitly refused in the audit trail: {detail}"
    );
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
