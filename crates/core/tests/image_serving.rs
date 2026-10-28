//! Synthetic payloads exercise storage, HTTP negotiation and retained revisions;
//! these are not image encoder tests. Requires a disposable PostgreSQL database.
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database, uploads,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request},
    response::Response,
};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
mod support;
use support::{MemStore, ORIGIN, SECRET, User, user};

async fn setup(pool: PgPool) -> (AppState, Router, Arc<MemStore>, User) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(MemStore::default());
    let state = AppState::new(
        pool,
        Config {
            database_url: "unused".into(),
            origin: ORIGIN.into(),
            service_secret: SECRET.into(),
            secure_cookie: false,
            bind: "127.0.0.1:0".into(),
            session_seconds: 3600,
            test_only_bypass_dsp_gate: true,
        },
        store.clone(),
    )
    .await
    .unwrap();
    let app = router(state.clone());
    let u = user(&app).await;
    (state, app, store, u)
}

async fn asset(state: &AppState, store: &MemStore, org: Uuid, bytes: &[u8]) -> Uuid {
    let id = Uuid::new_v4();
    let key = format!("registered/{org}/{id}");
    let sha = audeniq_core::domain::sha256_hex(bytes);
    store
        .files
        .lock()
        .await
        .insert(key.clone(), (bytes.to_vec(), "image/png".into()));
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(org)
        .bind(id)
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,sha256,state) VALUES($1,$2,'IMAGE',$3,$4,'image/png',$5,'REGISTERED')")
        .bind(id).bind(org).bind(&key).bind(bytes.len() as i64).bind(&sha).execute(&state.pool).await.unwrap();
    sqlx::query("INSERT INTO catalog.asset_safety(asset_id,org_id,source_key,source_sha256,safe_key,safe_sha256,rule_version) VALUES($1,$2,'quarantine/' || $1::text,$4,$3,$4,'1')")
        .bind(id).bind(org).bind(key).bind(sha).execute(&state.pool).await.unwrap();
    id
}

async fn get(app: &Router, path: &str, accept: &str, etag: Option<&str>) -> Response {
    let mut request = Request::builder()
        .uri(path)
        .header("x-audeniq-service", SECRET)
        .header("accept", accept);
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[sqlx::test]
async fn edits_change_revision_urls_and_old_versions_remain_scoped(pool: PgPool) {
    let (state, app, store, u) = setup(pool).await;
    let org = u.org;
    let artist = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'artist')")
        .bind(org)
        .bind(artist)
        .execute(&state.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.artists(org_id,id,name) VALUES($1,$2,'Artist')")
        .bind(org)
        .bind(artist)
        .execute(&state.pool)
        .await
        .unwrap();
    let user_id: Uuid =
        sqlx::query_scalar("SELECT user_id FROM identity.memberships WHERE org_id=$1 LIMIT 1")
            .bind(org)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO portal.artist_links(id,org_id,artist_id,slug,created_by) VALUES($1,$2,$3,'image_artist',$4)")
        .bind(Uuid::new_v4()).bind(org).bind(artist).bind(user_id).execute(&state.pool).await.unwrap();
    let old = asset(&state, &store, org, b"old PNG").await;
    let new = asset(&state, &store, org, b"new PNG").await;
    sqlx::query("INSERT INTO portal.artist_photos(org_id,artist_id,asset_id,updated_by) VALUES($1,$2,$3,$4)")
        .bind(org).bind(artist).bind(old).bind(user_id).execute(&state.pool).await.unwrap();
    let private = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/orgs/{org}/artists/{artist}/photo"))
                .header("x-audeniq-service", SECRET)
                .header("cookie", &u.cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(private.status(), 200);
    assert_eq!(private.headers()["cache-control"], "private, no-cache");
    let alias = "/api/artist-links/image_artist/photo?media=2";
    let first = get(&app, alias, "image/png", None).await;
    assert_eq!(first.status(), 307);
    assert_eq!(first.headers()["cache-control"], "no-store");
    let old_url = first.headers()["location"].to_str().unwrap().to_owned();
    sqlx::query("UPDATE portal.artist_photos SET asset_id=$3 WHERE org_id=$1 AND artist_id=$2")
        .bind(org)
        .bind(artist)
        .bind(new)
        .execute(&state.pool)
        .await
        .unwrap();
    let changed = get(&app, alias, "image/png", None).await;
    let new_url = changed.headers()["location"].to_str().unwrap().to_owned();
    assert_ne!(old_url, new_url);
    for (url, bytes) in [(&old_url, b"old PNG"), (&new_url, b"new PNG")] {
        let response = get(&app, url, "image/png", None).await;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers()["cache-control"],
            "private, max-age=31536000, immutable"
        );
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
            bytes
        );
    }
    let arbitrary = format!(
        "/api/artist-links/image_artist/photo?revision={}",
        "0".repeat(64)
    );
    assert_eq!(get(&app, &arbitrary, "image/png", None).await.status(), 404);
    sqlx::query("UPDATE portal.artist_links SET enabled=false WHERE artist_id=$1")
        .bind(artist)
        .execute(&state.pool)
        .await
        .unwrap();
    assert_eq!(get(&app, &old_url, "image/png", None).await.status(), 404);
    assert_eq!(get(&app, alias, "image/png", None).await.status(), 404);
}

