//! F3 Stage 2 review integration tests: real Postgres, worker pipeline from
//! the parked `stage2` job through decision, verification package, and the
//! `prepare_release` handoff.
use audeniq_core::{error::Error, operations, review};
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;

mod support;
use support::*;

async fn build_submittable(
    app: &Router,
    pool: &PgPool,
    store: &Arc<MemStore>,
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
    let art_key = format!("registered/{}/{art_id}/cover.png", u.org);
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

fn tmpdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("audeniq-f3-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn varied_reference_wav(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("external.wav");
    assert!(std::process::Command::new("ffmpeg").args([
        "-y","-v","error","-f","lavfi","-i",
        "aevalsrc=0.22*sin(2*PI*(210*t+12*t*t))+0.08*sin(2*PI*731*t)+0.07*sin(2*PI*(913*t+6*t*t)):s=48000:d=48",
        "-ac","2","-c:a","pcm_s16le"]).arg(&path).status().unwrap().success());
    path
}

fn external_metadata() -> audeniq_core::external_recordings::ReferenceMetadata {
    audeniq_core::external_recordings::ReferenceMetadata {
        title: "External original recording".into(),
        artist: "External original artist".into(),
        isrc: Some("GBABC2600123".into()),
        source_url: "https://example.org/artist/original".into(),
        permission_basis: "Synthetic CI fixture; testing only".into(),
    }
}

#[sqlx::test]
async fn external_reencoded_recording_is_held_without_another_org_catalog(pool: PgPool) {
    async fn assert_stage1_passed(pool: &PgPool, release: Uuid, revision: Uuid, encoding: &str) {
        let failures: Vec<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT check_code,status,detail FROM operations.check_results WHERE revision_id=$1 AND status IN ('BLOCKED','CORRECTION_REQUIRED','TECHNICAL_RETRY') ORDER BY check_code",
        )
        .bind(revision)
        .fetch_all(pool)
        .await
        .unwrap();
        assert_eq!(
            release_status(pool, release).await,
            "STAGE1_PASSED",
            "{encoding} must reach recording comparison; checks: {failures:?}"
        );
    }
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let original = varied_reference_wav(&dir);
    let copy = dir.join("submitted.flac");
    assert!(
        std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&original)
            .args(["-c:a", "flac"])
            .arg(&copy)
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&copy).unwrap();
    assert_ne!(
        sha256_hex(&bytes),
        sha256_hex(&std::fs::read(&original).unwrap())
    );
    let asset = register_asset(&pool, &store, &u, "submitted.flac", &bytes).await;
    // The shared helper registers WAV fixtures. Declare this FLAC accurately
    // so Stage 1 tests the recording rather than rejecting a MIME mismatch.
    let key: String = sqlx::query_scalar(
        "UPDATE catalog.assets SET content_type='audio/flac' WHERE id=$1 RETURNING object_key",
    )
    .bind(asset)
    .fetch_one(&pool)
    .await
    .unwrap();
    store.files.lock().await.get_mut(&key).unwrap().1 = "audio/flac".into();
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision = consent_and_submit(&app, &u, release, "external-reference-copy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_stage1_passed(&pool, release, revision, "FLAC").await;
    // The external recording becomes known AFTER Stage 1. Stage 2 must
    // compare freshly, with no new submitted-file download or other org.
    audeniq_core::external_recordings::import(&pool, "ci-operator", external_metadata(), &original)
        .await
        .unwrap();
    let before = store.get_calls.load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(
        store.get_calls.load(std::sync::atomic::Ordering::SeqCst),
        before
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_REVIEW");
    assert_eq!(
        check_status(
            &pool,
            revision,
            audeniq_core::external_recordings::CHECK_CODE
        )
        .await,
        "REVIEW_REQUIRED"
    );
    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM operations.check_results WHERE revision_id=$1 AND check_code=$2",
    )
    .bind(revision)
    .bind(audeniq_core::external_recordings::CHECK_CODE)
    .fetch_one(&pool)
    .await
    .unwrap();
    let report: Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(
        report["matches"][0]["signal"], "AUDIO_FINGERPRINT",
        "{report}"
    );
    assert_eq!(report["matches"][0]["artist"], "External original artist");
    assert_eq!(report["copyright_verdict"], "NOT_DETERMINED");
    assert_eq!(report["global_catalog_checked"], false);
    let packages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(packages, 0);
    // Also cover a common copy-upload path: lossy MP3 encoded back into a
    // technically accepted WAV. The master SHA differs; the recording holds.
    let mp3 = dir.join("copied.mp3");
    let restored = dir.join("restored.wav");
    assert!(
        std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&original)
            .args(["-c:a", "libmp3lame", "-b:a", "192k"])
            .arg(&mp3)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&mp3)
            .args(["-ar", "48000", "-c:a", "pcm_s16le"])
            .arg(&restored)
            .status()
            .unwrap()
            .success()
    );
    let asset = register_asset(
        &pool,
        &store,
        &u,
        "restored.wav",
        &std::fs::read(&restored).unwrap(),
    )
    .await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision = consent_and_submit(&app, &u, release, "external-reference-lossy-copy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_stage1_passed(&pool, release, revision, "MP3 restored to WAV").await;
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(
        check_status(
            &pool,
            revision,
            audeniq_core::external_recordings::CHECK_CODE
        )
        .await,
        "REVIEW_REQUIRED"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn external_reference_isrc_claim_and_missing_audio_are_distinct(pool: PgPool) {
    audeniq_core::database::MIGRATOR.run(&pool).await.unwrap();
    let dir = tmpdir();
    let original = varied_reference_wav(&dir);
    let id = audeniq_core::external_recordings::import(
        &pool,
        "ci-operator",
        external_metadata(),
        &original,
    )
    .await
    .unwrap();
    let repeated = audeniq_core::external_recordings::import(
        &pool,
        "ci-operator",
        external_metadata(),
        &original,
    )
    .await
    .unwrap();
    assert_eq!(repeated, id, "repeat import preserves immutable evidence");
    let mut incorrect = external_metadata();
    incorrect.artist = "Different attribution".into();
    assert!(matches!(
        audeniq_core::external_recordings::import(&pool, "ci-operator", incorrect, &original).await,
        Err(Error::InvalidCode("REFERENCE_ATTRIBUTION_CONFLICT"))
    ));
    let tracks = [json!({"id":Uuid::new_v4(),"asset_id":Uuid::new_v4(),"isrc":"GBABC2600123"})];
    let scan = audeniq_core::external_recordings::scan(&pool, Uuid::new_v4(), &tracks)
        .await
        .unwrap();
    assert_eq!(scan.status, "REVIEW_REQUIRED");
    let detail: Value = serde_json::from_str(&scan.detail).unwrap();
    assert_eq!(detail["matches"][0]["signal"], "ISRC_CLAIM");
    assert_eq!(
        detail["missing_fingerprint_assets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let changed = sqlx::query(
        "UPDATE catalog.external_recordings SET artist='Reassigned artist' WHERE id=$1",
    )
    .bind(id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert_eq!(
        changed.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    audeniq_core::external_recordings::set_active(&pool, "ci-operator", id, false)
        .await
        .unwrap();
    let after = audeniq_core::external_recordings::scan(&pool, Uuid::new_v4(), &tracks)
        .await
        .unwrap();
    assert_eq!(after.status, "NOT_APPLICABLE");
    assert!(after.epoch > scan.epoch);
    let report: Value = serde_json::from_str(&after.detail).unwrap();
    assert_eq!(report["inspection_status"], "NOT_CHECKED");
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn populated_external_catalog_without_submitted_fingerprint_requires_review(pool: PgPool) {
    audeniq_core::database::MIGRATOR.run(&pool).await.unwrap();
    let dir = tmpdir();
    let original = varied_reference_wav(&dir);
    audeniq_core::external_recordings::import(&pool, "ci-operator", external_metadata(), &original)
        .await
        .unwrap();
    let scan = audeniq_core::external_recordings::scan(
        &pool,
        Uuid::new_v4(),
        &[json!({"asset_id":Uuid::new_v4()})],
    )
    .await
    .unwrap();
    assert_eq!(scan.status, "REVIEW_REQUIRED");
    let detail: Value = serde_json::from_str(&scan.detail).unwrap();
    assert_eq!(detail["inspection_status"], "INCOMPLETE");
    assert!(detail["matches"].as_array().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn unrelated_audio_passes_only_the_imported_external_reference_scope(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let reference = varied_reference_wav(&dir);
    audeniq_core::external_recordings::import(
        &pool,
        "ci-operator",
        external_metadata(),
        &reference,
    )
    .await
    .unwrap();
    let asset = register_asset(&pool, &store, &u, "unrelated.wav", &make_good_wav(&dir)).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision = consent_and_submit(&app, &u, release, "external-reference-unrelated").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(
        check_status(
            &pool,
            revision,
            audeniq_core::external_recordings::CHECK_CODE
        )
        .await,
        "PASS"
    );
    let detail: String = sqlx::query_scalar(
        "SELECT detail FROM operations.check_results WHERE revision_id=$1 AND check_code=$2",
    )
    .bind(revision)
    .bind(audeniq_core::external_recordings::CHECK_CODE)
    .fetch_one(&pool)
    .await
    .unwrap();
    let report: Value = serde_json::from_str(&detail).unwrap();
    assert_eq!(report["inspection_status"], "COMPLETED");
    assert_eq!(report["global_catalog_checked"], false);
    assert!(report["matches"].as_array().unwrap().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

fn qr_png(dir: &std::path::Path) -> Vec<u8> {
    let small = dir.join("qr.png");
    assert!(
        std::process::Command::new("qrencode")
            .args(["-s", "40", "-o"])
            .arg(&small)
            .arg("https://example.invalid/private-payload")
            .status()
            .unwrap()
            .success()
    );
    let cover = dir.join("qr-cover.png");
    assert!(
        std::process::Command::new("ffmpeg")
            .args(["-y", "-v", "error", "-i"])
            .arg(&small)
            .args([
                "-vf",
                "pad=3000:3000:(ow-iw)/2:(oh-ih)/2:color=white",
                "-frames:v",
                "1",
                "-pix_fmt",
                "rgb24"
            ])
            .arg(&cover)
            .status()
            .unwrap()
            .success()
    );
    std::fs::read(cover).unwrap()
}

#[test]
fn free_artwork_readers_detect_text_and_qr_without_storing_qr_payloads() {
    let dir = tmpdir();
    let text = dir.join("text.png");
    assert!(std::process::Command::new("ffmpeg").args(["-y","-v","error","-f","lavfi","-i","color=c=white:s=3000x3000",
        "-vf","drawtext=fontfile=/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf:text='www.example.com':fontsize=130:fontcolor=black:x=150:y=600",
        "-frames:v","1","-pix_fmt","rgb24"]).arg(&text).status().unwrap().success());
    let measurements = audeniq_core::artwork_policy::inspect(&text);
    assert!(
        measurements
            .iter()
            .all(|m| m.status == audeniq_core::qc::CheckStatus::Pass),
        "{measurements:?}"
    );
    let ocr: Value = serde_json::from_str(
        &measurements
            .iter()
            .find(|m| m.check_code == "IMAGE_TEXT_SCAN")
            .unwrap()
            .detail,
    )
    .unwrap();
    assert!(
        ocr["report"]["lines"]
            .as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().contains("www.example.com")),
        "{ocr}"
    );
    let qr_bytes = qr_png(&dir);
    let qr_path = dir.join("qr-cover.png");
    let results = audeniq_core::artwork_policy::inspect(&qr_path);
    let qr = results
        .iter()
        .find(|m| m.check_code == "IMAGE_QR_SCAN")
        .unwrap();
    assert_eq!(qr.status, audeniq_core::qc::CheckStatus::Pass, "{qr:?}");
    let report: Value = serde_json::from_str(&qr.detail).unwrap();
    assert_eq!(report["report"]["qr_count"], 1, "{report}");
    assert!(!qr.detail.contains("private-payload"));
    assert!(!qr_bytes.is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

async fn requested_release(
    pool: &PgPool,
    platform: &str,
    lyrics: Option<&str>,
) -> (Router, Arc<MemStore>, User, PathBuf, Uuid) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let asset = register_asset(pool, &store, &u, "policy.wav", &make_good_wav(&dir)).await;
    let release = build_submittable(&app, pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.releases SET draft=draft || $2::jsonb,row_version=row_version+1 WHERE id=$1")
        .bind(release)
        .bind(json!({"platforms":[platform]}))
        .execute(pool)
        .await
        .unwrap();
    if let Some(lyrics) = lyrics {
        sqlx::query("UPDATE catalog.tracks SET lyrics=$2 WHERE release_id=$1")
            .bind(release)
            .bind(lyrics)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO catalog.credits(org_id,track_id,party_id,role) SELECT org_id,id,$2,'LYRICIST' FROM catalog.tracks WHERE release_id=$1")
            .bind(release).bind(u.party).execute(pool).await.unwrap();
    }
    (app, store, u, dir, release)
}

async fn check_status(pool: &PgPool, revision: Uuid, code: &str) -> String {
    sqlx::query_scalar("SELECT status FROM operations.check_results WHERE revision_id=$1 AND check_code=$2 ORDER BY created_at DESC LIMIT 1")
        .bind(revision).bind(code).fetch_one(pool).await.unwrap()
}

#[sqlx::test]
async fn apple_lyrics_format_is_a_server_correction_before_verification(pool: PgPool) {
    let (app, store, u, dir, release) =
        requested_release(&pool, "apple", Some("[Chorus]\nHello again\n(Repeat x3)")).await;
    let revision = consent_and_submit(&app, &u, release, "apple-policy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_CORRECTION");
    assert_eq!(
        check_status(&pool, revision, "S2_DSP_LYRICS_FORMAT").await,
        "CORRECTION_REQUIRED"
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn explicit_lyrics_signal_requests_review_without_changing_the_tag(pool: PgPool) {
    let (app, store, u, dir, release) =
        requested_release(&pool, "spotify", Some("Fuck this\nHello again")).await;
    let revision = consent_and_submit(&app, &u, release, "explicit-policy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_REVIEW");
    assert_eq!(
        check_status(&pool, revision, "S2_DSP_EXPLICIT_TAG_REVIEW").await,
        "REVIEW_REQUIRED"
    );
    let explicit: bool =
        sqlx::query_scalar("SELECT parental_advisory FROM catalog.tracks WHERE release_id=$1")
            .bind(release)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!explicit);
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn content_id_exclusive_rights_acknowledgments_are_required_by_the_server(pool: PgPool) {
    let (app, store, u, dir, release) = requested_release(&pool, "youtube-cid", None).await;
    let revision = consent_and_submit(&app, &u, release, "cid-policy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_CORRECTION");
    assert_eq!(
        check_status(&pool, revision, "S2_DSP_CONTENT_ID_DECLARATION").await,
        "CORRECTION_REQUIRED"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn qr_cover_is_a_correction_and_the_payload_is_not_retained(pool: PgPool) {
    let (app, store, u, dir, release) = requested_release(&pool, "spotify", None).await;
    let bytes = qr_png(&dir);
    let key:String=sqlx::query_scalar("SELECT a.object_key FROM catalog.releases r JOIN catalog.assets a ON a.id=r.artwork_asset_id WHERE r.id=$1").bind(release).fetch_one(&pool).await.unwrap();
    store
        .files
        .lock()
        .await
        .insert(key.clone(), (bytes.clone(), "image/png".into()));
    sqlx::query("UPDATE catalog.assets SET sha256=$2,size_bytes=$3 WHERE object_key=$1")
        .bind(key)
        .bind(sha256_hex(&bytes))
        .bind(bytes.len() as i64)
        .execute(&pool)
        .await
        .unwrap();
    let revision = consent_and_submit(&app, &u, release, "qr-policy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_CORRECTION");
    assert_eq!(
        check_status(&pool, revision, "S2_DSP_ARTWORK_QR").await,
        "CORRECTION_REQUIRED"
    );
    let detail:String=sqlx::query_scalar("SELECT detail FROM operations.check_results WHERE revision_id=$1 AND check_code='IMAGE_QR_SCAN'").bind(revision).fetch_one(&pool).await.unwrap();
    assert!(!detail.contains("private-payload"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn unreferenced_artwork_results_do_not_replace_pinned_measurements(pool: PgPool) {
    let (app, store, u, dir, release) = requested_release(&pool, "spotify", None).await;
    let revision = consent_and_submit(&app, &u, release, "pinned-art-policy").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    let misleading =
        json!({"rule_version":"1","report":{"inspection_status":"COMPLETED","qr_count":9}})
            .to_string();
    sqlx::query("INSERT INTO operations.check_results(id,revision_id,check_code,rule_version,status,result_hash,detail) VALUES($1,$2,'IMAGE_QR_SCAN',$3,'PASS',$4,$5)")
        .bind(Uuid::new_v4()).bind(revision).bind(audeniq_core::qc::QC_RULE_VERSION).bind("f".repeat(64)).bind(misleading).execute(&pool).await.unwrap();
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_PASSED");
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM operations.check_results WHERE revision_id=$1 AND check_code='S2_DSP_ARTWORK_QR'").bind(revision).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn lyrics_are_frozen_and_corrections_precede_manual_review(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let asset = register_asset(&pool, &store, &u, "lyrics.wav", &make_good_wav(&dir)).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.tracks SET lyrics='Lyrics generated by AI' WHERE release_id=$1")
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();
    let revision = consent_and_submit(&app, &u, release, "lyrics-review").await;
    let body: Value =
        sqlx::query_scalar("SELECT body FROM catalog.application_revisions WHERE id=$1")
            .bind(revision)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(body["tracks"][0]["lyrics"], "Lyrics generated by AI");
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_CORRECTION");
    let checks: Vec<(String,String)>=sqlx::query_as("SELECT check_code,status FROM operations.check_results WHERE revision_id=$1 AND check_code IN ('S2_LYRICS_CREDITS','S2_AI_LYRICS_PROVENANCE') ORDER BY check_code")
        .bind(revision).fetch_all(&pool).await.unwrap();
    assert_eq!(
        checks,
        vec![
            ("S2_AI_LYRICS_PROVENANCE".into(), "REVIEW_REQUIRED".into()),
            ("S2_LYRICS_CREDITS".into(), "CORRECTION_REQUIRED".into())
        ]
    );
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn cover_generator_metadata_is_review_only_and_reaches_stage2(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let asset = register_asset(
        &pool,
        &store,
        &u,
        "cover-metadata.wav",
        &make_good_wav(&dir),
    )
    .await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let cover_path = dir.join("cover.png");
    std::fs::write(&cover_path, cover_png()).unwrap();
    let status = std::process::Command::new("exiftool")
        .args([
            "-overwrite_original",
            "-XMP-xmp:CreatorTool=Stable Diffusion",
        ])
        .arg(&cover_path)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let (cover,key):(Uuid,String)=sqlx::query_as("SELECT a.id,a.object_key FROM catalog.releases r JOIN catalog.assets a ON a.id=r.artwork_asset_id WHERE r.id=$1")
        .bind(release).fetch_one(&pool).await.unwrap();
    let bytes = std::fs::read(cover_path).unwrap();
    sqlx::query("UPDATE catalog.assets SET sha256=$2,size_bytes=$3 WHERE id=$1")
        .bind(cover)
        .bind(sha256_hex(&bytes))
        .bind(bytes.len() as i64)
        .execute(&pool)
        .await
        .unwrap();
    store
        .files
        .lock()
        .await
        .insert(key, (bytes, "image/png".into()));
    let revision = consent_and_submit(&app, &u, release, "cover-ai-signal").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_REVIEW");
    let details:Vec<(String,String)>=sqlx::query_as("SELECT status,detail FROM operations.check_results WHERE revision_id=$1 AND check_code='IMAGE_AI_PROVENANCE'")
        .bind(revision).fetch_all(&pool).await.unwrap();
    assert_eq!(
        details.len(),
        2,
        "Stage 1 evidence and Stage 2 hold are both recorded"
    );
    assert!(details.iter().all(|(s, d)| s == "REVIEW_REQUIRED"
        && d.contains("AI_METADATA_SIGNAL")
        && d.contains("NOT_CHECKED")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn reviewed_cover_drift_is_blocked_without_pinning_verification(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let asset = register_asset(&pool, &store, &u, "cover-drift.wav", &make_good_wav(&dir)).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    let revision = consent_and_submit(&app, &u, release, "cover-drift").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    sqlx::query("UPDATE catalog.assets SET sha256=$2 WHERE id=(SELECT artwork_asset_id FROM catalog.releases WHERE id=$1)")
        .bind(release).bind("a".repeat(64)).execute(&pool).await.unwrap();
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(release_status(&pool, release).await, "STAGE2_REVIEW");
    let status:String=sqlx::query_scalar("SELECT status FROM operations.check_results WHERE revision_id=$1 AND check_code='S2_ARTWORK_INTEGRITY'")
        .bind(revision).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "BLOCKED");
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM distribution.verification_packages WHERE revision_id=$1",
    )
    .bind(revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unsupported_exif_audio_uses_free_ffprobe_tags() {
    let dir = tmpdir();
    let path = dir.join("source.tta");
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=997:duration=2:sample_rate=48000",
                "-c:a",
                "tta",
                "-metadata",
                "comment=generated by Suno"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let report = audeniq_core::provenance::inspect_audio(&path);
    assert_eq!(report["inspection_status"], "COMPLETED", "{report}");
    assert_eq!(report["reader"], "FFPROBE_FALLBACK", "{report}");
    assert_eq!(report["outcome"], "AI_METADATA_SIGNAL", "{report}");
    assert_eq!(report["synthid"], "NOT_CHECKED");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn cover_generation_settings_and_workflows_are_extracted_from_png_chunks() {
    let dir = tmpdir();
    for (key, value) in [
        ("parameters", "A landscape\nSteps: 20, Sampler: Euler, CFG scale: 7, Seed: 123".to_string()),
        ("prompt", json!({"1":{"class_type":"KSampler","inputs":{}},"2":{"class_type":"CheckpointLoaderSimple","inputs":{}}}).to_string()),
    ] {
        let mut payload = key.as_bytes().to_vec();
        payload.push(0);
        payload.extend_from_slice(value.as_bytes());
        let mut chunk = b"tEXt".to_vec();
        chunk.extend_from_slice(&payload);
        let mut crc = u32::MAX;
        for byte in &chunk {
            crc ^= u32::from(*byte);
            for _ in 0..8 { crc = (crc >> 1) ^ (0xedb88320 & 0_u32.wrapping_sub(crc & 1)); }
        }
        let png = cover_png();
        let iend = png.len() - 12;
        let mut bytes = png[..iend].to_vec();
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&chunk);
        bytes.extend_from_slice(&(!crc).to_be_bytes());
        bytes.extend_from_slice(&png[iend..]);
        let path = dir.join(format!("{key}.png"));
        std::fs::write(&path, bytes).unwrap();
        let report = audeniq_core::provenance::inspect(&path);
        assert_eq!(report["inspection_status"], "COMPLETED", "{report}");
        assert_eq!(report["outcome"], "AI_METADATA_SIGNAL", "{report}");
        assert_eq!(report["synthid"], "NOT_CHECKED");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn original_audio_provenance_is_preserved_and_not_shared_between_identical_masters(
    pool: PgPool,
) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let dir = tmpdir();
    let bytes = make_good_wav(&dir);
    let hash = sha256_hex(&bytes);
    let storage: Arc<dyn audeniq_core::storage::ObjectStore> = store.clone();
    for (name, ai_source) in [
        ("plain.wav", false),
        ("generated.wav", true),
        ("plain-again.wav", false),
    ] {
        let asset = register_asset(&pool, &store, &u, name, &bytes).await;
        if ai_source {
            let report = audeniq_core::provenance::from_metadata(&json!({"Software":"Suno"}));
            sqlx::query("INSERT INTO catalog.asset_provenance(asset_id,org_id,source_sha256,master_sha256,rule_version,body) VALUES($1,$2,$3,$4,$5,$6)")
                .bind(asset).bind(u.org).bind("e".repeat(64)).bind(&hash)
                .bind(audeniq_core::provenance::RULE_VERSION).bind(report)
                .execute(&pool).await.unwrap();
        }
        assert!(
            !audeniq_core::submission::precheck_asset(&pool, &storage, u.org, asset)
                .await
                .unwrap()
        );
        let release = build_submittable(&app, &pool, &store, &u, asset).await;
        let revision = consent_and_submit(&app, &u, release, name).await;
        assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
        let (status,detail):(String,String)=sqlx::query_as("SELECT status,detail FROM operations.check_results WHERE revision_id=$1 AND check_code='AUDIO_AI_PROVENANCE'")
            .bind(revision).fetch_one(&pool).await.unwrap();
        if ai_source {
            assert_eq!(status, "REVIEW_REQUIRED");
            assert!(detail.contains("AI_METADATA_SIGNAL") && detail.contains("suno"));
        } else {
            assert_eq!(status, "NOT_APPLICABLE");
            assert!(detail.contains("UNKNOWN"));
        }
        assert!(
            detail.starts_with("cache_hit:"),
            "upload analysis must be reused: {detail}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[sqlx::test]
async fn clean_delivery_is_auto_approved_but_staff_hold_and_signature_still_gate(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    sqlx::query("UPDATE distribution.dsp_contract_routes SET route='DIRECT' WHERE code='D-5'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE execution.partner_onboarding SET dpid_registered=true,endpoint_url='https://d5.example.test',credential_kind='api_key',credential_status='STORED',test_ern_validated_at=now(),test_ack_parsed_at=now(),contract_signed_at=now(),contract_ref='TEST' WHERE partner_id='D-5'")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE execution.adapter_profiles SET activation_kind='MOCK',delivery_enabled=true,ddex_recipient_dpid='PADPIDA2007040501G',capabilities=capabilities||'{\"send_or_publish\":true}'::jsonb WHERE partner_id='D-5'")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE identity.orgs SET ddex_sender_dpid='PADPIDA2007061301Q' WHERE id=$1")
        .bind(u.org)
        .execute(&pool)
        .await
        .unwrap();
    let dir = tmpdir();
    let asset = register_asset(&pool, &store, &u, "clean.wav", &make_good_wav(&dir)).await;
    let release = build_submittable(&app, &pool, &store, &u, asset).await;
    sqlx::query("UPDATE catalog.releases SET draft=draft||'{\"genre\":\"Pop\",\"platforms\":[\"spotify\"]}'::jsonb,row_version=row_version+1 WHERE id=$1")
        .bind(release).execute(&pool).await.unwrap();
    let revision = consent_and_submit(&app, &u, release, "automatic-delivery").await;
    assert_eq!(run_one(&pool, &store, "qc", "stage1").await, "SUCCEEDED");
    assert_eq!(
        run_one(&pool, &store, "rights", "stage2").await,
        "SUCCEEDED"
    );
    assert_eq!(
        run_one(&pool, &store, "distribution", "prepare_release").await,
        "SUCCEEDED"
    );
    let package:Uuid=sqlx::query_scalar("SELECT dp.id FROM distribution.distribution_packages dp JOIN distribution.canonical_releases cr ON cr.id=dp.canonical_release_id WHERE cr.revision_id=$1")
        .bind(revision).fetch_one(&pool).await.unwrap();
    let staged = audeniq_core::delivery_staging::stage_package(&pool, package)
        .await
        .unwrap();
    let staging: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM distribution.delivery_staging s WHERE package_id=$1",
    )
    .bind(package)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(staged.automatically_approved, 1, "{staging}");
    let (approval,by,rule,hash):(String,Option<Uuid>,Option<String>,String)=sqlx::query_as("SELECT approval,approval_by,approval_rule_version,ern_sha256 FROM distribution.delivery_staging WHERE package_id=$1")
        .bind(package).fetch_one(&pool).await.unwrap();
    assert_eq!(approval, "APPROVED");
    assert!(by.is_none());
    assert_eq!(rule.as_deref(), Some("2"));
    assert!(
        audeniq_core::execution::enqueue_delivery_jobs(&pool, package)
            .await
            .unwrap()
            .0
            .is_empty(),
        "unsigned agreements never send"
    );
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let restaged = audeniq_core::delivery_staging::stage_package(&pool, package)
        .await
        .unwrap();
    assert_eq!(
        restaged.automatically_approved, 0,
        "approval retry is idempotent"
    );
    let retry_hash: String = sqlx::query_scalar(
        "SELECT ern_sha256 FROM distribution.delivery_staging WHERE package_id=$1",
    )
    .bind(package)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(hash, retry_hash, "retries build identical ERN bytes");
    let error=sqlx::query("UPDATE distribution.delivery_staging SET checks='[{\"code\":\"DSP_LOUDNESS_ADVISORY\",\"class\":\"CONTENT\",\"severity\":\"WARNING\"}]' WHERE package_id=$1")
        .bind(package).execute(&pool).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    // Reproduce the real review-to-signature transition (the fixture helper
    // otherwise inserts an already-signed agreement without its update hook).
    set_agreement(&pool, u.org, release, false).await;
    // New external evidence makes the frozen comparison stale for a new
    // automatic approval. Staff decisions below still retain their authority.
    sqlx::query("UPDATE catalog.external_recording_epoch SET epoch=epoch+1 WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    let stale = audeniq_core::delivery_staging::stage_package(&pool, package)
        .await
        .unwrap();
    assert_eq!(stale.automatically_approved, 0);
    let stale_approval: String = sqlx::query_scalar(
        "SELECT approval FROM distribution.delivery_staging WHERE package_id=$1",
    )
    .bind(package)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_approval, "PENDING");
    set_agreement(&pool, u.org, release, true).await;
    assert_eq!(
        audeniq_core::execution::enqueue_delivery_jobs(&pool, package)
            .await
            .unwrap()
            .0
            .len(),
        1
    );
    sqlx::query(
        "INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'OPERATOR','test')",
    )
    .bind(u.user)
    .execute(&pool)
    .await
    .unwrap();
    let (status, value) = call(
        &app,
        "POST",
        &format!("/api/staff/deliveries/{package}/D-5/decision"),
        json!({"action":"HOLD","note":"confirm ownership"}),
        Some(&u),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    // Force an ERN difference relative to the previous row: HOLD still wins.
    sqlx::query("UPDATE distribution.delivery_staging SET ern_sha256=NULL WHERE package_id=$1")
        .bind(package)
        .execute(&pool)
        .await
        .unwrap();
    let restaged = audeniq_core::delivery_staging::stage_package(&pool, package)
        .await
        .unwrap();
    assert_eq!(restaged.automatically_approved, 0);
    let (approval,rule):(String,Option<String>)=sqlx::query_as("SELECT approval,approval_rule_version FROM distribution.delivery_staging WHERE package_id=$1")
        .bind(package).fetch_one(&pool).await.unwrap();
    assert_eq!(approval, "HELD");
    assert!(rule.is_none());
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE resource_id=$1 AND action='delivery.auto_approved'")
        .bind(release).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
    std::fs::remove_dir_all(dir).unwrap();
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
        release_id: None,
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
