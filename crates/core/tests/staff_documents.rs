//! Staff can open only the inspected derivative of a submitted document
//! (`GET /api/staff/documents/{id}/file`): reviewers get the bytes with a
//! safe type and file name, the view is audited, and people without the
//! documents or review duty are refused.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

mod support;
use support::*;

async fn staff_user(app: &Router, pool: &PgPool, role: &str) -> User {
    let s = user(app).await;
    sqlx::query(
        "INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,$2,'test-operator')",
    )
    .bind(s.user)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    s
}

/// A document with an uploaded file, stored under `key` in the memory store.
async fn document_with_file(
    pool: &PgPool,
    store: &MemStore,
    org: Uuid,
    content_type: &str,
    file_name: &str,
    bytes: &[u8],
) -> Uuid {
    let asset = Uuid::new_v4();
    let key = format!("registered/{org}/{asset}/file");
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(org)
        .bind(asset)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,state)
         VALUES($1,$2,'DOCUMENT',$3,$4,$5,'REGISTERED')",
    )
    .bind(asset)
    .bind(org)
    .bind(&key)
    .bind(bytes.len() as i64)
    .bind(content_type)
    .execute(pool)
    .await
    .unwrap();
    if matches!(content_type, "application/pdf" | "image/jpeg" | "image/png") {
        use sha2::Digest;
        let sha = hex::encode(sha2::Sha256::digest(bytes));
        sqlx::query("UPDATE catalog.assets SET sha256=$2 WHERE id=$1")
            .bind(asset)
            .bind(&sha)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO catalog.asset_safety(asset_id,org_id,source_key,source_sha256,safe_key,safe_sha256,rule_version) VALUES($1,$2,$3,$4,$5,$4,'1')")
            .bind(asset).bind(org).bind(format!("quarantine/{org}/{asset}/source")).bind(&sha).bind(&key).execute(pool).await.unwrap();
    }
    let doc = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.documents(id,org_id,kind,title,status,asset_id,file_name)
         VALUES($1,$2,'RIGHTS_PROOF','샘플 사용 허락서','REVIEW',$3,$4)",
    )
    .bind(doc)
    .bind(org)
    .bind(asset)
    .bind(file_name)
    .execute(pool)
    .await
    .unwrap();
    store
        .files
        .lock()
        .await
        .insert(key, (bytes.to_vec(), content_type.to_string()));
    doc
}

async fn get_file(
    app: &Router,
    doc: Uuid,
    u: &User,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/staff/documents/{doc}/file"))
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("cookie", &u.cookie)
        .body(Body::empty())
        .unwrap();
    let r = app.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let bytes = axum::body::to_bytes(r.into_body(), 32 * 1024 * 1024)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

#[sqlx::test]
async fn reviewer_opens_the_submitted_file_and_the_view_is_audited(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let artist = user(&app).await;
    let pdf = b"%PDF-1.4\n% sample licence\n".to_vec();
    let doc = document_with_file(
        &pool,
        &store,
        artist.org,
        "application/pdf",
        "샘플 허락서.pdf",
        &pdf,
    )
    .await;

    let reviewer = staff_user(&app, &pool, "REVIEWER").await;
    let (s, h, body) = get_file(&app, doc, &reviewer).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, pdf);
    assert_eq!(h["content-type"], "application/pdf");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(h["cache-control"].to_str().unwrap().contains("no-store"));
    let disp = h["content-disposition"].to_str().unwrap();
    assert!(disp.starts_with("inline; "), "{disp}");
    assert!(disp.contains("UTF-8''%EC%83%98%ED%94%8C"), "{disp}");

    let audited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events WHERE action='staff.document_viewed' AND resource_id=$1 AND actor_user_id=$2",
    )
    .bind(doc)
    .bind(reviewer.user)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audited, 1);

    // An artist (not staff) and support staff (no documents/review duty) cannot.
    let (s, _, _) = get_file(&app, doc, &artist).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let support_staff = staff_user(&app, &pool, "SUPPORT").await;
    let (s, _, _) = get_file(&app, doc, &support_staff).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // Unknown document.
    let (s, _, _) = get_file(&app, Uuid::new_v4(), &reviewer).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn unexpected_or_uninspected_files_are_never_served(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let artist = user(&app).await;
    let doc = document_with_file(
        &pool,
        &store,
        artist.org,
        "text/html",
        "x.html",
        b"<script>alert(1)</script>",
    )
    .await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let (s, _, _) = get_file(&app, doc, &admin).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