#[sqlx::test]
async fn stored_derivatives_negotiate_without_cross_variant_revalidation(pool: PgPool) {
    let (state, _, store, u) = setup(pool).await;
    let org = u.org;
    let id = asset(&state, &store, org, b"source PNG").await;
    let source_sha = audeniq_core::domain::sha256_hex(b"source PNG");
    for (mime, bytes) in [
        ("image/avif", b"\0\0\0\x18ftypaviffixture".as_slice()),
        ("image/webp", b"RIFF1234WEBPfixture".as_slice()),
    ] {
        let key = format!("variants/{id}/{}", mime.replace('/', "-"));
        store
            .files
            .lock()
            .await
            .insert(key.clone(), (bytes.to_vec(), mime.into()));
        sqlx::query("INSERT INTO catalog.image_variants(org_id,asset_id,source_sha256,content_type,object_key,sha256,size_bytes) VALUES($1,$2,$3,$4,$5,$6,$7)")
            .bind(org).bind(id).bind(&source_sha).bind(mime).bind(key).bind(audeniq_core::domain::sha256_hex(bytes)).bind(bytes.len() as i64)
            .execute(&state.pool).await.unwrap();
    }
    let mut last_tag = String::new();
    for (accept, mime) in [
        ("image/avif,image/webp", "image/avif"),
        ("image/webp", "image/webp"),
        ("image/png", "image/png"),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("accept", accept.parse().unwrap());
        if !last_tag.is_empty() {
            headers.insert("if-none-match", last_tag.parse().unwrap());
        }
        let original = uploads::stored_image(&state, org, id).await.unwrap();
        let selected = audeniq_core::image_variants::negotiate(&state, original, &headers).await;
        let response = uploads::immutable_image_response(selected, &headers);
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], mime);
        assert_eq!(response.headers()["vary"], "Accept");
        last_tag = response.headers()["etag"].to_str().unwrap().to_owned();
    }
    assert_eq!(
        uploads::stored_image(&state, org, id).await.unwrap().bytes,
        b"source PNG"
    );
    // A wrong source revision must not borrow another image's derivative.
    let other = asset(&state, &store, org, b"another source").await;
    let bytes = b"RIFF1234WEBPfixture";
    let key = format!("variants/{other}/mismatched");
    store
        .files
        .lock()
        .await
        .insert(key.clone(), (bytes.to_vec(), "image/webp".into()));
    sqlx::query("INSERT INTO catalog.image_variants(org_id,asset_id,source_sha256,content_type,object_key,sha256,size_bytes) VALUES($1,$2,$3,'image/webp',$4,$5,$6)")
        .bind(org).bind(other).bind(&source_sha).bind(key).bind(audeniq_core::domain::sha256_hex(bytes)).bind(bytes.len() as i64)
        .execute(&state.pool).await.unwrap();
    let mut headers = HeaderMap::new();
    headers.insert("accept", "image/avif,image/webp".parse().unwrap());
    let original = uploads::stored_image(&state, org, other).await.unwrap();
    assert_eq!(
        audeniq_core::image_variants::negotiate(&state, original, &headers)
            .await
            .content_type,
        "image/png"
    );
}
