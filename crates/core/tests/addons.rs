//! Add-on API and workers against isolated real PostgreSQL, with existing
//! session/CSRF/resource ACL and production migrations.
use async_trait::async_trait;
use audeniq_core::{
    addons::{
        jobs::{self, AdapterOutcome, ProviderAdapter, WorkItem},
        model::Status,
        workflow,
    },
    error::Result,
    operations,
    storage::ObjectStore,
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;
mod support;
use support::*;

async fn admin_user(app: &Router, pool: &PgPool) -> User {
    let a = user(app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','addon-test')").bind(a.user).execute(pool).await.unwrap();
    a
}
fn profile(artist: Uuid, plus: bool) -> Value {
    json!({"service_code":if plus{"PROFILE_PLUS"}else{"PROFILE_BASIC"},"target_type":"artist","target_id":artist,"details":{"type":"artist_profile","request_type":"LINK","platforms":["spotify","youtube"],"notes":"Please connect"}})
}
fn release_order(code: &str, id: Uuid) -> Value {
    json!({"service_code":code,"target_type":"release","target_id":id,"details":{"type":if code=="PRIORITY_DELIVERY"{"priority"}else if code=="MUSIC_DATA_BASIC"{"music_data"}else{"promo"}}})
}
async fn create_call(app: &Router, u: &User, key: &str, body: Value) -> (StatusCode, Value) {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/orgs/{}/addons/orders", u.org))
                .header("x-audeniq-service", SECRET)
                .header("origin", ORIGIN)
                .header("cookie", &u.cookie)
                .header("x-csrf-token", &u.csrf)
                .header("content-type", "application/json")
                .header("idempotency-key", key)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = r.status();
    let b = axum::body::to_bytes(r.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&b)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&b).into_owned())),
    )
}
async fn create_ok(app: &Router, u: &User, body: Value) -> Value {
    let (s, v) = create_call(app, u, &Uuid::new_v4().to_string(), body).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}
