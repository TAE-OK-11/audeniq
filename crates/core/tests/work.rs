//! Staff work module (`/api/staff/work/*`): tasks with review and recurrence,
//! the unified inbox, SLA escalation, notes with mentions and chunked
//! attachments, the exception queue (fed by database triggers), CRM, DSP
//! operations and analytics. Only staff can use any of it.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use base64::Engine as _;
use serde_json::{Value, json};
use sha2::Digest;
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

async fn inbox_kinds(app: &Router, u: &User) -> Vec<(String, String)> {
    let (s, r) = send(app, "GET", "/api/staff/work/inbox", json!({}), Some(u)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["kind"].as_str().unwrap().to_string(),
                i["id"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[sqlx::test]
async fn tasks_flow_through_review_and_recur(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let artist = user(&app).await;
    let me = staff_user(&app, &pool, "OPERATOR").await;
    let lead = staff_user(&app, &pool, "ADMIN").await;

    // Not staff: nothing.
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/work/tasks",
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (s, t) = send(
        &app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": "Lumi Records 계약 갱신 검토", "type": "CONTRACT", "priority": "NORMAL",
               "assignee_id": me.user, "reviewer_id": lead.user,
               "recurrence": {"freq": "MONTHLY", "interval": 1},
               "related": [{"kind": "dsp", "id": "D-1", "label": "멜론"}], "tags": ["월말"]}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let id = t["id"].as_str().unwrap().to_string();
    assert!(t["key"].as_str().unwrap().starts_with("WRK-"));
    assert!(t["sla_due_at"].is_string(), "contract SLA applies");
    assert_eq!(t["status"], "TODO");

    // Only real staff can be assigned.
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": "x", "type": "GENERAL", "priority": "LOW", "assignee_id": artist.user}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Related filter and "mine".
    let (_, l) = send(
        &app,
        "GET",
        "/api/staff/work/tasks?view=all&related_kind=dsp&related_id=D-1",
        json!({}),
        Some(&me),
    )
    .await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);
    assert!(
        inbox_kinds(&app, &me)
            .await
            .iter()
            .any(|(k, _)| k == "ASSIGNED")
    );

    // Start, then "done" asks the reviewer.
    let (s, t) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": 1, "status": "IN_PROGRESS"}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": 1, "status": "DONE"}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "stale row_version");
    let (_, t) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": t["row_version"], "status": "DONE"}),
        Some(&me),
    )
    .await;
    assert_eq!(t["status"], "IN_REVIEW");
    assert!(
        inbox_kinds(&app, &lead)
            .await
            .iter()
            .any(|(k, _)| k == "REVIEW_REQUEST")
    );

    // The assignee cannot approve their own review.
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": t["row_version"], "review": "APPROVE"}),
        Some(&me),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Changes requested need a note, then land in the assignee's inbox.
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": t["row_version"], "review": "REQUEST_CHANGES"}),
        Some(&lead),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, t) = send(&app, "POST", &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": t["row_version"], "review": "REQUEST_CHANGES", "review_note": "비교표 추가"}), Some(&lead)).await;
    assert_eq!(t["status"], "IN_PROGRESS");
    assert!(
        inbox_kinds(&app, &me)
            .await
            .iter()
            .any(|(k, _)| k == "CORRECTION")
    );

    let (_, t) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": t["row_version"], "status": "DONE"}),
        Some(&me),
    )
    .await;
    let (_, t) = send(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{id}"),
        json!({"row_version": t["row_version"], "review": "APPROVE"}),
        Some(&lead),
    )
    .await;
    assert_eq!(t["status"], "DONE");
    assert_eq!(t["sla_state"], "MET");

    // Monthly recurrence made the next one, same people.
    let next: (String, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT status, assignee_id, reviewer_id FROM work.tasks WHERE recurred_from=$1::uuid",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(next, ("TODO".into(), Some(me.user), Some(lead.user)));

    let (_, d) = send(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{id}"),
        json!({}),
        Some(&me),
    )
    .await;
    let actions: Vec<&str> = d["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    for a in [
        "CREATED",
        "REVIEW_REQUESTED",
        "CHANGES_REQUESTED",
        "APPROVED",
        "RECURRED",
    ] {
        assert!(actions.contains(&a), "{a} in {actions:?}");
    }
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE resource_id=$1::uuid AND action LIKE 'work.task_%'")
        .bind(&id).fetch_one(&pool).await.unwrap();
    assert!(audited >= 5);
}

