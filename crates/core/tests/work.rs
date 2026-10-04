//! Staff work module, Phase 1 (`/api/staff/work/*`): tasks and subtasks with
//! review and recurrence, links to any entity (labels read from the source
//! tables, nothing copied), internal notes with mentions, edit history,
//! resolve and archive, and object-store attachments. Only staff can use it,
//! sensitive actions are permission-checked and every change is audited.
use async_trait::async_trait;
use audeniq_core::{
    api::{AppState, router},
    config::Config,
    database,
    error::{Error, Result},
    storage::{ObjectMeta, ObjectStore, UploadGrant},
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

mod support;
use support::{ORIGIN, SECRET, User, send, user};

/// Object store double with presigned PUT, freeze and get.
#[derive(Default)]
struct Store {
    objects: Mutex<BTreeMap<String, (ObjectMeta, Vec<u8>)>>,
}
impl Store {
    async fn upload(&self, key: &str, bytes: &[u8], content_type: &str, nonce: &str) {
        let meta = ObjectMeta {
            size: bytes.len() as i64,
            content_type: content_type.into(),
            nonce: nonce.into(),
            etag: "e1".into(),
        };
        self.objects
            .lock()
            .await
            .insert(key.into(), (meta, bytes.to_vec()));
    }
}
#[async_trait]
impl ObjectStore for Store {
    async fn presign_put(
        &self,
        key: &str,
        size: i64,
        mime: &str,
        nonce: &str,
        expires: DateTime<Utc>,
    ) -> Result<UploadGrant> {
        Ok(UploadGrant {
            url: format!("https://mock.invalid/{key}"),
            method: "PUT",
            headers: BTreeMap::from([
                ("content-length".into(), size.to_string()),
                ("content-type".into(), mime.into()),
                ("x-amz-meta-upload-nonce".into(), nonce.into()),
            ]),
            expires_at: expires,
        })
    }
    async fn head(&self, key: &str) -> Result<Option<ObjectMeta>> {
        Ok(self.objects.lock().await.get(key).map(|(m, _)| ObjectMeta {
            size: m.size,
            content_type: m.content_type.clone(),
            nonce: m.nonce.clone(),
            etag: m.etag.clone(),
        }))
    }
    async fn freeze(&self, source: &str, target: &str, etag: &str) -> Result<()> {
        let mut m = self.objects.lock().await;
        let (meta, body) = m
            .get(source)
            .map(|(m, b)| {
                (
                    ObjectMeta {
                        size: m.size,
                        content_type: m.content_type.clone(),
                        nonce: m.nonce.clone(),
                        etag: m.etag.clone(),
                    },
                    b.clone(),
                )
            })
            .ok_or(Error::Storage)?;
        if meta.etag != etag {
            return Err(Error::Conflict);
        }
        m.insert(target.into(), (meta, body));
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        self.objects
            .lock()
            .await
            .get(key)
            .map(|(_, b)| b.clone())
            .ok_or(Error::Storage)
    }
}

async fn app(pool: PgPool) -> (Router, Arc<Store>) {
    database::MIGRATOR.run(&pool).await.unwrap();
    let store = Arc::new(Store::default());
    let s = AppState::new(
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
    (router(s), store)
}

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

async fn audited(pool: &PgPool, resource: &str, action: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events WHERE resource_id=$1::uuid AND action=$2",
    )
    .bind(resource)
    .bind(action)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn rv(v: &Value) -> Value {
    v["task"]["row_version"].clone()
}

#[sqlx::test]
async fn tasks_subtasks_review_recurrence_and_archive(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let artist = user(&app).await;
    let me = staff_user(&app, &pool, "OPERATOR").await;
    let lead = staff_user(&app, &pool, "ADMIN").await;
    let other = staff_user(&app, &pool, "SUPPORT").await;

    // Not staff: nothing at all.
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/work/tasks",
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (s, t) = send(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "Lumi Records 계약 갱신 검토", "type": "CONTRACT", "priority": "NORMAL",
               "assignee_id": me.user, "reviewer_id": lead.user, "recurrence": {"freq": "MONTHLY", "interval": 1},
               "links": [{"entity_kind": "dsp", "entity_id": "D-1"}, {"entity_kind": "org", "entity_id": artist.org.to_string()}],
               "tags": ["월말"]}), Some(&me)).await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let id = t["task"]["id"].as_str().unwrap().to_string();
    assert!(t["task"]["key"].as_str().unwrap().starts_with("WRK-"));
    // labels come from the source tables (DSP registry, identity.orgs)
    let links = t["task"]["links"].as_array().unwrap();
    assert_eq!(links.len(), 2);
    assert!(
        links
            .iter()
            .any(|l| l["kind"] == "dsp" && l["label"].is_string())
    );
    assert!(links.iter().all(|l| l["missing"].is_null()));

    // Unknown entities, non-staff assignees and self links are refused.
    let (s, r) = send(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "x", "type": "GENERAL", "priority": "LOW", "links": [{"entity_kind": "release", "entity_id": Uuid::new_v4().to_string()}]}), Some(&me)).await;
    assert_eq!(
        (s, r["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("ENTITY_NOT_FOUND"))
    );
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": "x", "type": "GENERAL", "priority": "LOW", "assignee_id": artist.user}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Subtask (one level only).
    let (s, sub) = send(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "수수료 비교표", "type": "CONTRACT", "priority": "NORMAL", "parent_id": id, "assignee_id": other.user}), Some(&me)).await;
    assert_eq!(s, StatusCode::OK, "{sub}");
    let sub_id = sub["task"]["id"].as_str().unwrap().to_string();
    assert_eq!(sub["parent"]["id"], id.as_str());
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": "x", "type": "GENERAL", "priority": "LOW", "parent_id": sub_id}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no sub-subtasks");

    // Filters: entity link, priority, top-level by default.
    let (_, l) = send(
        &app,
        "GET",
        "/api/staff/work/tasks?view=all&entity_kind=dsp&entity_id=D-1",
        json!({}),
        Some(&me),
    )
    .await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);
    let (_, l) = send(
        &app,
        "GET",
        "/api/staff/work/tasks?view=all",
        json!({}),
        Some(&me),
    )
    .await;
    assert_eq!(
        l["items"].as_array().unwrap().len(),
        1,
        "subtasks hidden from the top-level list"
    );
    assert_eq!(l["items"][0]["subtasks"]["total"], 1);

    // Start; "done" with an open subtask is refused; then review flow.
    let (_, d) = send(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{id}"),
        json!({}),
        Some(&me),
    )
    .await;
    let (s, d) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "status": "IN_PROGRESS"}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": 1, "priority": "HIGH"}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "stale row_version");
    // done → asks the reviewer (subtask still open blocks only the final DONE)
    let (_, d) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "status": "DONE"}),
        Some(&me),
    )
    .await;
    assert_eq!(d["task"]["status"], "IN_REVIEW");
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "review": "APPROVE"}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "the assignee cannot approve");
    let (s, r) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "review": "APPROVE"}),
        Some(&lead),
    )
    .await;
    assert_eq!(
        (s, r["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("TASK_SUBTASKS_OPEN")),
        "{r}"
    );
    let (s, r) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "review": "REQUEST_CHANGES"}),
        Some(&lead),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "note required: {r}");
    let (_, d) = send(&app, "POST", &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "review": "REQUEST_CHANGES", "review_note": "지난 분기 조건도 넣어 주세요"}), Some(&lead)).await;
    assert_eq!(d["task"]["status"], "IN_PROGRESS");
    assert_eq!(d["task"]["changes_requested"], true);

    // finish the subtask, then the task, then approve
    let (_, sd) = send(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{sub_id}"),
        json!({}),
        Some(&other),
    )
    .await;
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{sub_id}"),
        json!({"row_version": rv(&sd), "status": "DONE"}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, d) = send(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{id}"),
        json!({}),
        Some(&me),
    )
    .await;
    let (_, d) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "status": "DONE"}),
        Some(&me),
    )
    .await;
    let (s, d) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": rv(&d), "review": "APPROVE"}),
        Some(&lead),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["task"]["status"], "DONE");

    // Monthly recurrence: next task with the same people and links.
    let next: (Uuid, String, Option<Uuid>, i64) = sqlx::query_as(
        "SELECT t.id, t.status, t.reviewer_id, (SELECT count(*) FROM work.links l WHERE l.subject_id=t.id AND l.archived_at IS NULL)
           FROM work.tasks t WHERE t.recurred_from=$1::uuid",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (next.1.as_str(), next.2, next.3),
        ("TODO", Some(lead.user), 2)
    );

    // History and audit trail.
    let actions: Vec<String> = d["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap().to_string())
        .collect();
    for a in [
        "CREATED",
        "LINKED",
        "SUBTASK_ADDED",
        "REVIEW_REQUESTED",
        "CHANGES_REQUESTED",
        "APPROVED",
        "RECURRED",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a} in {actions:?}");
    }
    assert_eq!(audited(&pool, &id, "work.task_created").await, 1);
    assert!(audited(&pool, &id, "work.task_updated").await >= 4);
    assert_eq!(audited(&pool, &id, "work.link_added").await, 2);

    // Unlink (archived, not deleted).
    let link_id = d["task"]["links"][0]["link_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, d) = send(
        &app,
        "POST",
        &format!("/api/staff/work/links/{link_id}/archive"),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(d["task"]["links"].as_array().unwrap().len(), 1);
    let kept: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.links WHERE id=$1::uuid AND archived_at IS NOT NULL",
    )
    .bind(&link_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(kept, 1);

    // Archive: not by an unrelated staff member; creator can; subtasks follow.
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}/archive"),
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, a) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}/archive"),
        json!({"reason": "중복"}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["archived"].as_array().unwrap().len(), 2);
    let (_, l) = send(
        &app,
        "GET",
        "/api/staff/work/tasks?view=all&status=archived",
        json!({}),
        Some(&me),
    )
    .await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": 99, "title": "x"}),
        Some(&me),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::UNPROCESSABLE_ENTITY,
        "archived tasks are read-only"
    );
    assert_eq!(audited(&pool, &id, "work.task_archived").await, 1);

    // Entity search for the picker.
    let (s, r) = send(
        &app,
        "GET",
        "/api/staff/work/entities/search?kind=dsp&q=",
        json!({}),
        Some(&other),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(!r["items"].as_array().unwrap().is_empty());
}