fn id(v: &Value) -> Uuid {
    Uuid::parse_str(v["id"].as_str().unwrap()).unwrap()
}
async fn action(app: &Router, a: &User, v: &Value, path: &str, extra: Value) -> Value {
    let mut body =
        json!({"row_version":v["row_version"],"reason":"Verified manual operator action"});
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (s, result) = call(
        app,
        "POST",
        &format!("/api/admin/addons/orders/{}/{path}", id(v)),
        body,
        Some(a),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{path}: {result}");
    result
}
async fn pay_queue(app: &Router, a: &User, v: Value) -> Value {
    let paid = action(
        app,
        a,
        &v,
        "payment",
        json!({"payment_reference":Uuid::new_v4().to_string()}),
    )
    .await;
    assert_eq!(paid["status"], "PAID");
    assert_eq!(paid["payment_status"], "PAID");
    action(app, a, &paid, "status", json!({"status":"QUEUED"})).await
}
async fn start(app: &Router, a: &User, v: Value) -> Value {
    let approved = action(app, a, &v, "approve", json!({})).await;
    assert_eq!(approved["status"], "APPROVED");
    action(app, a, &approved, "status", json!({"status":"IN_PROGRESS"})).await
}
async fn get_order(app: &Router, u: &User, id: Uuid) -> Value {
    let (s, v) = call(
        app,
        "GET",
        &format!("/api/orgs/{}/addons/orders/{id}", u.org),
        Value::Null,
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}
async fn drain(pool: &PgPool, store: Arc<dyn ObjectStore>) {
    for _ in 0..60 {
        let mut did = false;
        for q in ["interactive", "delivery"] {
            if let Some(j) = operations::claim(pool, q, "addon-test", 300).await.unwrap() {
                operations::execute(pool, &store, &j).await.unwrap();
                did = true;
            }
        }
        if !did {
            return;
        }
    }
    panic!("queue did not drain")
}
/// Fixtures are complete immutable assets, not a production upload shortcut.
async fn asset(pool: &PgPool, u: &User, kind: &str) -> Uuid {
    let id = Uuid::new_v4();
    let key = format!("registered/{}/{id}/fixture", u.org);
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'asset')")
        .bind(u.org)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO identity.resource_acl(org_id,resource_id,principal_party_id,action) VALUES($1,$2,$3,'read'),($1,$2,$3,'write')").bind(u.org).bind(id).bind(u.party).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO catalog.assets(id,org_id,kind,object_key,size_bytes,content_type,state,sha256,etag,qc_status) VALUES($1,$2,$3,$4,128,$5,'REGISTERED',$6,'immutable-fixture','PASS')").bind(id).bind(u.org).bind(kind).bind(key).bind(match kind{"VIDEO"=>"video/mp4","LRC"=>"text/plain","DOCUMENT"=>"application/pdf","IMAGE"=>"image/png",_=>"audio/flac"}).bind("b".repeat(64)).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO catalog.upload_sessions(id,org_id,asset_id,expected_key,nonce,expected_bytes,content_type,expires_at,status,completed_at) VALUES($1,$2,$3,$4,$5,128,$6,now()+interval '1 hour','COMPLETED',now())").bind(Uuid::new_v4()).bind(u.org).bind(id).bind(format!("quarantine/{id}")).bind(Uuid::new_v4()).bind("fixture").execute(pool).await.unwrap();
    id
}
async fn track(app: &Router, pool: &PgPool, u: &User, with_assets: bool) -> Uuid {
    let r = create_release(app, u).await;
    let ar = create_artist(app, u).await;
    let audio = if with_assets {
        Some(asset(pool, u, "AUDIO").await)
    } else {
        None
    };
    if with_assets {
        let image = asset(pool, u, "IMAGE").await;
        sqlx::query(
            "UPDATE catalog.releases SET artwork_asset_id=$2,row_version=row_version+1 WHERE id=$1",
        )
        .bind(r)
        .bind(image)
        .execute(pool)
        .await
        .unwrap();
    }
    let(s,v)=call(app,"POST",&format!("/api/orgs/{}/releases/{r}/tracks",u.org),json!({"title":"Test track","disc_number":1,"track_number":1,"artist_id":ar,"asset_id":audio,"row_version":row_version(pool,r).await}),Some(u)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    id(&v)
}

#[sqlx::test]
async fn free_order_catalog_and_admin_acl(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, false)).await;
    assert_eq!(v["status"], "QUEUED");
    assert_eq!(v["payment_status"], "NOT_REQUIRED");
    assert_eq!(v["amount"], 0);
    let (s, c) = call(&app, "GET", "/api/addons/catalog", Value::Null, Some(&u)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(c["items"].as_array().unwrap().len(), 11);
    let (s, _) = call(
        &app,
        "GET",
        "/api/admin/addons/orders",
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}
#[sqlx::test]
async fn paid_order_requires_payment_and_cannot_be_approved(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, true)).await;
    assert_eq!(v["status"], "PAYMENT_REQUIRED");
    assert_eq!(v["payment_status"], "PENDING");
    assert_eq!(v["amount"], 19000);
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/admin/addons/orders/{}/approve", id(&v)),
        json!({"row_version":v["row_version"],"reason":"Cannot skip payment"}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let queued = pay_queue(&app, &a, v).await;
    let started = start(&app, &a, queued).await;
    assert_eq!(started["status"], "IN_PROGRESS");
    assert!(started["first_reviewed_at"].is_string());
    assert!(started["valid_until"].is_string());
    let calendar: bool = sqlx::query_scalar(
        "SELECT valid_until=paid_at+interval '12 months' FROM catalog.addon_orders WHERE id=$1",
    )
    .bind(id(&started))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(calendar, "PROFILE_PLUS validity must use calendar months");
}
#[sqlx::test]
async fn illegal_transition_is_rejected_by_api_and_database(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, false)).await;
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/admin/addons/orders/{}/status", id(&v)),
        json!({"row_version":v["row_version"],"reason":"Illegal jump","status":"COMPLETED"}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(sqlx::query("UPDATE catalog.addon_orders SET status='COMPLETED',row_version=row_version+1 WHERE id=$1").bind(id(&v)).execute(&pool).await.is_err());
    assert!(Status::QUEUED.transition(Status::COMPLETED).is_err());
}
#[sqlx::test]
async fn cross_org_and_resource_acl_are_enforced(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let other = user(&app).await;
    let ar = create_artist(&app, &other).await;
    let r = create_release(&app, &other).await;
    for b in [profile(ar, true), release_order("PRIORITY_DELIVERY", r)] {
        let (s, _) = create_call(&app, &u, "cross-org-denied", b).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
    }
    let own = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(own, false)).await;
    sqlx::query("UPDATE identity.resource_acl SET revoked_at=now() WHERE resource_id=$1")
        .bind(own)
        .execute(&pool)
        .await
        .unwrap();
    let (s, _) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/addons/orders/{}", u.org, id(&v)),
        Value::Null,
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, listed) = call(
        &app,
        "GET",
        &format!("/api/orgs/{}/addons/orders", u.org),
        Value::Null,
        Some(&u),
    )
    .await;
    assert!(listed["items"].as_array().unwrap().is_empty());
}
#[sqlx::test]
async fn catalog_version_change_preserves_order_snapshot(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let old = create_ok(&app, &u, profile(ar, true)).await;
    let(s,v)=call(&app,"PUT","/api/admin/addons/catalog/PROFILE_PLUS",json!({"expected_version":1,"price_krw":22000,"active":true,"display_name":"Profile Plus","description":"New price","validity_days":365,"max_revisions":null,"reason":"Annual price update"}),Some(&a)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let fetched = get_order(&app, &u, id(&old)).await;
    assert_eq!(fetched["amount"], 19000);
    assert_eq!(fetched["catalog_version"], 1);
    let ar2 = create_artist(&app, &u).await;
    let new = create_ok(&app, &u, profile(ar2, true)).await;
    assert_eq!(new["amount"], 22000);
    assert_eq!(new["catalog_version"], 2);
    assert!(sqlx::query("UPDATE catalog.addon_orders SET amount=1,price_snapshot_krw=1,row_version=row_version+1 WHERE id=$1").bind(id(&old)).execute(&pool).await.is_err());
    let mut client_price = profile(ar2, true);
    client_price["price_krw"] = json!(1);
    let (s, _) = create_call(&app, &u, "client-price-denied", client_price).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}
#[sqlx::test]
async fn idempotency_retry_concurrency_and_payload_conflict(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let ar = create_artist(&app, &u).await;
    let body = profile(ar, false);
    let (first, second) = tokio::join!(
        create_call(&app, &u, "retry-creation-key", body.clone()),
        create_call(&app, &u, "retry-creation-key", body.clone())
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);
    assert_eq!(id(&first.1), id(&second.1));
    let mut changed = body;
    changed["details"]["notes"] = json!("Different payload");
    let (s, _) = create_call(&app, &u, "retry-creation-key", changed).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.addon_orders")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}
#[sqlx::test]
async fn profile_plus_active_and_completed_valid_period_prevent_duplicates(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, true)).await;
    let (s, e) = create_call(&app, &u, "second-profile-key", profile(ar, true)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(e["error"]["code"], "ADDON_ACTIVE_ORDER_EXISTS");
    let queued = pay_queue(&app, &a, v).await;
    let started = start(&app, &a, queued).await;
    drain(&pool, store).await;
    let pending = get_order(&app, &u, id(&started)).await;
    let done = action(
        &app,
        &a,
        &pending,
        "results",
        json!({"external_reference":"manual-profile-dsp-case"}),
    )
    .await;
    let completed = action(&app, &a, &done, "complete", json!({})).await;
    assert_eq!(completed["status"], "COMPLETED");
    let (s, _) = create_call(&app, &u, "third-profile-key", profile(ar, true)).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let mut next = profile(ar, true)["details"].clone();
    next["request_type"] = json!("RETRY");
    let(s,reopened)=call(&app,"POST",&format!("/api/orgs/{}/addons/orders/{}/follow-ups",u.org,id(&completed)),json!({"row_version":completed["row_version"],"reason":"Follow up with DSP","details":next}),Some(&u)).await;
    assert_eq!(s, StatusCode::OK, "{reopened}");
    assert_eq!(reopened["status"], "UNDER_REVIEW");
    assert_eq!(reopened["payment_status"], "PAID");
    assert_eq!(reopened["amount"], 19000);
}
#[sqlx::test]
async fn mv_global_only_needs_verified_evidence(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let video = asset(&pool, &u, "VIDEO").await;
    let body = json!({"service_code":"MV_GLOBAL_ONLY","target_type":"music_video","target_id":video,"details":{"type":"mv"}});
    let (s, v) = create_call(&app, &u, "mv-missing-evidence", body.clone()).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], "MV_REVIEW_EVIDENCE_REQUIRED");
    let evidence = asset(&pool, &u, "DOCUMENT").await;
    sqlx::query("UPDATE catalog.assets SET state='UPLOADING' WHERE id=$1")
        .bind(evidence)
        .execute(&pool)
        .await
        .unwrap();
    let mut body = body;
    body["details"]["review_evidence_asset_id"] = json!(evidence);
    let (s, _) = create_call(&app, &u, "mv-quarantine-denied", body).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
}
#[sqlx::test]
async fn mv_evidence_approval_gates_distribution_and_completion(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let video = asset(&pool, &u, "VIDEO").await;
    let evidence = asset(&pool, &u, "DOCUMENT").await;
    let v=create_ok(&app,&u,json!({"service_code":"MV_GLOBAL_ONLY","target_type":"music_video","target_id":video,"details":{"type":"mv","review_evidence_asset_id":evidence}})).await;
    let queued = pay_queue(&app, &a, v).await;
    let under = action(
        &app,
        &a,
        &queued,
        "status",
        json!({"status":"UNDER_REVIEW"}),
    )
    .await;
    let (s, _) = call(
        &app,
        "POST",
        &format!("/api/admin/addons/orders/{}/approve", id(&under)),
        json!({"row_version":under["row_version"],"reason":"Evidence not decided"}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let reviewed = action(
        &app,
        &a,
        &under,
        "evidence",
        json!({"approved":true,"valid_until":"2028-12-31T00:00:00Z"}),
    )
    .await;
    let started = start(&app, &a, reviewed).await;
    drain(&pool, store).await;
    let pending = get_order(&app, &u, id(&started)).await;
    assert_eq!(pending["status"], "EXTERNAL_PENDING");
    assert_eq!(pending["provider_tasks"][0]["kind"], "addon.mv.distribute");
    let prepared = action(
        &app,
        &a,
        &pending,
        "results",
        json!({"external_reference":"video-dsp-case","mv_distribution_status":"PREPARED"}),
    )
    .await;
    let delivered = action(
        &app,
        &a,
        &prepared,
        "results",
        json!({"external_reference":"video-dsp-receipt","mv_distribution_status":"DELIVERED"}),
    )
    .await;
    let completed = action(&app, &a, &delivered, "complete", json!({})).await;
    assert_eq!(completed["status"], "COMPLETED");
}
#[sqlx::test]
async fn lyric_video_two_revisions_then_manual_review(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let t = track(&app, &pool, &u, true).await;
    let v=create_ok(&app,&u,json!({"service_code":"LYRIC_VIDEO_PLUS","target_type":"track","target_id":t,"details":{"type":"lyric_video","lyrics_text":"Test lyrics","template_id":"premium-01"}})).await;
    let queued = pay_queue(&app, &a, v).await;
    let started = start(&app, &a, queued).await;
    drain(&pool, store.clone()).await;
    let mut pending = get_order(&app, &u, id(&started)).await;
    assert_eq!(pending["provider_tasks"].as_array().unwrap().len(), 2);
    for count in 1..=3 {
        let (s, v) = call(
            &app,
            "POST",
            &format!(
                "/api/orgs/{}/addons/orders/{}/revisions",
                u.org,
                id(&pending)
            ),
            json!({"row_version":pending["row_version"],"reason":"Adjust subtitle layout"}),
            Some(&u),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        assert_eq!(v["revision_count"], count);
        assert_eq!(
            v["status"],
            if count <= 2 {
                "IN_PROGRESS"
            } else {
                "UNDER_REVIEW"
            }
        );
        drain(&pool, store.clone()).await;
        pending = get_order(&app, &u, id(&v)).await;
    }
    assert_eq!(pending["status"], "UNDER_REVIEW");
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.addon_provider_tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tasks, 6);
}
#[sqlx::test]
async fn priority_updates_queued_and_future_jobs_and_ages_normal_work(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let r = create_release(&app, &u).await;
    let mut c = pool.acquire().await.unwrap();
    let queued = operations::enqueue(
        &mut c,
        "qc",
        "test.qc",
        &json!({"release_id":r}),
        "priority-queued",
        None,
    )
    .await
    .unwrap();
    let running = operations::enqueue(
        &mut c,
        "rights",
        "test.rights",
        &json!({"release_id":r}),
        "priority-running",
        None,
    )
    .await
    .unwrap();
    drop(c);
    let held = operations::claim(&pool, "rights", "already-running", 300)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(held.id, running);
    let v = create_ok(&app, &u, release_order("PRIORITY_DELIVERY", r)).await;
    let v = pay_queue(&app, &a, v).await;
    drain(&pool, store).await;
    let p: i32 = sqlx::query_scalar("SELECT priority FROM operations.jobs WHERE id=$1")
        .bind(queued)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(p, 10);
    let (p, st): (i32, String) =
        sqlx::query_as("SELECT priority,status FROM operations.jobs WHERE id=$1")
            .bind(running)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((p, st), (0, "RUNNING".into()));
    let mut c = pool.acquire().await.unwrap();
    let future = operations::enqueue(
        &mut c,
        "distribution",
        "test.future",
        &json!({"release_id":r}),
        "priority-future",
        None,
    )
    .await
    .unwrap();
    let old = operations::enqueue(
        &mut c,
        "distribution",
        "test.old",
        &json!({}),
        "priority-aging",
        None,
    )
    .await
    .unwrap();
    drop(c);
    let p: i32 = sqlx::query_scalar("SELECT priority FROM operations.jobs WHERE id=$1")
        .bind(future)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(p, 10);
    sqlx::query("UPDATE operations.jobs SET created_at=now()-interval '2 hours' WHERE id=$1")
        .bind(old)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        operations::claim(&pool, "distribution", "aging", 300)
            .await
            .unwrap()
            .unwrap()
            .id,
        old
    );
    let current = get_order(&app, &u, id(&v)).await;
    let (s, cancelled) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/addons/orders/{}/cancel", u.org, id(&v)),
        json!({"row_version":current["row_version"],"reason":"Cancel before processing"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["refund_status"], "REQUESTED");
    let p: i32 = sqlx::query_scalar("SELECT priority FROM operations.jobs WHERE id=$1")
        .bind(future)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(p, 0);
}
#[sqlx::test]
async fn cancelled_order_jobs_cannot_create_external_work(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, false)).await;
    let started = start(&app, &a, v).await;
    // Publish intents first, then cancel before a provider handler can execute.
    loop {
        let mut c = pool.acquire().await.unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM operations.jobs WHERE kind='outbox.record' AND status='QUEUED'",
        )
        .fetch_one(&mut *c)
        .await
        .unwrap();
        drop(c);
        if n == 0 {
            break;
        }
        let j = operations::claim(&pool, "interactive", "publish", 300)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(j.kind, "outbox.record");
        let st: Arc<dyn ObjectStore> = store.clone();
        operations::execute(&pool, &st, &j).await.unwrap();
    }
    let (s, v) = call(
        &app,
        "POST",
        &format!("/api/orgs/{}/addons/orders/{}/cancel", u.org, id(&started)),
        json!({"row_version":started["row_version"],"reason":"Artist cancellation"}),
        Some(&u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    drain(&pool, store).await;
    let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.addon_provider_tasks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tasks, 0);
    assert_eq!(
        get_order(&app, &u, id(&started)).await["status"],
        "CANCELLED"
    );
}
#[sqlx::test]
async fn outbox_receipt_retry_deduplicates_jobs(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, false)).await;
    let started = start(&app, &a, v).await;
    drain(&pool, store.clone()).await;
    let row=sqlx::query("SELECT j.id,j.payload,j.queue,j.kind FROM operations.jobs j JOIN operations.outbox o ON o.id=(j.payload->>'event_id')::uuid WHERE j.kind='outbox.record' AND o.aggregate_id=$1 AND o.event_type='addon.dispatch'").bind(id(&started)).fetch_one(&pool).await.unwrap();
    let job: Uuid = row.get("id");
    sqlx::query("UPDATE operations.jobs SET status='QUEUED',attempts=0 WHERE id=$1")
        .bind(job)
        .execute(&pool)
        .await
        .unwrap();
    drain(&pool, store).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.jobs WHERE kind='addon.profile.process'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
    let receipts:i64=sqlx::query_scalar("SELECT count(*) FROM operations.event_receipts WHERE event_id=(SELECT id FROM operations.outbox WHERE event_type='addon.dispatch' AND aggregate_id=$1)").bind(id(&started)).fetch_one(&pool).await.unwrap();
    assert_eq!(receipts, 1);
}
#[sqlx::test]
async fn manual_payment_assignment_override_and_refund_have_audit(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, true)).await;
    let assigned = action(
        &app,
        &a,
        &v,
        "assign",
        json!({"assigned_admin_user_id":a.user}),
    )
    .await;
    let paid = action(
        &app,
        &a,
        &assigned,
        "status",
        json!({"status":"PAID","payment_reference":"bank-manual-verified"}),
    )
    .await;
    let cancelled = action(&app, &a, &paid, "status", json!({"status":"CANCELLED"})).await;
    let refunded = action(
        &app,
        &a,
        &cancelled,
        "refund",
        json!({"refund_reference":"bank-refund-receipt"}),
    )
    .await;
    assert_eq!(refunded["payment_status"], "REFUNDED");
    let rows:Vec<(String,Value,Value)>=sqlx::query_as("SELECT action,before_value,after_value FROM operations.audit_events WHERE resource_id=$1 AND actor_user_id=$2 AND action LIKE 'addon.%'").bind(id(&v)).bind(a.user).fetch_all(&pool).await.unwrap();
    for name in [
        "addon.assigned",
        "addon.payment.override",
        "addon.admin.override",
        "addon.refund.override",
    ] {
        assert!(
            rows.iter()
                .any(|r| r.0 == name && !r.1.is_null() && !r.2.is_null()),
            "missing {name}"
        );
    }
}

