//! F4 Stage 3 prep integration tests (Muse portion): the `prepare_release`
//! durable job builds the canonical snapshot, freezes the distribution
//! package, and moves the release to READY_FOR_DELIVERY. Real Postgres,
//! full worker pipeline stage1 -> stage2 -> prepare_release.
use audeniq_core::{distribution, operations, storage::ObjectStore};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

mod support;
use support::*;

fn tmpdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("audeniq-f4-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// UPC + cover artwork + ISRC + release metadata: DDEX ERN and the
/// preflight checks need all of them on the canonical snapshot.
async fn add_preparation_supplements(
    pool: &PgPool,
    store: &Arc<MemStore>,
    u: &User,
    release: Uuid,
) -> String {
    // UPC + cover artwork: DDEX ERN needs both on the canonical snapshot.
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

/// F5.5: prepare_release persists one real DDEX ERN 3.8.2 message per DSP
/// with configured DPIDs, in the same transaction as READY_FOR_DELIVERY.
/// The message is the interchange artifact; the synthetic ERN stays the
/// preflight integrity envelope.
#[sqlx::test]
async fn prepare_release_persists_ddex_ern_per_dsp(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    let _art_key = add_preparation_supplements(&pool, &store, &u, release).await;
    let _revision_id = consent_and_submit(&app, &u, release, "k-f4-ddex").await;
    // Sender DPID = org distributor identity (partner onboarding data).
    sqlx::query("UPDATE identity.orgs SET ddex_sender_dpid='TESTDPID-SENDER-0001' WHERE id=$1")
        .bind(u.org)
        .execute(&pool)
        .await
        .unwrap();
    // Pin the seeded MockDSP profile to a fixed DSP id BEFORE stage2 runs:
    // stage2's DSP-eligibility module reads activated adapter profiles.
    let mock_dsp = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET dsp_id=$1 WHERE partner_id='mockdsp'")
        .bind(mock_dsp)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "READY_FOR_DELIVERY");

    let package_id: Uuid = sqlx::query_scalar(
        "SELECT package_id FROM distribution.preparation_artifacts WHERE release_id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    // distribution.ddex_messages has FORCE ROW LEVEL SECURITY: even the
    // table owner must present the tenant via app.org_id.
    let mut authed = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id', $1, false)")
        .bind(u.org.to_string())
        .execute(&mut *authed)
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
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(xml.contains("<ern:NewReleaseMessage"));
    assert!(xml.contains("ern/382"));
    assert!(xml.contains("036000291452"), "UPC in release list");
    assert!(xml.contains("USABC2600001"), "ISRC in resource list");
    assert_eq!(sha, sha256_hex(xml.as_bytes()));
    // Exactly one DSP had configured DPIDs: no invented rows.
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM distribution.ddex_messages WHERE package_id=$1")
            .bind(package_id)
            .fetch_one(&mut *authed)
            .await
            .unwrap();
    assert_eq!(n, 1);
}

/// F5.5: without DPIDs configured, preparation persists no DDEX rows — party
/// identifiers are partner-onboarding data and are never invented.
#[sqlx::test]
async fn prepare_release_skips_ddex_without_dpids(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    let _art_key = add_preparation_supplements(&pool, &store, &u, release).await;
    let _revision_id = consent_and_submit(&app, &u, release, "k-f4-ddex-nodpid").await;
    // NOTE: no sender DPID on the org, no dsp_id pin on the profile.
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "READY_FOR_DELIVERY");
    let mut authed = pool.acquire().await.unwrap();
    sqlx::query("SELECT set_config('app.org_id', $1, false)")
        .bind(u.org.to_string())
        .execute(&mut *authed)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM distribution.ddex_messages")
        .fetch_one(&mut *authed)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test]
async fn prepare_release_identifier_conflict_dead_letters(pool: PgPool) {
    // Round 2: a same-org ISRC reuse is now a Stage 1 correction
    // (IDENTIFIER_IN_USE) instead of passing Stage 1/2 and dying in packaging.
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    add_preparation_supplements(&pool, &store, &u, release).await;
    // Another release in the same org already owns this ISRC.
    seed_conflicting_isrc(&pool, &u, "USABC2600001").await;
    let revision_id = consent_and_submit(&app, &u, release, "k-f4-conflict").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(release_status(&pool, release).await, "STAGE1_CORRECTION");
    let status: String = sqlx::query_scalar(
        "SELECT status FROM operations.check_results WHERE revision_id=$1 AND check_code='IDENTIFIER_IN_USE'",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "CORRECTION_REQUIRED");
}

#[sqlx::test]
async fn prepare_release_late_identifier_conflict_is_recoverable(pool: PgPool) {
    // A conflict that appears after Stage 1 (a race with another release)
    // is still permanent for packaging, but the release lands in
    // STAGE3_CORRECTION with an explanation instead of a silent
    // STAGE3_PREPARING dead end.
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    add_preparation_supplements(&pool, &store, &u, release).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-f4-late-conflict").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_PASSED");
    seed_conflicting_isrc(&pool, &u, "USABC2600001").await;

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
    assert_eq!(release_status(&pool, release).await, "STAGE3_CORRECTION");
    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM operations.check_results WHERE revision_id=$1 AND check_code='STAGE3_PREPARATION_FAILED' AND status='CORRECTION_REQUIRED'",
    )
    .bind(revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!detail.is_empty());
}

