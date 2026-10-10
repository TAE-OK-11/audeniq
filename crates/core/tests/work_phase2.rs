//! Staff work module, Phase 2: SLA policies and business-time deadlines,
//! task escalation, the exception queue over existing failure sources with
//! duty-checked triage, and the unified inbox with read marks.
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;
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

async fn ok(app: &Router, method: &str, path: &str, body: Value, u: &User) -> Value {
    let (s, v) = send(app, method, path, body, Some(u)).await;
    assert_eq!(s, StatusCode::OK, "{method} {path}: {v}");
    v
}

fn find<'a>(items: &'a Value, id_prefix: &str) -> Option<&'a Value> {
    items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["id"].as_str().unwrap().starts_with(id_prefix))
}

#[sqlx::test]
async fn sla_exceptions_and_inbox(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let op = staff_user(&app, &pool, "OPERATOR").await;
    let support = staff_user(&app, &pool, "SUPPORT").await;
    let artist = user(&app).await;

    // --- SLA policies: ADMIN only, one active policy per type and scope
    let (s, _) = send(&app, "POST", "/api/staff/work/sla/policies",
        json!({"type": "EXCEPTION", "scope_kind": "dsp", "scope_id": "D-5", "target_minutes": 60, "calendar_id": null,
               "warn_ratio": 0.25, "idle_alert_minutes": 30, "escalate_to": "OPERATOR", "auto_raise_priority": true, "enabled": true}),
        Some(&op)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let sla = ok(&app, "POST", "/api/staff/work/sla/policies",
        json!({"type": "EXCEPTION", "scope_kind": "dsp", "scope_id": "D-5", "target_minutes": 60, "calendar_id": null,
               "warn_ratio": 0.25, "idle_alert_minutes": 30, "escalate_to": "OPERATOR", "auto_raise_priority": true, "enabled": true}),
        &admin).await;
    assert!(
        sla["policies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["scope_id"] == "D-5")
    );
    let (s, r) = send(&app, "POST", "/api/staff/work/sla/policies",
        json!({"type": "EXCEPTION", "scope_kind": "dsp", "scope_id": "D-5", "target_minutes": 90, "calendar_id": null,
               "warn_ratio": 0.25, "idle_alert_minutes": 30, "escalate_to": "OPERATOR", "auto_raise_priority": true, "enabled": true}),
        Some(&admin)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "SLA_POLICY_EXISTS");
    // holidays: set and remove, audited
    let v = ok(
        &app,
        "POST",
        "/api/staff/work/calendars/KR_BUSINESS/holidays",
        json!({"day": "2099-01-01", "name": "신정"}),
        &admin,
    )
    .await;
    assert!(
        v["calendars"][0]["holidays"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d == "2099-01-01")
    );
    let v = ok(
        &app,
        "POST",
        "/api/staff/work/calendars/KR_BUSINESS/holidays",
        json!({"day": "2099-01-01"}),
        &admin,
    )
    .await;
    assert!(
        !v["calendars"][0]["holidays"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d == "2099-01-01")
    );
    let (s, _) = send(
        &app,
        "POST",
        "/api/staff/work/calendars/KR_BUSINESS/holidays",
        json!({"day": "2099-01-01", "name": "x"}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // --- Task SLA: from the policy at creation (URGENT_DELIVERY = 2h, 24h clock)
    let t = ok(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "긴급 송출", "type": "URGENT_DELIVERY", "priority": "NORMAL", "assignee_id": op.user}), &admin).await;
    let task: Uuid = t["task"]["id"].as_str().unwrap().parse().unwrap();
    let due: chrono::DateTime<chrono::Utc> =
        t["task"]["sla_due_at"].as_str().unwrap().parse().unwrap();
    let mins = (due - chrono::Utc::now()).num_minutes();
    assert!((115..=120).contains(&mins), "{mins}");
    assert_eq!(t["task"]["sla_state"], "OK");

    // A GENERAL task past its SLA escalates to ADMIN with the priority raised once.
    let late = ok(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "늦은 업무", "type": "GENERAL", "priority": "NORMAL", "assignee_id": op.user}), &admin).await;
    let late: Uuid = late["task"]["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE work.tasks SET created_at=now()-interval '3 hours', sla_due_at=now()-interval '1 hour' WHERE id=$1")
        .bind(late).execute(&pool).await.unwrap();

    // --- Exceptions over existing sources: a dead-lettered automation job
    let job = Uuid::new_v4();
    sqlx::query("INSERT INTO operations.jobs(id,queue,kind,payload,status,idempotency_key,last_error,dead_lettered_at)
                 VALUES($1,'qc','stage1','{}','DEAD_LETTER',$2,'boom',now())")
        .bind(job).bind(format!("t:{job}")).execute(&pool).await.unwrap();
    let ex = ok(&app, "GET", "/api/staff/work/exceptions", json!({}), &op).await;
    let item = find(&ex["items"], &format!("job:{job}"))
        .expect("dead job listed")
        .clone();
    assert_eq!(item["kind"], "AUTOMATION_FAILURE");
    assert_eq!(item["can_act"], false); // automation → ADMIN
    let path = format!("/api/staff/work/exceptions/job/{job}");
    let (s, _) = send(&app, "POST", &path, json!({"action": "ACK"}), Some(&op)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, r) = send(
        &app,
        "POST",
        &path,
        json!({"action": "RETRY"}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r["error"]["code"], "EXCEPTION_NOT_RETRYABLE");
    let v = ok(&app, "POST", &path, json!({"action": "ACK"}), &admin).await;
    assert_eq!(v["item"]["triage"]["status"], "ACKNOWLEDGED");
    assert_eq!(v["item"]["triage"]["assignee_id"], admin.user.to_string());
    let (s, r) = send(
        &app,
        "POST",
        &path,
        json!({"action": "IGNORE"}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "RESOLUTION_REQUIRED");
    let v = ok(
        &app,
        "POST",
        &path,
        json!({"action": "RESOLVE", "resolution": "수동 재처리 완료"}),
        &admin,
    )
    .await;
    assert_eq!(v["item"]["closed"], true);
    let ex = ok(&app, "GET", "/api/staff/work/exceptions", json!({}), &admin).await;
    assert!(find(&ex["items"], &format!("job:{job}")).is_none());
    let ex = ok(
        &app,
        "GET",
        "/api/staff/work/exceptions?view=resolved",
        json!({}),
        &admin,
    )
    .await;
    assert!(find(&ex["items"], &format!("job:{job}")).is_some());
    // the source failing again (newer than the resolution) reopens it
    sqlx::query(
        "UPDATE operations.jobs SET dead_lettered_at=now()+interval '1 second' WHERE id=$1",
    )
    .bind(job)
    .execute(&pool)
    .await
    .unwrap();
    let ex = ok(&app, "GET", "/api/staff/work/exceptions", json!({}), &admin).await;
    assert!(find(&ex["items"], &format!("job:{job}")).is_some());

    // Manual report → task (linked, typed, tagged), only once
    let r = ok(&app, "POST", "/api/staff/work/exceptions/reports",
        json!({"kind": "DSP_REJECTION", "severity": "HIGH", "title": "D-5 메타데이터 반려", "detail": "아티스트명 표기", "entity_kind": "dsp", "entity_id": "D-5"}),
        &op).await;
    let manual = r["id"]
        .as_str()
        .unwrap()
        .strip_prefix("manual:")
        .unwrap()
        .to_string();
    let mpath = format!("/api/staff/work/exceptions/manual/{manual}");
    let ex = ok(
        &app,
        "GET",
        "/api/staff/work/exceptions?kind=DSP_REJECTION",
        json!({}),
        &op,
    )
    .await;
    let m = find(&ex["items"], &format!("manual:{manual}")).unwrap();
    assert_eq!(m["can_act"], true);
    assert!(m["sla"]["due_at"].is_string()); // D-5's own EXCEPTION SLA
    let v = ok(&app, "POST", &mpath, json!({"action": "TO_TASK"}), &op).await;
    let tid = v["item"]["triage"]["task_id"].as_str().unwrap().to_string();
    let td = ok(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{tid}"),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(td["task"]["type"], "URGENT_DELIVERY");
    assert_eq!(td["task"]["priority"], "HIGH");
    assert_eq!(td["task"]["assignee_id"], op.user.to_string());
    assert_eq!(td["task"]["links"][0]["kind"], "dsp");
    let (s, r) = send(
        &app,
        "POST",
        &mpath,
        json!({"action": "TO_TASK"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r["error"]["code"], "EXCEPTION_HAS_TASK");
    // support has no delivery duty
    let (s, _) = send(
        &app,
        "POST",
        &mpath,
        json!({"action": "ACK"}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // --- A customer inquiry past its SLA reaches support as an escalation
    let (s, _) = send(&app, "POST", &format!("/api/orgs/{}/inquiries", artist.org),
        json!({"category": "RELEASE", "release_id": null, "subject": "언제 나오나요", "body": "급해요"}), Some(&artist)).await;
    assert_eq!(s, StatusCode::OK);
    sqlx::query("UPDATE portal.inquiries SET created_at=now()-interval '10 days' WHERE org_id=$1")
        .bind(artist.org)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE portal.inquiry_messages SET created_at=now()-interval '10 days' WHERE org_id=$1",
    )
    .bind(artist.org)
    .execute(&pool)
    .await
    .unwrap();

    // --- Mention for the operator
    ok(&app, "POST", "/api/staff/work/notes",
        json!({"target_kind": "task", "target_id": task.to_string(), "body": "확인 부탁해요", "mentions": [op.user]}), &admin).await;

    // --- Inbox (the first read also runs the escalation sweep)
    let inbox = ok(&app, "GET", "/api/staff/work/inbox", json!({}), &op).await;
    let mine = find(&inbox["items"], &format!("task:{task}")).unwrap();
    assert_eq!(mine["category"], "ASSIGNED");
    let overdue = find(&inbox["items"], &format!("task:{late}")).unwrap();
    assert_eq!(overdue["category"], "OVERDUE");
    assert_eq!(overdue["priority"], "HIGH"); // NORMAL raised once
    assert!(
        overdue["flags"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "ESCALATED")
    );
    let mention = inbox["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["category"] == "MENTION")
        .unwrap()
        .clone();
    assert_eq!(mention["read"], false);
    // the operator's own manual exception is assigned to them
    assert!(find(&inbox["items"], &format!("exception:manual:{manual}")).is_some());
    let escalated: i16 = sqlx::query_scalar("SELECT escalation_level FROM work.tasks WHERE id=$1")
        .bind(late)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(escalated, 1);
    let ev: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM work.task_events WHERE task_id=$1 AND action='SLA_ESCALATED'",
    )
    .bind(late)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(ev, 1);

    // escalation goes to the policy's role (GENERAL → ADMIN), not to SUPPORT
    let a = ok(
        &app,
        "GET",
        "/api/staff/work/inbox?category=OVERDUE",
        json!({}),
        &admin,
    )
    .await;
    assert!(find(&a["items"], &format!("task:{late}")).is_some());
    let sp = ok(&app, "GET", "/api/staff/work/inbox", json!({}), &support).await;
    assert!(find(&sp["items"], &format!("task:{late}")).is_none());
    assert!(
        sp["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["category"] == "SUPPORT_ESCALATION")
    );

    // read marks: unread count drops; a changed item is unread again
    let before = inbox["unread"].as_u64().unwrap();
    ok(
        &app,
        "POST",
        "/api/staff/work/inbox/read",
        json!({"ids": [mention["id"]]}),
        &op,
    )
    .await;
    let after = ok(&app, "GET", "/api/staff/work/inbox", json!({}), &op).await;
    assert_eq!(after["unread"].as_u64().unwrap(), before - 1);
    let unread = ok(
        &app,
        "GET",
        "/api/staff/work/inbox?unread=true&category=MENTION",
        json!({}),
        &op,
    )
    .await;
    assert!(unread["items"].as_array().unwrap().is_empty());

    let sum = ok(&app, "GET", "/api/staff/work/summary", json!({}), &op).await;
    assert!(sum["inbox"]["total"].as_u64().unwrap() >= 3);
    assert!(sum["overdue"].as_u64().unwrap() >= 1);
    assert!(sum["exceptions"]["open"].as_u64().unwrap() >= 2);

    // the SLA board sees the overdue inquiry
    let board = ok(&app, "GET", "/api/staff/work/sla", json!({}), &support).await;
    let q = board["board"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["queue"] == "SUPPORT_INQUIRY")
        .unwrap();
    assert_eq!(q["breached"], 1);

    // non-staff are out
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/work/inbox",
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/work/exceptions",
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // everything is audited
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action LIKE 'work.exception_%' OR action LIKE 'work.sla_%' OR action LIKE 'work.holiday_%' OR action='work.task_escalated'")
        .fetch_one(&pool).await.unwrap();
    assert!(n >= 8, "{n}");
}