#[sqlx::test]
async fn overdue_tasks_escalate_and_raise_priority(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let op = staff_user(&app, &pool, "OPERATOR").await;
    let reviewer = staff_user(&app, &pool, "REVIEWER").await;
    let (_, t) = send(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "Paper Moon 긴급 송출", "type": "URGENT_DELIVERY", "priority": "NORMAL", "assignee_id": reviewer.user}), Some(&op)).await;
    let id = t["id"].as_str().unwrap().to_string();
    // created 3h ago with a 2h (wall clock) SLA
    sqlx::query("UPDATE work.tasks SET created_at=now()-interval '3 hours', sla_due_at=now()-interval '1 hour' WHERE id=$1::uuid")
        .bind(&id).execute(&pool).await.unwrap();

    let (s, sum) = send(&app, "GET", "/api/staff/work/summary", json!({}), Some(&op)).await;
    assert_eq!(s, StatusCode::OK, "{sum}");
    assert_eq!(sum["overdue"], 1);
    let (_, d) = send(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{id}"),
        json!({}),
        Some(&op),
    )
    .await;
    assert_eq!(d["task"]["sla_state"], "OVERDUE");
    assert_eq!(d["task"]["priority"], "URGENT");
    assert_eq!(d["task"]["priority_raised_from"], "NORMAL");
    assert_eq!(d["task"]["escalation_level"], 2);
    let actions: Vec<&str> = d["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"ESCALATED") && actions.contains(&"PRIORITY_RAISED"));
    // escalated to OPERATOR (policy) and the assignee both see the breach
    assert!(
        inbox_kinds(&app, &op)
            .await
            .iter()
            .any(|(k, _)| k == "SLA_BREACH")
    );
    assert!(
        inbox_kinds(&app, &reviewer)
            .await
            .iter()
            .any(|(k, _)| k == "SLA_BREACH")
    );
    // idempotent: a second sweep adds no events
    send(&app, "GET", "/api/staff/work/summary", json!({}), Some(&op)).await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.task_events WHERE task_id=$1::uuid AND action='ESCALATED'",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);

    // SLA policy edits: ADMIN only, and open tasks are re-derived.
    let (s, _) = send(&app, "PUT", "/api/staff/work/sla/URGENT_DELIVERY",
        json!({"type": "URGENT_DELIVERY", "target_minutes": 600, "business_hours": false, "warn_ratio": 0.25, "idle_alert_minutes": 30,
               "escalate_to": "OPERATOR", "auto_raise_priority": true, "enabled": true}), Some(&op)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let (s, r) = send(&app, "PUT", "/api/staff/work/sla/URGENT_DELIVERY",
        json!({"type": "URGENT_DELIVERY", "target_minutes": 600, "business_hours": false, "warn_ratio": 0.25, "idle_alert_minutes": 30,
               "escalate_to": "OPERATOR", "auto_raise_priority": true, "enabled": true}), Some(&admin)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let level: i16 =
        sqlx::query_scalar("SELECT escalation_level FROM work.tasks WHERE id=$1::uuid")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(level, 0, "10h from 3h ago is no longer overdue");
}

#[sqlx::test]
async fn notes_mention_colleagues_and_carry_chunked_attachments(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let a = staff_user(&app, &pool, "REVIEWER").await;
    let b = staff_user(&app, &pool, "SUPPORT").await;
    let release = Uuid::new_v4().to_string();

    let bytes: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
    let sha = hex::encode(sha2::Sha256::digest(&bytes));
    let (s, up) = send(&app, "POST", "/api/staff/work/uploads",
        json!({"name": "비교표.xlsx", "content_type": "application/vnd.ms-excel", "size": bytes.len(), "sha256": sha}), Some(&a)).await;
    assert_eq!(s, StatusCode::OK, "{up}");
    let up = up["id"].as_str().unwrap().to_string();
    // An incomplete upload cannot be attached.
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "release", "target_id": release, "body": "x", "attachments": [up]}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    for (i, chunk) in bytes.chunks(32 * 1024).enumerate() {
        let data = base64::engine::general_purpose::STANDARD.encode(chunk);
        let (s, r) = send(
            &app,
            "POST",
            &format!("/api/staff/work/uploads/{up}/parts"),
            json!({"index": i, "data": data}),
            Some(&a),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{r}");
    }
    // a retried part is harmless; a part out of order is not
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/uploads/{up}/parts"),
        json!({"index": 0, "data": "AA=="}),
        Some(&a),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, n) = send(&app, "POST", "/api/staff/work/notes",
        json!({"target_kind": "release", "target_id": release, "body": format!("@{} 피처링 반려 이력 확인해 주세요", "b"),
               "mentions": [b.user], "attachments": [up]}), Some(&a)).await;
    assert_eq!(s, StatusCode::OK, "{n}");
    assert_eq!(n["attachments"][0]["size"], 70_000);
    let att = n["attachments"][0]["id"].as_str().unwrap().to_string();

    let mentions = inbox_kinds(&app, &b).await;
    assert!(mentions.iter().any(|(k, _)| k == "MENTION"));
    let (_, list) = send(
        &app,
        "GET",
        &format!("/api/staff/work/notes?target_kind=release&target_id={release}"),
        json!({}),
        Some(&b),
    )
    .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);

    // Download as attachment only, same bytes.
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
    let body = axum::body::to_bytes(r.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(body.to_vec(), bytes);

    // Resolving the note clears the mention; read marks stick.
    let nid = n["id"].as_str().unwrap();
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/notes/{nid}/resolve"),
        json!({"resolved": true}),
        Some(&b),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        !inbox_kinds(&app, &b)
            .await
            .iter()
            .any(|(k, _)| k == "MENTION")
    );
}