struct RetryAdapter;
#[async_trait]
impl ProviderAdapter for RetryAdapter {
    async fn prepare(&self, _: &WorkItem) -> Result<AdapterOutcome> {
        Ok(AdapterOutcome::Retryable {
            code: "TEMPORARY_PARTNER_FAILURE",
        })
    }
}
#[sqlx::test]
async fn retryable_provider_failure_preserves_order_until_dead_letter(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, false)).await;
    let started = start(&app, &a, v).await;
    let st: Arc<dyn ObjectStore> = store;
    loop {
        let j = operations::claim(&pool, "interactive", "failure-test", 300)
            .await
            .unwrap()
            .unwrap();
        if j.kind.starts_with("addon.") {
            jobs::execute_with(&pool, &j, &RetryAdapter).await.unwrap();
            assert_eq!(
                get_order(&app, &u, id(&started)).await["status"],
                "IN_PROGRESS"
            );
            sqlx::query("UPDATE operations.jobs SET run_at=now(),max_attempts=2 WHERE id=$1")
                .bind(j.id)
                .execute(&pool)
                .await
                .unwrap();
            let j = operations::claim(&pool, "interactive", "failure-test", 300)
                .await
                .unwrap()
                .unwrap();
            jobs::execute_with(&pool, &j, &RetryAdapter).await.unwrap();
            break;
        }
        operations::execute(&pool, &st, &j).await.unwrap();
    }
    assert_eq!(get_order(&app, &u, id(&started)).await["status"], "FAILED");
}