#[sqlx::test]
async fn freeze_package_is_idempotent(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    add_preparation_supplements(&pool, &store, &u, release).await;
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

/// Regression: the READY_FOR_DELIVERY idempotent path of run_prepare_release
/// must report the real ddex_messages count. ddex_messages is FORCE RLS, so
/// a pool-direct COUNT without app.org_id would silently return 0 and the
/// retry summary would misreport; the handler authorizes the read's org in
/// a short transaction.
#[sqlx::test]
async fn prepare_release_retry_reports_ddex_count_under_rls(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let wav = make_good_wav(&dir);
    let asset = register_asset(&pool, &store, &u, "good.wav", &wav).await;
    let release = build_submittable(&app, &pool, &u, asset).await;
    let _art_key = add_preparation_supplements(&pool, &store, &u, release).await;
    let revision_id = consent_and_submit(&app, &u, release, "k-f4-ddex-retry").await;
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
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "READY_FOR_DELIVERY");

    let (package_id, verification_package_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT dp.id, cr.verification_package_id
         FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         JOIN catalog.releases r ON r.org_id=cr.org_id AND r.id=cr.release_id
         WHERE r.id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Simulate the worker-crash retry: re-run the handler on a pool with no
    // app.org_id set anywhere. Without the fix, the FORCE RLS count reads 0.
    let retry_job = operations::Job {
        id: Uuid::new_v4(),
        token: Uuid::new_v4(),
        kind: "prepare_release".to_string(),
        payload: json!({"revision_id": revision_id, "verification_package_id": verification_package_id}),
        attempts: 1,
        release_id: None,
    };
    let dyn_store: Arc<dyn ObjectStore> = store.clone();
    let summary = distribution::run_prepare_release(&pool, &dyn_store, &retry_job)
        .await
        .unwrap()
        .expect("idempotent completion");
    assert_eq!(summary.package_id, package_id);
    assert_eq!(summary.release_status, "READY_FOR_DELIVERY");
    assert!(
        !summary.returned_to_s2,
        "retry must not bounce back to stage 2"
    );
    assert_eq!(
        summary.ddex_messages, 1,
        "retry must report the persisted DDEX row, not 0"
    );
    // No duplicate package or DDEX rows from the retry.
    let pkgs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM distribution.distribution_packages dp
         JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id
         JOIN catalog.releases r ON r.org_id=cr.org_id AND r.id=cr.release_id
         WHERE r.id=$1",
    )
    .bind(release)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pkgs, 1);
}
