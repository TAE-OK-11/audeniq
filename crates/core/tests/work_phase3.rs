//! Staff work module, Phase 3: CRM Lite (relationships over source records,
//! contacts, contact history, links, derived contract status, inbox follow-up)
//! and DSP operations (registry + delivery numbers + exceptions + relationship).
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

#[sqlx::test]
async fn crm_and_dsp_operations(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let op = staff_user(&app, &pool, "OPERATOR").await;
    let artist = user(&app).await;
    let org_name: String = sqlx::query_scalar("SELECT name FROM identity.orgs WHERE id=$1")
        .bind(artist.org)
        .fetch_one(&pool)
        .await
        .unwrap();

    // --- Workspace relationship: name and contract from the source
    let (s, r) = send(&app, "POST", "/api/staff/work/relationships",
        json!({"kind": "ORG", "entity_kind": "org", "entity_id": artist.org.to_string(), "name": "다른 이름"}), Some(&op)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "NAME_FROM_SOURCE");
    let (s, r) = send(
        &app,
        "POST",
        "/api/staff/work/relationships",
        json!({"kind": "ARTIST", "entity_kind": "org", "entity_id": artist.org.to_string()}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "KIND_MISMATCH");
    let d = ok(&app, "POST", "/api/staff/work/relationships",
        json!({"kind": "ORG", "entity_kind": "org", "entity_id": artist.org.to_string(), "owner_id": op.user,
               "next_contact_at": "2020-01-01T00:00:00Z", "tags": ["핵심"]}), &op).await;
    let rel = d["relationship"]["id"].as_str().unwrap().to_string();
    assert_eq!(d["relationship"]["name"], org_name);
    assert_eq!(d["relationship"]["contract"]["status"], "NONE");
    assert_eq!(d["relationship"]["contract"]["source"], "agreements");
    assert_eq!(d["relationship"]["contact_due"], true);
    // one active relationship per source record
    let (s, r) = send(
        &app,
        "POST",
        "/api/staff/work/relationships",
        json!({"kind": "ORG", "entity_kind": "org", "entity_id": artist.org.to_string()}),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "RELATIONSHIP_EXISTS");
    // a signed agreement in the workspace shows up as the contract status (read, not copied)
    sqlx::query("INSERT INTO portal.documents(id,org_id,kind,title,status) VALUES($1,$2,'AGREEMENT','배급 계약','REVIEW')")
        .bind(Uuid::new_v4()).bind(artist.org).execute(&pool).await.unwrap();
    let d = ok(
        &app,
        "GET",
        &format!("/api/staff/work/relationships/{rel}"),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(d["relationship"]["contract"]["status"], "NEGOTIATING");
    assert_eq!(d["agreements"].as_array().unwrap().len(), 1);

    // contacts (first one primary), activities (last contact derived), next contact moved
    let d = ok(&app, "POST", &format!("/api/staff/work/relationships/{rel}/contacts"),
        json!({"name": "이서연", "role": "A&R", "email": "sy@example.test", "phone": "010-1234-5678"}), &op).await;
    let contact = d["contacts"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(d["contacts"][0]["is_primary"], true);
    let (s, r) = send(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{rel}/contacts"),
        json!({"name": "x", "email": "not-an-email"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "EMAIL_INVALID");
    let d = ok(&app, "POST", &format!("/api/staff/work/relationships/{rel}/activities"),
        json!({"kind": "CALL", "summary": "수수료 조정 요청 받음", "contact_id": contact, "next_contact_at": "2099-01-01T00:00:00Z"}), &op).await;
    assert!(d["relationship"]["last_contact_at"].is_string());
    assert_eq!(d["relationship"]["contact_due"], false);
    assert_eq!(d["activities"][0]["contact"], "이서연");
    // activities are append-only in the database
    assert!(
        sqlx::query("UPDATE work.relationship_activities SET summary='x'")
            .execute(&pool)
            .await
            .is_err()
    );

    // links to related records; notes and tasks on the relationship
    let release = Uuid::new_v4();
    sqlx::query("INSERT INTO identity.resources(org_id,id,kind) VALUES($1,$2,'release')")
        .bind(artist.org)
        .bind(release)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO catalog.releases(id,org_id,title,release_type,status,draft) VALUES($1,$2,'연결 발매','SINGLE','DRAFT','{}')")
        .bind(release).bind(artist.org).execute(&pool).await.unwrap();
    let d = ok(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{rel}/links"),
        json!({"entity_kind": "release", "entity_id": release.to_string()}),
        &op,
    )
    .await;
    assert_eq!(d["links"][0]["label"], "연결 발매");
    let link = d["links"][0]["link_id"].as_str().unwrap().to_string();
    ok(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "relationship", "target_id": rel, "body": "오후 연락 선호"}),
        &op,
    )
    .await;
    ok(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "계약 갱신 검토", "type": "CONTRACT", "priority": "NORMAL", "links": [{"entity_kind": "relationship", "entity_id": rel}]}), &op).await;
    let d = ok(
        &app,
        "GET",
        &format!("/api/staff/work/relationships/{rel}"),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(d["relationship"]["open_tasks"], 1);
    assert_eq!(d["relationship"]["note_count"], 1);
    let d = ok(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{rel}/links/{link}/archive"),
        json!({}),
        &op,
    )
    .await;
    assert!(d["links"].as_array().unwrap().is_empty());
    // the entity picker finds relationships by their source name
    let found = ok(
        &app,
        "GET",
        &format!(
            "/api/staff/work/entities/search?kind=relationship&q={}",
            urlencode(&org_name)
        ),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(found["items"][0]["id"], rel);

    // update with row_version; stale is a conflict
    let rv = d["relationship"]["row_version"].as_i64().unwrap();
    let d = ok(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{rel}"),
        json!({"row_version": rv, "status": "AT_RISK"}),
        &op,
    )
    .await;
    assert_eq!(d["relationship"]["status"], "AT_RISK");
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{rel}"),
        json!({"row_version": rv, "status": "ACTIVE"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // --- Partner without a source record: own name, hand-tracked contract
    let p = ok(&app, "POST", "/api/staff/work/relationships",
        json!({"kind": "PARTNER", "name": "모노 퍼블리싱", "status": "PROSPECT", "manual_contract_status": "NEGOTIATING",
               "owner_id": op.user, "next_contact_at": "2020-01-01T00:00:00Z"}), &op).await;
    let partner = p["relationship"]["id"].as_str().unwrap().to_string();
    assert_eq!(p["relationship"]["name"], "모노 퍼블리싱");
    assert_eq!(p["relationship"]["contract"]["source"], "manual");
    let (s, r) = send(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{rel}"),
        json!({"row_version": rv + 1, "manual_contract_status": "SIGNED"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "CONTRACT_FROM_SOURCE");
    // the owner's inbox has the follow-up
    let inbox = ok(
        &app,
        "GET",
        "/api/staff/work/inbox?category=CONTACT_DUE",
        json!({}),
        &op,
    )
    .await;
    assert!(
        inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["entity"]["id"] == partner)
    );

    // --- DSP relationship and operations
    ok(
        &app,
        "POST",
        "/api/staff/work/relationships",
        json!({"kind": "DSP", "entity_kind": "dsp", "entity_id": "D-5", "owner_id": op.user}),
        &op,
    )
    .await;
    let list = ok(
        &app,
        "GET",
        "/api/staff/work/relationships?kind=DSP",
        json!({}),
        &op,
    )
    .await;
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    assert_eq!(list["items"][0]["contract"]["source"], "dsp_contract");
    assert_eq!(list["counts"]["ORG"], 1);
    ok(&app, "POST", "/api/staff/work/exceptions/reports",
        json!({"kind": "DSP_REJECTION", "severity": "HIGH", "title": "반려", "entity_kind": "dsp", "entity_id": "D-5"}), &op).await;
    let ops = ok(
        &app,
        "GET",
        "/api/staff/work/dsp-ops?dsp=D-5",
        json!({}),
        &op,
    )
    .await;
    assert_eq!(ops["window_days"], 30);
    let d5 = ops["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "D-5")
        .unwrap();
    assert_eq!(d5["exceptions"]["open"], 1);
    assert_eq!(d5["relationship"]["owner_id"], op.user.to_string());
    assert!(d5["metrics"]["total"].is_number());
    assert_eq!(d5["open_exceptions"].as_array().unwrap().len(), 1);
    assert!(d5["recent_failures"].is_array());
    let d1 = ops["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["code"] == "D-1")
        .unwrap();
    assert!(d1.get("recent_failures").is_none());

    // --- archive: ADMIN only; archived relationships are read-only
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{partner}/archive"),
        json!({}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{partner}/archive"),
        json!({}),
        &admin,
    )
    .await;
    let (s, r) = send(
        &app,
        "POST",
        &format!("/api/staff/work/relationships/{partner}/activities"),
        json!({"kind": "NOTE", "summary": "x"}),
        Some(&op),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r["error"]["code"], "RELATIONSHIP_ARCHIVED");
    let arch = ok(
        &app,
        "GET",
        "/api/staff/work/relationships?archived=true",
        json!({}),
        &op,
    )
    .await;
    assert_eq!(arch["items"].as_array().unwrap().len(), 1);

    // non-staff are out; everything is audited
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/work/relationships",
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/work/dsp-ops",
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE action LIKE 'work.relationship_%' OR action LIKE 'work.contact_%' OR action='work.activity_logged'")
        .fetch_one(&pool).await.unwrap();
    assert!(n >= 8, "{n}");
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}