#[sqlx::test]
async fn notes_mentions_history_and_attachments(pool: PgPool) {
    let (app, store) = app(pool.clone()).await;
    let artist = user(&app).await;
    let a = staff_user(&app, &pool, "REVIEWER").await;
    let b = staff_user(&app, &pool, "SUPPORT").await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let org = artist.org.to_string();

    // Attachment: presigned upload, completion checks, frozen out of quarantine.
    let (s, r) = send(
        &app,
        "POST",
        "/api/staff/work/attachments",
        json!({"name": "x.exe", "content_type": "application/x-msdownload", "size_bytes": 10}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    let pdf = b"%PDF-1.4 comparison".to_vec();
    let (s, up) = send(&app, "POST", "/api/staff/work/attachments", json!({"name": "수수료 비교.pdf", "content_type": "application/pdf", "size_bytes": pdf.len()}), Some(&a)).await;
    assert_eq!(s, StatusCode::OK, "{up}");
    let att = up["id"].as_str().unwrap().to_string();
    let url = up["grant"]["url"].as_str().unwrap();
    let key = url.trim_start_matches("https://mock.invalid/").to_string();
    let nonce = up["grant"]["headers"]["x-amz-meta-upload-nonce"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/attachments/{att}/complete"),
        json!({}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY, "not uploaded yet");
    store.upload(&key, &pdf, "application/pdf", &nonce).await;
    let (s, done) = send(
        &app,
        "POST",
        &format!("/api/staff/work/attachments/{att}/complete"),
        json!({}),
        Some(&b),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "only the uploader completes: {done}"
    );
    let (s, done) = send(
        &app,
        "POST",
        &format!("/api/staff/work/attachments/{att}/complete"),
        json!({}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{done}");

    // Note on an org (CRM-style entity), mentioning b, with the attachment.
    let (s, r) = send(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "release", "target_id": Uuid::new_v4().to_string(), "body": "x"}),
        Some(&a),
    )
    .await;
    assert_eq!(
        (s, r["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("ENTITY_NOT_FOUND"))
    );
    let (s, r) = send(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "org", "target_id": org, "body": "x", "mentions": [artist.user]}),
        Some(&a),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "only staff can be mentioned: {r}"
    );
    let (s, n) = send(&app, "POST", "/api/staff/work/notes",
        json!({"target_kind": "org", "target_id": org, "body": "@b 갱신 조건 확인 부탁해요", "mentions": [b.user], "attachments": [att]}), Some(&a)).await;
    assert_eq!(s, StatusCode::OK, "{n}");
    let nid = n["id"].as_str().unwrap().to_string();
    assert_eq!(n["attachments"][0]["name"], "수수료 비교.pdf");
    // the same attachment cannot be reused
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "org", "target_id": org, "body": "y", "attachments": [att]}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Mentioned person sees it, with the target label from identity.orgs.
    let (_, m) = send(&app, "GET", "/api/staff/work/mentions", json!({}), Some(&b)).await;
    assert_eq!(m["items"].as_array().unwrap().len(), 1);
    assert!(m["items"][0]["target"]["label"].is_string());

    // Download as attachment only.
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/staff/work/attachments/{att}"))
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("cookie", &b.cookie)
        .body(Body::empty())
        .unwrap();
    let r = app.clone().oneshot(req).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["content-type"], "application/octet-stream");
    assert!(
        r.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(
        axum::body::to_bytes(r.into_body(), 1 << 20)
            .await
            .unwrap()
            .to_vec(),
        pdf
    );
    assert_eq!(audited(&pool, &att, "work.attachment_viewed").await, 1);

    // Edit: author only; history kept.
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/notes/{nid}"),
        json!({"body": "hijack"}),
        Some(&b),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, e) = send(
        &app,
        "POST",
        &format!("/api/staff/work/notes/{nid}"),
        json!({"body": "@b 갱신 조건 확인 부탁해요 (수정)", "mentions": [b.user]}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{e}");
    assert_eq!(e["revisions"], 1);
    assert!(e["edited_at"].is_string());
    let (_, h) = send(
        &app,
        "GET",
        &format!("/api/staff/work/notes/{nid}/revisions"),
        json!({}),
        Some(&b),
    )
    .await;
    assert_eq!(h["items"][0]["body"], "@b 갱신 조건 확인 부탁해요");

    // Resolve clears the mention; archive by a non-author non-admin is refused.
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/notes/{nid}/resolve"),
        json!({"resolved": true}),
        Some(&b),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, m) = send(&app, "GET", "/api/staff/work/mentions", json!({}), Some(&b)).await;
    assert!(m["items"].as_array().unwrap().is_empty());
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/notes/{nid}/archive"),
        json!({}),
        Some(&b),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/notes/{nid}/archive"),
        json!({}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, l) = send(
        &app,
        "GET",
        &format!("/api/staff/work/notes?target_kind=org&target_id={org}"),
        json!({}),
        Some(&b),
    )
    .await;
    assert!(l["items"].as_array().unwrap().is_empty());
    let (_, l) = send(
        &app,
        "GET",
        &format!("/api/staff/work/notes?target_kind=org&target_id={org}&archived=true"),
        json!({}),
        Some(&b),
    )
    .await;
    assert_eq!(
        l["items"].as_array().unwrap().len(),
        1,
        "archived, not deleted"
    );
    // archived attachments are no longer served
    let req = Request::builder()
        .method("GET")
        .uri(format!("/api/staff/work/attachments/{att}"))
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("cookie", &b.cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );

    for action in [
        "work.note_added",
        "work.note_edited",
        "work.note_resolved",
        "work.note_archived",
    ] {
        assert_eq!(audited(&pool, &nid, action).await, 1, "{action}");
    }

    // Customers cannot read notes; there is no tenant route, and the staff route refuses them.
    let (s, _) = send(
        &app,
        "GET",
        &format!("/api/staff/work/notes?target_kind=org&target_id={org}"),
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // A note on a task shows in the task history.
    let (_, t) = send(
        &app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": "정산 확인", "type": "SETTLEMENT", "priority": "LOW"}),
        Some(&a),
    )
    .await;
    let tid = t["task"]["id"].as_str().unwrap().to_string();
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "task", "target_id": tid, "body": "리포트 받음"}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, d) = send(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{tid}"),
        json!({}),
        Some(&a),
    )
    .await;
    assert_eq!(d["task"]["note_count"], 1);
    assert!(
        d["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "NOTE")
    );
}
