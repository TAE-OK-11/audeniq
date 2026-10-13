//! Resumable (multipart) master uploads through the API against a real
//! SigV4-verifying S3 server: part URLs on demand, resume from the stored
//! parts, completion that refuses missing parts, cancellation.
//!
//! Runs when `S3_TEST_ENDPOINT` is set (CI's Versity S3 Gateway); skipped
//! otherwise.
mod support;

use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    storage::S3Store,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use support::*;

fn store() -> Option<S3Store> {
    let endpoint = std::env::var("S3_TEST_ENDPOINT").ok()?;
    Some(
        S3Store::new(
            &endpoint,
            std::env::var("S3_TEST_BUCKET").expect("S3_TEST_BUCKET"),
            std::env::var("S3_TEST_ACCESS_KEY_ID").expect("S3_TEST_ACCESS_KEY_ID"),
            std::env::var("S3_TEST_SECRET_ACCESS_KEY").expect("S3_TEST_SECRET_ACCESS_KEY"),
            std::env::var("S3_TEST_REGION").unwrap_or_else(|_| "us-east-1".into()),
            true,
        )
        .expect("test store"),
    )
}

/// PUT the way Studio does: the granted headers minus the ones the browser
/// sets itself.
async fn put(grant: &Value, body: &[u8]) -> u16 {
    let mut req = reqwest::Client::new().put(grant["url"].as_str().unwrap());
    for (k, v) in grant["headers"].as_object().unwrap() {
        if k == "content-length" || k == "host" {
            continue;
        }
        req = req.header(k, v.as_str().unwrap());
    }
    req.body(body.to_vec())
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

fn long_wav(dir: &std::path::Path) -> Vec<u8> {
    let out = dir.join("long.wav");
    let st = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=100",
        ])
        .args([
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

#[sqlx::test]
async fn large_master_uploads_in_parts_and_resumes(pool: PgPool) {
    let Some(s3) = store() else {
        eprintln!("S3_TEST_ENDPOINT not set; skipped");
        return;
    };
    database::MIGRATOR.run(&pool).await.unwrap();
    let state = AppState::new(
        pool.clone(),
        Config {
            database_url: "unused".into(),
            origin: ORIGIN.into(),
            service_secret: SECRET.into(),
            secure_cookie: false,
            bind: "127.0.0.1:0".into(),
            session_seconds: 3600,
            test_only_bypass_dsp_gate: true,
        },
        Arc::new(s3),
    )
    .await
    .unwrap();
    let app = router(state);
    let u = user(&app).await;
    let dir = std::env::temp_dir().join(format!("audeniq-resumable-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = long_wav(&dir);
    std::fs::remove_dir_all(&dir).ok();
    let part_size = audeniq_core::uploads::PART_SIZE as usize;
    assert!(
        wav.len() > part_size && wav.len() < 2 * part_size,
        "two parts"
    );
    let base = format!("/api/orgs/{}/uploads", u.org);

    let (s, issued) = call(
        &app,
        "POST",
        &base,
        json!({"kind":"AUDIO","size_bytes":wav.len(),"content_type":"audio/wav","resumable":true}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{issued}");
    assert!(issued.get("grant").is_none());
    assert_eq!(issued["multipart"]["part_count"], 2);
    let session = issued["upload_session_id"].as_str().unwrap().to_string();
    let parts_path = format!("{base}/{session}/parts");
    let complete = json!({"asset_id":issued["asset_id"],"expected_key":issued["expected_key"]});

    // Part 1, then the page reloads: the stored parts say where to continue.
    let (s, v) = call(
        &app,
        "POST",
        &parts_path,
        json!({"part_numbers":[1]}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["parts"][0]["size_bytes"], part_size);
    assert_eq!(put(&v["parts"][0]["grant"], &wav[..part_size]).await, 200);
    let (s, v) = call(&app, "GET", &parts_path, Value::Null, Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["parts"],
        json!([{"part_number":1,"size_bytes":part_size}])
    );
    for bad in [
        json!({"part_numbers":[3]}),
        json!({"part_numbers":[0]}),
        json!({"part_numbers":[]}),
    ] {
        let (_, v) = call(&app, "POST", &parts_path, bad, Some(&u)).await;
        assert_eq!(v["error"]["code"], "UPLOAD_PARTS_INVALID", "{v}");
    }
    // Completing with a part missing is refused and changes nothing.
    let (_, v) = call(
        &app,
        "POST",
        &format!("{base}/{session}/complete"),
        complete.clone(),
        Some(&u),
    )
    .await;
    assert_eq!(v["error"]["code"], "UPLOAD_PARTS_INCOMPLETE", "{v}");
    // Another account cannot ask for part URLs.
    let other = user(&app).await;
    let (s, _) = call(
        &app,
        "POST",
        &parts_path.replace(&u.org.to_string(), &other.org.to_string()),
        json!({"part_numbers":[2]}),
        Some(&other),
    )
    .await;
    assert_ne!(s, StatusCode::OK);

    let (_, v) = call(
        &app,
        "POST",
        &parts_path,
        json!({"part_numbers":[2]}),
        Some(&u),
    )
    .await;
    assert_eq!(v["parts"][0]["size_bytes"], wav.len() - part_size);
    assert_eq!(put(&v["parts"][0]["grant"], &wav[part_size..]).await, 200);
    let (s, v) = call(
        &app,
        "POST",
        &format!("{base}/{session}/complete"),
        complete.clone(),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "REGISTERED");
    let (s, v) = call(
        &app,
        "POST",
        &format!("{base}/{session}/complete"),
        complete,
        Some(&u),
    )
    .await;
    assert_eq!(
        (s, v["duplicate"].as_bool()),
        (StatusCode::OK, Some(true)),
        "{v}"
    );
    let sha: Option<String> =
        sqlx::query_scalar("SELECT sha256 FROM catalog.assets WHERE id=$1::uuid")
            .bind(issued["asset_id"].as_str().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(sha.is_some(), "registered master is hashed");
    // A finished session hands out no more part URLs.
    let (s, _) = call(
        &app,
        "POST",
        &parts_path,
        json!({"part_numbers":[1]}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Cancelling an unfinished resumable upload closes it.
    let (_, issued) = call(
        &app,
        "POST",
        &base,
        json!({"kind":"AUDIO","size_bytes":wav.len(),"content_type":"audio/wav","resumable":true}),
        Some(&u),
    )
    .await;
    let session = issued["upload_session_id"].as_str().unwrap();
    let (s, v) = call(
        &app,
        "POST",
        &format!("{base}/{session}/cancel"),
        json!({}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _) = call(
        &app,
        "GET",
        &format!("{base}/{session}/parts"),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Small files keep the single signed PUT even when resumable is asked.
    let (_, v) = call(
        &app,
        "POST",
        &base,
        json!({"kind":"AUDIO","size_bytes":1024,"content_type":"audio/wav","resumable":true}),
        Some(&u),
    )
    .await;
    assert!(
        v.get("grant").is_some() && v.get("multipart").is_none(),
        "{v}"
    );
}