#[sqlx::test]
async fn migration_takedown_requires_delivery_and_matching(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let r = create_release(&app, &u).await;
    let v=create_ok(&app,&u,json!({"service_code":"MIGRATION","target_type":"release","target_id":r,"details":{"type":"migration","previous_distributor":"Previous","preserve_upc":true,"original_release_date":"2023-05-10","previous_release_urls":["https://example.org/release"]}})).await;
    let started = start(&app, &a, v).await;
    drain(&pool, store).await;
    let mut v = get_order(&app, &u, id(&started)).await;
    let(s,_)=call(&app,"POST",&format!("/api/admin/addons/orders/{}/results",id(&v)),json!({"row_version":v["row_version"],"reason":"Too early","external_reference":"case","migration_step":"TAKEDOWN_REQUESTED"}),Some(&a)).await;
    assert_eq!(s, StatusCode::CONFLICT);
    for step in [
        "UPC_APPROVED",
        "DELIVERED",
        "MATCH_CONFIRMED",
        "TAKEDOWN_REQUESTED",
        "TAKEDOWN_COMPLETED",
    ] {
        v = action(
            &app,
            &a,
            &v,
            "results",
            json!({"external_reference":"migration-case","migration_step":step}),
        )
        .await;
    }
    assert_eq!(
        action(&app, &a, &v, "complete", json!({})).await["status"],
        "COMPLETED"
    );
}

