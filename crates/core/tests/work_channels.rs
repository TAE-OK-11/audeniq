//! Staff channels: default channels, messages as internal notes (mentions →
//! inbox, newest-first paging), unread counts and read marks, permissions for
//! renaming and archiving, no posting to an archived channel.
use axum::{Router, http::StatusCode};
use serde_json::{Value, json};
use sqlx::PgPool;

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

fn channel<'a>(list: &'a Value, id: &str) -> &'a Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap()
}

#[sqlx::test]
async fn channels_carry_team_conversation(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let kim = staff_user(&app, &pool, "REVIEWER").await;
    let tae = staff_user(&app, &pool, "OPERATOR").await;
    let artist = user(&app).await;

    // default channels exist for everyone
    let list = ok(&app, "GET", "/api/staff/work/channels", json!({}), &kim).await;
    let names: Vec<&str> = list["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"전체") && names.contains(&"배급"),
        "{names:?}"
    );
    let (s, _) = send(
        &app,
        "GET",
        "/api/staff/work/channels",
        json!({}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // any staff creates a channel; names are unique
    let c = ok(
        &app,
        "POST",
        "/api/staff/work/channels",
        json!({"name": "#멜론 장애", "topic": "SFTP 연결 거부 대응"}),
        &tae,
    )
    .await;
    let id = c["id"].as_str().unwrap().to_string();
    assert_eq!(c["name"], "멜론 장애");
    let (s, r) = send(
        &app,
        "POST",
        "/api/staff/work/channels",
        json!({"name": "멜론 장애"}),
        Some(&kim),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error"]["code"], "CHANNEL_EXISTS");

    // messages are notes: mention reaches the inbox and links back to the channel
    for i in 0..5 {
        ok(
            &app,
            "POST",
            "/api/staff/work/notes",
            json!({"target_kind": "channel", "target_id": id, "body": format!("업데이트 {i}")}),
            &tae,
        )
        .await;
    }
    ok(&app, "POST", "/api/staff/work/notes",
        json!({"target_kind": "channel", "target_id": id, "body": "@kim 재전송 확인 부탁해요", "mentions": [kim.user]}), &tae).await;
    let inbox = ok(
        &app,
        "GET",
        "/api/staff/work/inbox?category=MENTION",
        json!({}),
        &kim,
    )
    .await;
    assert_eq!(inbox["items"][0]["link"], format!("/admin/chat/{id}"));

    // unread for others, not for the author
    let list = ok(&app, "GET", "/api/staff/work/channels", json!({}), &kim).await;
    assert_eq!(channel(&list, &id)["unread"], 6);
    assert_eq!(channel(&list, &id)["mentions"], 1);
    let list = ok(&app, "GET", "/api/staff/work/channels", json!({}), &tae).await;
    assert_eq!(channel(&list, &id)["unread"], 0);
    let sum = ok(&app, "GET", "/api/staff/work/summary", json!({}), &kim).await;
    assert_eq!(sum["chat"]["unread"], 6);
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/channels/{id}/read"),
        json!({}),
        &kim,
    )
    .await;
    let list = ok(&app, "GET", "/api/staff/work/channels", json!({}), &kim).await;
    assert_eq!(channel(&list, &id)["unread"], 0);

    // newest-first paging, shown oldest-first
    let page = ok(
        &app,
        "GET",
        &format!("/api/staff/work/notes?target_kind=channel&target_id={id}&latest=4"),
        json!({}),
        &kim,
    )
    .await;
    let items = page["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(page["has_more"], true);
    assert_eq!(items[3]["body"], "@kim 재전송 확인 부탁해요");
    let before = items[0]["created_at"].as_str().unwrap();
    let older = ok(
        &app,
        "GET",
        &format!(
            "/api/staff/work/notes?target_kind=channel&target_id={id}&latest=4&before={}",
            before.replace('+', "%2B")
        ),
        json!({}),
        &kim,
    )
    .await;
    assert_eq!(older["items"].as_array().unwrap().len(), 2);
    assert_eq!(older["has_more"], false);
    assert_eq!(older["items"][0]["body"], "업데이트 0");

    // rename / archive: creator or ADMIN only
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/channels/{id}"),
        json!({"name": "x"}),
        Some(&kim),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let c = ok(
        &app,
        "POST",
        &format!("/api/staff/work/channels/{id}"),
        json!({"name": "멜론 전송 장애", "topic": "해결 중"}),
        &tae,
    )
    .await;
    assert_eq!(c["name"], "멜론 전송 장애");
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/channels/{id}/archive"),
        json!({}),
        &admin,
    )
    .await;
    let (s, r) = send(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "channel", "target_id": id, "body": "보관 후"}),
        Some(&tae),
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(r["error"]["code"], "CHANNEL_ARCHIVED");
    let arch = ok(
        &app,
        "GET",
        "/api/staff/work/channels?archived=true",
        json!({}),
        &kim,
    )
    .await;
    assert_eq!(arch["items"].as_array().unwrap().len(), 1);
    // a system channel is ADMIN's to manage
    let sys = ok(&app, "GET", "/api/staff/work/channels", json!({}), &kim).await["items"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/staff/work/channels/{sys}/archive"),
        json!({}),
        Some(&tae),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM operations.audit_events WHERE action LIKE 'work.channel_%'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 3);
}