#[sqlx::test]
async fn automation_feeds_the_exception_queue(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let artist = user(&app).await;
    let op = staff_user(&app, &pool, "OPERATOR").await;

    // A rights proof sent back for more opens RIGHTS_DOCS; approval clears it.
    let doc = Uuid::new_v4();
    sqlx::query("INSERT INTO portal.documents(id,org_id,kind,title,status) VALUES($1,$2,'RIGHTS_PROOF','공동 작곡 권리 확인서','REVIEW')")
        .bind(doc).bind(artist.org).execute(&pool).await.unwrap();
    sqlx::query(
        "UPDATE portal.documents SET status='NEEDS', review_note='서명 페이지 누락' WHERE id=$1",
    )
    .bind(doc)
    .execute(&pool)
    .await
    .unwrap();
    let (s, l) = send(
        &app,
        "GET",
        "/api/staff/work/exceptions?kind=RIGHTS_DOCS",
        json!({}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{l}");
    let items = l["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["source"], "rights-checker");
    assert!(
        inbox_kinds(&app, &op)
            .await
            .iter()
            .any(|(k, _)| k == "RIGHTS")
    );
    sqlx::query("UPDATE portal.documents SET status='APPROVED' WHERE id=$1")
        .bind(doc)
        .execute(&pool)
        .await
        .unwrap();
    let (_, l) = send(
        &app,
        "GET",
        "/api/staff/work/exceptions?status=RESOLVED",
        json!({}),
        Some(&op),
    )
    .await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);

    // Repeats bump one row; a resolved problem that happens again comes back.
    for _ in 0..3 {
        sqlx::query("SELECT work.report_exception('DELIVERY_FAILURE','CRITICAL','FLO 전송 실패','timeout','delivery-runner','job:x','[]')")
            .execute(&pool).await.unwrap();
    }
    let (_, l) = send(
        &app,
        "GET",
        "/api/staff/work/exceptions?kind=DELIVERY_FAILURE",
        json!({}),
        Some(&op),
    )
    .await;
    let ex = &l["items"][0];
    assert_eq!(ex["occurrences"], 3);
    let ex_id = ex["id"].as_str().unwrap().to_string();

    // Hand it to the task engine.
    let (s, ex) = send(
        &app,
        "POST",
        &format!("/api/staff/work/exceptions/{ex_id}"),
        json!({"action": "TO_TASK"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ex}");
    assert_eq!(ex["status"], "ASSIGNED");
    let task = ex["task_id"].as_str().unwrap();
    let (_, t) = send(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{task}"),
        json!({}),
        Some(&op),
    )
    .await;
    assert_eq!(t["task"]["type"], "URGENT_DELIVERY");
    assert_eq!(t["task"]["priority"], "URGENT");
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/exceptions/{ex_id}"),
        json!({"action": "TO_TASK"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/exceptions/{ex_id}"),
        json!({"action": "RESOLVE"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "resolution required");
    let (s, ex) = send(
        &app,
        "POST",
        &format!("/api/staff/work/exceptions/{ex_id}"),
        json!({"action": "RESOLVE", "resolution": "SFTP 복구 후 재전송"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ex}");
    sqlx::query("SELECT work.report_exception('DELIVERY_FAILURE','CRITICAL','FLO 전송 실패','timeout','delivery-runner','job:x','[]')")
        .execute(&pool).await.unwrap();
    let st: String = sqlx::query_scalar("SELECT status FROM work.exceptions WHERE id=$1::uuid")
        .bind(&ex_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "ASSIGNED", "reopened to the same person");

    // Manual report (settlement mismatch found by a person).
    let (s, r) = send(&app, "POST", "/api/staff/work/exceptions",
        json!({"kind": "SETTLEMENT_MISMATCH", "severity": "HIGH", "title": "9월 리포트 합계 차이", "detail": "3.1%"}), Some(&op)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["source"], "staff:operator");
}

#[sqlx::test]
async fn crm_dsp_operations_and_analytics(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let support = staff_user(&app, &pool, "SUPPORT").await;
    let op = staff_user(&app, &pool, "OPERATOR").await;

    let (s, c) = send(&app, "POST", "/api/staff/work/contacts",
        json!({"kind": "LABEL", "name": "Lumi Records", "person": "정하늘 대표", "email": "ceo@lumi.example",
               "owner_id": support.user, "contract_stage": "RENEWAL", "next_follow_up_at": "2020-01-01T00:00:00Z"}), Some(&support)).await;
    assert_eq!(s, StatusCode::OK, "{c}");
    let cid = c["id"].as_str().unwrap().to_string();
    let (_, sum) = send(
        &app,
        "GET",
        "/api/staff/work/summary",
        json!({}),
        Some(&support),
    )
    .await;
    assert_eq!(sum["follow_ups_due"], 1);
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/contacts/{cid}/activities"),
        json!({"kind": "CALL", "summary": "갱신 조건 1차 통화", "next_follow_up_at": null}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, d) = send(
        &app,
        "GET",
        &format!("/api/staff/work/contacts/{cid}"),
        json!({}),
        Some(&support),
    )
    .await;
    assert!(d["contact"]["last_contact_at"].is_string());
    assert!(d["contact"]["next_follow_up_at"].is_null());
    assert_eq!(d["activities"].as_array().unwrap().len(), 1);
    let (_, l) = send(
        &app,
        "GET",
        "/api/staff/work/contacts?q=lumi",
        json!({}),
        Some(&support),
    )
    .await;
    assert_eq!(l["items"].as_array().unwrap().len(), 1);

    // DSP operations: every active DSP, editable by operators only.
    let (s, ops) = send(
        &app,
        "GET",
        "/api/staff/work/dsp-ops",
        json!({}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ops}");
    let first: &Value = &ops["items"][0];
    let code = first["code"].as_str().unwrap().to_string();
    assert_eq!(first["failure_trend"].as_array().unwrap().len(), 8);
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/dsp-ops/{code}"),
        json!({"sla_hours": 48}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, d) = send(&app, "POST", &format!("/api/staff/work/dsp-ops/{code}"),
        json!({"sla_hours": 48, "owner_id": op.user, "contract_stage": "SIGNED", "health": "DEGRADED"}), Some(&op)).await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["sla_hours"], 48);
    assert_eq!(d["health"], "DEGRADED");

    let (s, a) = send(
        &app,
        "GET",
        "/api/staff/work/analytics?days=7",
        json!({}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    assert_eq!(a["daily"].as_array().unwrap().len(), 7);
    let manual: i64 = a["daily"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["manual"].as_i64().unwrap())
        .sum();
    assert!(
        manual >= 3,
        "staff work actions are counted from the audit log"
    );
    let (s, p) = send(
        &app,
        "GET",
        "/api/staff/work/people",
        json!({}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(p["items"].as_array().unwrap().len(), 2);
}