#[test]
fn lrc_validation_rejects_bad_timing_and_video_upload_is_explicit() {
    use audeniq_core::uploads::{expected_container, validate_lrc};
    assert!(validate_lrc("[ar:Test]\n[00:01.00]Hello\n[00:02.50]World").is_ok());
    for s in [
        "plain lyrics",
        "[00:61.00]bad",
        "[00:02]later\n[00:01]earlier",
        "[00:NaN]bad",
    ] {
        assert!(validate_lrc(s).is_err(), "{s}");
    }
    assert_eq!(expected_container("VIDEO", "video/mp4"), Some("M4A"));
    assert_eq!(expected_container("LRC", "text/plain"), Some("LRC"));
    assert_eq!(expected_container("VIDEO", "audio/mp4"), None);
}

#[sqlx::test]
async fn remaining_services_support_manual_results_and_completion(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let r = create_release(&app, &u).await;
    let t = track(&app, &pool, &u, false).await;
    for code in [
        "LYRICS_BASIC",
        "AI_SYNC_LYRICS",
        "PROMO_BASIC",
        "MUSIC_DATA_BASIC",
    ] {
        let b = if matches!(code, "LYRICS_BASIC" | "AI_SYNC_LYRICS") {
            json!({"service_code":code,"target_type":"track","target_id":t,"details":{"type":"lyrics","lyrics_text":"Test lyrics"}})
        } else {
            release_order(code, r)
        };
        let mut v = create_ok(&app, &u, b).await;
        if code == "AI_SYNC_LYRICS" {
            v = pay_queue(&app, &a, v).await;
        }
        v = start(&app, &a, v).await;
        drain(&pool, store.clone()).await;
        v = get_order(&app, &u, id(&v)).await;
        let mut results = json!({"external_reference":format!("manual-{code}")});
        if code == "AI_SYNC_LYRICS" {
            results["lrc_asset_id"] = json!(asset(&pool, &u, "LRC").await);
        }
        if code == "PROMO_BASIC" {
            results["qr_asset_id"] = json!(asset(&pool, &u, "IMAGE").await);
            results["promo_card_asset_id"] = json!(asset(&pool, &u, "IMAGE").await);
            results["dsp_links"] =
                json!([{"platform":"spotify","url":"https://open.spotify.com/album/example"}]);
            assert_eq!(v["provider_tasks"].as_array().unwrap().len(), 2);
        }
        v = action(&app, &a, &v, "results", results).await;
        v = action(&app, &a, &v, "complete", json!({})).await;
        assert_eq!(v["status"], "COMPLETED");
    }
}
#[sqlx::test]
async fn audeniq_mv_review_preparation_then_evidence_then_distribution(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let video = asset(&pool, &u, "VIDEO").await;
    let v=create_ok(&app,&u,json!({"service_code":"MV_REVIEW_AND_GLOBAL","target_type":"music_video","target_id":video,"details":{"type":"mv"}})).await;
    let queued = pay_queue(&app, &a, v).await;
    let v = start(&app, &a, queued).await;
    drain(&pool, store.clone()).await;
    let v = get_order(&app, &u, id(&v)).await;
    assert_eq!(v["provider_tasks"][0]["kind"], "addon.mv.review_prepare");
    let evidence = asset(&pool, &u, "DOCUMENT").await;
    let v = action(
        &app,
        &a,
        &v,
        "results",
        json!({"external_reference":"broadcast-review-case","evidence_asset_id":evidence}),
    )
    .await;
    let v = action(
        &app,
        &a,
        &v,
        "evidence",
        json!({"approved":true,"valid_until":"2028-12-31T00:00:00Z"}),
    )
    .await;
    let v = action(&app, &a, &v, "status", json!({"status":"IN_PROGRESS"})).await;
    drain(&pool, store).await;
    let v = get_order(&app, &u, id(&v)).await;
    assert!(
        v["provider_tasks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["kind"] == "addon.mv.distribute")
    );
}
#[sqlx::test]
async fn expired_mv_evidence_and_revoked_acl_stop_workers(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let video = asset(&pool, &u, "VIDEO").await;
    let evidence = asset(&pool, &u, "DOCUMENT").await;
    let v=create_ok(&app,&u,json!({"service_code":"MV_GLOBAL_ONLY","target_type":"music_video","target_id":video,"details":{"type":"mv","review_evidence_asset_id":evidence}})).await;
    let v = pay_queue(&app, &a, v).await;
    let v = action(
        &app,
        &a,
        &v,
        "evidence",
        json!({"approved":true,"valid_until":"2028-12-31T00:00:00Z"}),
    )
    .await;
    let v = start(&app, &a, v).await;
    sqlx::query("UPDATE catalog.mv_requests SET evidence_valid_until=now()-interval '1 second' WHERE addon_order_id=$1").bind(id(&v)).execute(&pool).await.unwrap();
    drain(&pool, store.clone()).await;
    assert_eq!(get_order(&app, &u, id(&v)).await["status"], "NEEDS_INFO");
    let ar = create_artist(&app, &u).await;
    let v = create_ok(&app, &u, profile(ar, false)).await;
    let v = start(&app, &a, v).await;
    sqlx::query("UPDATE identity.resource_acl SET revoked_at=now() WHERE resource_id=$1")
        .bind(ar)
        .execute(&pool)
        .await
        .unwrap();
    drain(&pool, store).await;
    let status: String = sqlx::query_scalar("SELECT status FROM catalog.addon_orders WHERE id=$1")
        .bind(id(&v))
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, "NEEDS_INFO");
}
#[sqlx::test]
async fn runtime_roles_keep_tenant_rls_and_can_run_addon_outbox(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let u = user(&app).await;
    let other = user(&app).await;
    let a = admin_user(&app, &pool).await;
    let ar = create_artist(&app, &u).await;
    let other_ar = create_artist(&app, &other).await;
    let v = create_ok(&app, &u, profile(ar, false)).await;
    create_ok(&app, &other, profile(other_ar, false)).await;
    let v = start(&app, &a, v).await;
    sqlx::raw_sql("DO $$ BEGIN IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='audeniq_api') THEN CREATE ROLE audeniq_api NOLOGIN; END IF; IF NOT EXISTS(SELECT FROM pg_roles WHERE rolname='audeniq_worker') THEN CREATE ROLE audeniq_worker NOLOGIN; END IF; END $$").execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("../../../deploy/grants.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE audeniq_api")
        .execute(&mut *tx)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.addon_orders")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(n, 0);
    workflow::scope(&mut tx, u.org).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM catalog.addon_orders")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(n, 1);
    tx.rollback().await.unwrap();
    let opts = pool.connect_options();
    let runtime = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET ROLE audeniq_worker").execute(c).await?;
                Ok(())
            })
        })
        .connect_with((*opts).clone())
        .await
        .unwrap();
    drain(&runtime, store).await;
    assert_eq!(
        get_order(&app, &u, id(&v)).await["status"],
        "EXTERNAL_PENDING"
    );
    runtime.close().await;
}

