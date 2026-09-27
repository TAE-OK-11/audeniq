//! Presigned uploads against a real SigV4-verifying S3 server.
//!
//! The user's device gets only the signed URL, never the R2 key. These tests
//! prove the URL grants exactly one upload: this key, content type, nonce and
//! byte count, and nothing after a tampered signature. Server-side reads
//! (HEAD, copy to the registered key, streaming download) use the server's
//! own credentials.
//!
//! Runs when `S3_TEST_ENDPOINT` is set (CI starts Versity S3 Gateway, which
//! verifies signatures like R2; moto does not). Skipped otherwise.
use audeniq_core::storage::{ObjectStore, S3Store, UploadGrant};
use chrono::{Duration, Utc};

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

/// Upload the way Studio does: every granted header except the ones the
/// browser fills in itself; the HTTP client sets Content-Length from the body.
async fn put(url: &str, grant: &UploadGrant, body: Vec<u8>, content_type: Option<&str>) -> u16 {
    let mut req = reqwest::Client::new().put(url);
    for (k, v) in &grant.headers {
        if k == "content-length" || k == "host" {
            continue;
        }
        let v = if k == "content-type" {
            content_type.unwrap_or(v)
        } else {
            v
        };
        req = req.header(k, v);
    }
    req.body(body).send().await.expect("PUT").status().as_u16()
}

#[tokio::test]
async fn presigned_put_accepts_only_the_declared_object() {
    let Some(s) = store() else {
        eprintln!("S3_TEST_ENDPOINT not set; skipped");
        return;
    };
    let id = uuid::Uuid::new_v4();
    let key = format!("quarantine/test/{id}");
    let body: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let nonce = id.to_string();
    let grant = s
        .presign_put(
            &key,
            body.len() as i64,
            "audio/wav",
            &nonce,
            Utc::now() + Duration::minutes(10),
        )
        .await
        .unwrap();
    assert_eq!(grant.method, "PUT");
    assert!(grant.headers.contains_key("content-length"));
    assert!(
        grant.url.contains("content-length%3Bcontent-type%3Bhost"),
        "size, type and host are signed: {}",
        grant.url
    );
    let secret = std::env::var("S3_TEST_SECRET_ACCESS_KEY").unwrap();
    assert!(
        !grant.url.contains(&secret) && !grant.headers.values().any(|v| v.contains(&secret)),
        "the secret key never reaches the client"
    );

    // A bigger (or smaller) body than declared is refused by the store.
    let mut bigger = body.clone();
    bigger.extend_from_slice(&[0; 1024]);
    assert_eq!(put(&grant.url, &grant, bigger, None).await, 403);
    assert_eq!(
        put(&grant.url, &grant, body[..100].to_vec(), None).await,
        403
    );
    // So is another content type, and a tampered signature.
    assert_eq!(
        put(&grant.url, &grant, body.clone(), Some("audio/flac")).await,
        403
    );
    let tampered = grant.url.replace("X-Amz-Signature=", "X-Amz-Signature=0");
    assert_eq!(put(&tampered, &grant, body.clone(), None).await, 403);
    // The URL is bound to its key: pointing it at another object fails.
    let other = grant
        .url
        .replace(&id.to_string(), &uuid::Uuid::new_v4().to_string());
    assert_eq!(put(&other, &grant, body.clone(), None).await, 403);
    assert!(s.head(&key).await.unwrap().is_none(), "nothing was stored");

    // The declared object goes through.
    assert_eq!(put(&grant.url, &grant, body.clone(), None).await, 200);
    // Single use: the same URL cannot upload again, not even identical bytes.
    assert!(grant.headers.contains_key("if-none-match"));
    assert_eq!(put(&grant.url, &grant, body.clone(), None).await, 412);
    assert_eq!(
        put(&grant.url, &grant, vec![9u8; body.len()], None).await,
        412
    );
    let meta = s.head(&key).await.unwrap().expect("stored");
    assert_eq!(meta.size, body.len() as i64);
    assert_eq!(meta.content_type, "audio/wav");
    assert_eq!(meta.nonce, nonce);
    // Upload completion sniffs audio with a ranged read, not a download.
    assert_eq!(s.read_prefix(&key, 16).await.unwrap(), body[..16].to_vec());

    // The server copies it to its registered key and reads it back with its
    // own credentials (upload completion / Stage 1).
    let stable = format!("registered/test/{id}");
    s.freeze(&key, &stable, &meta.etag).await.unwrap();
    assert_eq!(s.get(&stable).await.unwrap(), body);
    let digest = s.digest(&stable, body.len() as u64).await.unwrap();
    assert_eq!(digest.size, body.len() as u64);
    // A master the server derives (FLAC from ALAC) goes up with its own key.
    let local = std::env::temp_dir().join(format!("audeniq-s3-put-{id}"));
    std::fs::write(&local, &body).unwrap();
    let derived = format!("registered/test/{id}-flac");
    s.put_file(&derived, &local, "audio/flac", &nonce)
        .await
        .unwrap();
    let _ = std::fs::remove_file(&local);
    let put = s.head(&derived).await.unwrap().expect("derived stored");
    assert_eq!(
        (put.size, put.content_type.as_str(), put.nonce.as_str()),
        (body.len() as i64, "audio/flac", nonce.as_str())
    );
    assert_eq!(s.get(&derived).await.unwrap(), body);
    s.delete(&key).await.unwrap();
    assert!(s.head(&key).await.unwrap().is_none());
}

#[tokio::test]
async fn expired_grant_is_refused() {
    let Some(s) = store() else {
        eprintln!("S3_TEST_ENDPOINT not set; skipped");
        return;
    };
    let key = format!("quarantine/test/{}", uuid::Uuid::new_v4());
    let body = vec![7u8; 64];
    let grant = s
        .presign_put(
            &key,
            body.len() as i64,
            "image/png",
            "n",
            Utc::now() + Duration::seconds(3),
        )
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert_eq!(put(&grant.url, &grant, body, None).await, 403);
}