type UploadedFiles = std::collections::BTreeMap<String, (Vec<u8>, String, String)>;
#[derive(Default)]
struct UploadStore {
    files: tokio::sync::Mutex<UploadedFiles>,
}
#[async_trait]
impl ObjectStore for UploadStore {
    async fn presign_put(
        &self,
        _: &str,
        _: i64,
        _: &str,
        nonce: &str,
        expires: chrono::DateTime<chrono::Utc>,
    ) -> Result<audeniq_core::storage::UploadGrant> {
        Ok(audeniq_core::storage::UploadGrant {
            url: "https://upload.example.test/signed".into(),
            method: "PUT",
            headers: std::collections::BTreeMap::from([(
                "x-amz-meta-upload-nonce".into(),
                nonce.into(),
            )]),
            expires_at: expires,
        })
    }
    async fn head(&self, key: &str) -> Result<Option<audeniq_core::storage::ObjectMeta>> {
        Ok(self
            .files
            .lock()
            .await
            .get(key)
            .map(|(b, m, n)| audeniq_core::storage::ObjectMeta {
                size: b.len() as i64,
                content_type: m.clone(),
                nonce: n.clone(),
                etag: sha256_hex(b),
            }))
    }
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()> {
        let mut files = self.files.lock().await;
        let f = files
            .get(source)
            .cloned()
            .ok_or(audeniq_core::error::Error::Storage)?;
        assert_eq!(sha256_hex(&f.0), etag);
        files.insert(target.to_owned(), f);
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        self.files
            .lock()
            .await
            .get(key)
            .map(|v| v.0.clone())
            .ok_or(audeniq_core::error::Error::Storage)
    }
}
async fn upload_app(pool: PgPool) -> (Router, Arc<UploadStore>) {
    audeniq_core::database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(UploadStore::default());
    let s = audeniq_core::api::AppState::new(
        pool,
        audeniq_core::config::Config {
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
    (audeniq_core::api::router(s), store)
}
async fn upload(
    app: &Router,
    store: &UploadStore,
    u: &User,
    kind: &str,
    mime: &str,
    bytes: Vec<u8>,
) -> (StatusCode, Value) {
    let (s, v) = call(
        app,
        "POST",
        &format!("/api/orgs/{}/uploads", u.org),
        json!({"kind":kind,"content_type":mime,"size_bytes":bytes.len()}),
        Some(u),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    store.files.lock().await.insert(
        v["expected_key"].as_str().unwrap().into(),
        (
            bytes,
            mime.into(),
            v["grant"]["headers"]["x-amz-meta-upload-nonce"]
                .as_str()
                .unwrap()
                .into(),
        ),
    );
    call(
        app,
        "POST",
        &format!(
            "/api/orgs/{}/uploads/{}/complete",
            u.org,
            v["upload_session_id"].as_str().unwrap()
        ),
        json!({"asset_id":v["asset_id"],"expected_key":v["expected_key"]}),
        Some(u),
    )
    .await
}
#[sqlx::test]
async fn lrc_uses_signed_upload_and_invalid_text_stays_unregistered(pool: PgPool) {
    let (app, store) = upload_app(pool.clone()).await;
    let u = user(&app).await;
    let (s, v) = upload(
        &app,
        &store,
        &u,
        "LRC",
        "text/plain",
        b"[00:01.00]First\n[00:02.00]Second".to_vec(),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let state: String = sqlx::query_scalar("SELECT state FROM catalog.assets WHERE id=$1")
        .bind(Uuid::parse_str(v["asset_id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "REGISTERED");
    let (s, v) = upload(
        &app,
        &store,
        &u,
        "LRC",
        "text/plain",
        b"Not a timed lyric file".to_vec(),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog.assets WHERE kind='LRC' AND state='REGISTERED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}
#[sqlx::test]
async fn mv_upload_probes_video_and_rejects_audio_container(pool: PgPool) {
    let (app, store) = upload_app(pool.clone()).await;
    let u = user(&app).await;
    let path = std::env::temp_dir().join(format!("addon-upload-test-{}.mp4", Uuid::new_v4()));
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=32x32:d=0.1",
                "-c:v",
                "libx264"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let (s, v) = upload(&app, &store, &u, "VIDEO", "video/mp4", bytes).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let path = std::env::temp_dir().join(format!("addon-audio-disguised-{}.mp4", Uuid::new_v4()));
    assert!(
        std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=duration=0.1",
                "-c:a",
                "aac"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let (s, v) = upload(&app, &store, &u, "VIDEO", "video/mp4", bytes).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM catalog.assets WHERE kind='VIDEO' AND state='REGISTERED'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}
