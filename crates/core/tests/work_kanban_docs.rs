//! Kanban boards and AUDENIQ Docs against the real database and API:
//! board / column permissions, personal-board isolation (IDOR), card moves and
//! ordering with optimistic concurrency, automatic cards (idempotent), and
//! documents — access levels (restricted documents are a 404 everywhere:
//! detail, tree, search, comments, embeds), validated block content, save
//! conflicts, versions and restore, tree moves (cycle / depth), review status.
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

async fn status(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    u: &User,
) -> (StatusCode, String) {
    let (s, v) = send(app, method, path, body, Some(u)).await;
    (s, v["error"]["code"].as_str().unwrap_or("").to_string())
}

fn board_by_key<'a>(list: &'a Value, key: &str) -> &'a Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["key"] == key)
        .unwrap_or_else(|| panic!("board {key}: {list}"))
}

fn column<'a>(view: &'a Value, name: &str) -> &'a Value {
    view["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("column {name}"))
}

/// Card ids of a column, top to bottom.
fn order(view: &Value, column_id: &Value) -> Vec<String> {
    let mut cards: Vec<&Value> = view["cards"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| &c["column_id"] == column_id)
        .collect();
    cards.sort_by(|a, b| {
        a["position"]
            .as_f64()
            .partial_cmp(&b["position"].as_f64())
            .unwrap()
    });
    cards
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect()
}

async fn card(app: &Router, u: &User, board: &Value, title: &str) -> Value {
    ok(
        app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": title, "type": "GENERAL", "priority": "NORMAL", "board_id": board["id"]}),
        u,
    )
    .await["task"]
        .clone()
}

#[sqlx::test]
async fn kanban_boards_columns_and_moves(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let op = staff_user(&app, &pool, "OPERATOR").await;
    let support = staff_user(&app, &pool, "SUPPORT").await;
    let artist = user(&app).await;

    // artists never reach boards
    let (s, _) = status(&app, "GET", "/api/staff/work/boards", json!({}), &artist).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // seeded team boards with the default columns
    let boards = ok(&app, "GET", "/api/staff/work/boards", json!({}), &op).await;
    for key in [
        "general",
        "distribution",
        "rights",
        "support",
        "engineering",
    ] {
        board_by_key(&boards, key);
    }
    let dist = board_by_key(&boards, "distribution").clone();
    assert_eq!(dist["access"]["write"], true);
    let view = ok(
        &app,
        "GET",
        &format!("/api/staff/work/boards/{}", dist["id"].as_str().unwrap()),
        json!({}),
        &op,
    )
    .await;
    let names: Vec<&str> = view["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "Inbox",
            "To Do",
            "In Progress",
            "Review",
            "Waiting",
            "Blocked",
            "Done"
        ]
    );

    // a team board is read-only for other teams; columns are ADMIN's
    let (s, _) = status(
        &app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": "다른 팀", "type": "GENERAL", "priority": "LOW", "board_id": dist["id"]}),
        &support,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = status(
        &app,
        "POST",
        &format!(
            "/api/staff/work/boards/{}/columns",
            dist["id"].as_str().unwrap()
        ),
        json!({"name": "QA", "status": "IN_REVIEW"}),
        &op,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = status(
        &app,
        "POST",
        "/api/staff/work/boards",
        json!({"name": "운영 보드", "kind": "OPERATIONS"}),
        &op,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // cards: new cards land in the first To Do column, at the end
    let a = card(&app, &op, &dist, "멜론 재전송").await;
    let b = card(&app, &op, &dist, "지니 반려 확인").await;
    let c = card(&app, &op, &dist, "벅스 메타 수정").await;
    assert_eq!(a["column_name"], "Inbox");
    assert_eq!(a["status"], "TODO");
    let view = ok(
        &app,
        "GET",
        &format!("/api/staff/work/boards/{}", dist["id"].as_str().unwrap()),
        json!({}),
        &op,
    )
    .await;
    let inbox = column(&view, "Inbox")["id"].clone();
    let ids = |v: &[&Value]| {
        v.iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&view, &inbox), ids(&[&a, &b, &c]));

    // reorder within the column: c goes to the top
    let moved = ok(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{}/move", c["id"].as_str().unwrap()),
        json!({"row_version": c["row_version"], "column_id": inbox, "after_id": a["id"]}),
        &op,
    )
    .await;
    assert_eq!(moved["redirected"], false);
    let view = ok(
        &app,
        "GET",
        &format!("/api/staff/work/boards/{}", dist["id"].as_str().unwrap()),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(order(&view, &inbox), ids(&[&c, &a, &b]));

    // across columns: status follows the column
    let progress = column(&view, "In Progress")["id"].clone();
    let m = ok(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{}/move", a["id"].as_str().unwrap()),
        json!({"row_version": a["row_version"], "column_id": progress}),
        &op,
    )
    .await;
    assert_eq!(m["card"]["status"], "IN_PROGRESS");
    assert_eq!(m["card"]["column_id"], progress);

    // a stale row_version is a conflict, never a silent overwrite
    let (s, _) = status(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{}/move", a["id"].as_str().unwrap()),
        json!({"row_version": a["row_version"], "column_id": inbox}),
        &op,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Review needs a reviewer: the move is refused and nothing changes
    let review = column(&view, "Review")["id"].clone();
    let (s, code) = status(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{}/move", a["id"].as_str().unwrap()),
        json!({"row_version": m["card"]["row_version"], "column_id": review}),
        &op,
    )
    .await;
    assert_eq!(
        (s, code.as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, "TASK_NO_REVIEWER")
    );

    // a column of another board is rejected
    let general = board_by_key(&boards, "general").clone();
    let gview = ok(
        &app,
        "GET",
        &format!("/api/staff/work/boards/{}", general["id"].as_str().unwrap()),
        json!({}),
        &op,
    )
    .await;
    let (s, code) = status(
        &app,
        "POST",
        &format!("/api/staff/work/tasks/{}/move", a["id"].as_str().unwrap()),
        json!({"row_version": m["card"]["row_version"], "column_id": column(&gview, "Done")["id"]}),
        &op,
    )
    .await;
    assert_eq!(
        (s, code.as_str()),
        (StatusCode::BAD_REQUEST, "COLUMN_NOT_ON_BOARD")
    );

    // the move is in the card's activity and the audit log
    let detail = ok(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{}", a["id"].as_str().unwrap()),
        json!({}),
        &op,
    )
    .await;
    let moves = detail["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["action"] == "MOVED")
        .count();
    assert_eq!(moves, 1, "{}", detail["events"]);
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE resource_id=$1 AND action='work.task_moved'")
        .bind(Uuid::parse_str(a["id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audited, 1);

    // filters: search and source
    let found = ok(
        &app,
        "GET",
        &format!(
            "/api/staff/work/boards/{}?q=%EC%A7%80%EB%8B%88",
            dist["id"].as_str().unwrap()
        ),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(
        ids(&found["cards"]
            .as_array()
            .unwrap()
            .iter()
            .collect::<Vec<_>>()),
        ids(&[&b])
    );
    let auto = ok(
        &app,
        "GET",
        &format!(
            "/api/staff/work/boards/{}?source=auto",
            dist["id"].as_str().unwrap()
        ),
        json!({}),
        &op,
    )
    .await;
    assert!(auto["cards"].as_array().unwrap().is_empty());

    // ADMIN manages columns; a non-empty column needs a destination
    let qa = ok(
        &app,
        "POST",
        &format!(
            "/api/staff/work/boards/{}/columns",
            dist["id"].as_str().unwrap()
        ),
        json!({"name": "QA", "status": "IN_PROGRESS"}),
        &admin,
    )
    .await;
    let qa_col = column(&qa, "QA")["id"].clone();
    let (s, _) = status(
        &app,
        "POST",
        &format!(
            "/api/staff/work/board-columns/{}/archive",
            progress.as_str().unwrap()
        ),
        json!({}),
        &admin,
    )
    .await;
    assert_eq!(s.as_u16() / 100, 4);
    ok(
        &app,
        "POST",
        &format!(
            "/api/staff/work/board-columns/{}/archive",
            progress.as_str().unwrap()
        ),
        json!({"move_to": qa_col}),
        &admin,
    )
    .await;
    let after = ok(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{}", a["id"].as_str().unwrap()),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(after["task"]["column_id"], qa_col);

    // personal boards: invisible to everyone else, by list, view, card and notes
    let mine = ok(
        &app,
        "POST",
        "/api/staff/work/boards",
        json!({"name": "내 할 일", "kind": "PERSONAL"}),
        &support,
    )
    .await;
    let mine_id = mine["board"]["id"]
        .as_str()
        .or(mine["id"].as_str())
        .unwrap()
        .to_string();
    let secret = ok(&app, "POST", "/api/staff/work/tasks",
        json!({"title": "개인 메모 카드", "type": "GENERAL", "priority": "LOW", "board_id": mine_id}), &support).await;
    let sid = secret["task"]["id"].as_str().unwrap();
    let boards = ok(&app, "GET", "/api/staff/work/boards", json!({}), &op).await;
    assert!(
        !boards["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["id"] == mine_id.as_str())
    );
    for path in [
        format!("/api/staff/work/boards/{mine_id}"),
        format!("/api/staff/work/tasks/{sid}"),
        format!("/api/staff/work/notes?target_kind=task&target_id={sid}"),
        format!("/api/staff/work/embeds?refs=task:{sid}"),
    ] {
        let (s, v) = send(&app, "GET", &path, json!({}), Some(&op)).await;
        if path.contains("embeds") {
            assert_eq!(v["items"][format!("task:{sid}")]["missing"], true, "{v}");
        } else {
            assert_eq!(s, StatusCode::NOT_FOUND, "{path}: {v}");
        }
    }
    let (s, _) = status(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "task", "target_id": sid, "body": "엿보기"}),
        &op,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let found = ok(
        &app,
        "GET",
        "/api/staff/work/entities/search?kind=task&q=%EA%B0%9C%EC%9D%B8",
        json!({}),
        &op,
    )
    .await;
    assert!(found["items"].as_array().unwrap().is_empty(), "{found}");
    let list = ok(&app, "GET", "/api/staff/work/tasks", json!({}), &op).await;
    assert!(
        !list["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["id"] == sid)
    );
    // its owner sees it
    ok(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{sid}"),
        json!({}),
        &support,
    )
    .await;
}

#[sqlx::test]
async fn automatic_cards_are_idempotent(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let op = staff_user(&app, &pool, "OPERATOR").await;
    let artist = user(&app).await;
    let inquiry = Uuid::new_v4();
    sqlx::query("INSERT INTO portal.inquiries(id,org_id,created_by,category,subject) VALUES($1,$2,$3,'RELEASE','발매일 변경 요청')")
        .bind(inquiry)
        .bind(artist.org)
        .bind(artist.user)
        .execute(&pool)
        .await
        .unwrap();

    let (s, _) = status(
        &app,
        "POST",
        "/api/staff/work/automation/run",
        json!({}),
        &op,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let first = ok(
        &app,
        "POST",
        "/api/staff/work/automation/run",
        json!({}),
        &admin,
    )
    .await;
    assert!(first["created"].as_u64().unwrap() >= 1, "{first}");
    let again = ok(
        &app,
        "POST",
        "/api/staff/work/automation/run",
        json!({}),
        &admin,
    )
    .await;
    assert_eq!(again["created"], 0);

    let rows: Vec<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT t.id, t.source, t.created_by FROM work.tasks t JOIN work.links l ON l.subject_id=t.id AND l.subject_kind='task'
          WHERE l.entity_kind='inquiry' AND l.entity_id=$1",
    )
    .bind(inquiry.to_string())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1, "support");
    assert_eq!(rows[0].2, None);
    let boards = ok(&app, "GET", "/api/staff/work/boards", json!({}), &admin).await;
    let sup = board_by_key(&boards, "support");
    let view = ok(
        &app,
        "GET",
        &format!(
            "/api/staff/work/boards/{}?source=auto",
            sup["id"].as_str().unwrap()
        ),
        json!({}),
        &admin,
    )
    .await;
    assert!(
        view["cards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == rows[0].0.to_string())
    );
    let audited: i64 = sqlx::query_scalar("SELECT count(*) FROM operations.audit_events WHERE resource_id=$1 AND action='work.task_auto_created'")
        .bind(rows[0].0)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audited, 1);

    // the ticket embed shows the inquiry with its card
    let e = ok(
        &app,
        "GET",
        &format!("/api/staff/work/embeds?refs=inquiry:{inquiry}"),
        json!({}),
        &op,
    )
    .await;
    let v = &e["items"][format!("inquiry:{inquiry}")];
    assert_eq!(v["subject"], "발매일 변경 요청");
    assert_eq!(v["task"]["id"], rows[0].0.to_string());
}

fn para(text: &str) -> Value {
    json!({"type": "paragraph", "content": [{"type": "text", "text": text}]})
}

#[sqlx::test]
async fn docs_access_content_versions_and_tree(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let admin = staff_user(&app, &pool, "ADMIN").await;
    let owner = staff_user(&app, &pool, "OPERATOR").await;
    let kim = staff_user(&app, &pool, "REVIEWER").await;
    let other = staff_user(&app, &pool, "SUPPORT").await;
    let artist = user(&app).await;
    let release = create_release(&app, &artist).await;

    let (s, _) = status(&app, "GET", "/api/staff/work/docs", json!({}), &artist).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // create from a template; staff-wide editable by default
    let templates = ok(
        &app,
        "GET",
        "/api/staff/work/doc-templates",
        json!({}),
        &owner,
    )
    .await;
    let incident = templates["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["key"] == "incident_report")
        .unwrap();
    let d = ok(
        &app,
        "POST",
        "/api/staff/work/docs",
        json!({"title": "멜론 전송 장애", "template_id": incident["id"]}),
        &owner,
    )
    .await;
    let id = d["doc"]["id"].as_str().unwrap().to_string();
    assert_eq!(d["doc"]["version"], 1);
    assert_eq!(d["doc"]["level"], "OWNER");
    assert!(d["doc"]["content"]["content"].as_array().unwrap().len() > 2);

    // save with an embed: the release lists the document, the version moves on
    let content = json!({"type": "doc", "content": [
        para("원인: SFTP 연결 거부"),
        {"type": "entityEmbed", "attrs": {"kind": "release", "id": release.to_string(), "view": "qc", "label": "연결 발매"}},
        {"type": "entityEmbed", "attrs": {"kind": "audit", "id": release.to_string(), "view": "summary", "label": "감사 기록"}},
        {"type": "taskList", "content": [{"type": "taskItem", "attrs": {"checked": false}, "content": [para("재전송")]}]},
    ]});
    let saved = ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}"),
        json!({"base_version": 1, "content": content}),
        &other,
    )
    .await;
    assert_eq!(saved["version"], 2);
    let detail = ok(
        &app,
        "GET",
        &format!("/api/staff/work/docs/{id}"),
        json!({}),
        &kim,
    )
    .await;
    assert!(
        detail["related"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == release.to_string() && r["origin"] == "embed"),
        "{}",
        detail["related"]
    );
    let by_release = ok(
        &app,
        "GET",
        &format!("/api/staff/work/docs/search?entity_kind=release&entity_id={release}"),
        json!({}),
        &kim,
    )
    .await;
    assert!(
        by_release["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == id.as_str())
    );

    // embeds: live release QC view for everyone; the audit trail for reviewers only
    let refs = format!("release:{release}:qc,release:{release}:distribution,audit:{release}");
    let e = ok(
        &app,
        "GET",
        &format!("/api/staff/work/embeds?refs={refs}"),
        json!({}),
        &kim,
    )
    .await;
    assert!(
        e["items"][format!("release:{release}:qc")]["qc"].is_object(),
        "{e}"
    );
    assert!(
        e["items"][format!("release:{release}:distribution")]["distribution"].is_array(),
        "{e}"
    );
    assert!(
        e["items"][format!("audit:{release}")]["events"].is_array(),
        "{e}"
    );
    let e = ok(
        &app,
        "GET",
        &format!("/api/staff/work/embeds?refs={refs}"),
        json!({}),
        &other,
    )
    .await;
    assert_eq!(e["items"][format!("audit:{release}")]["restricted"], true);
    assert!(e["items"][format!("release:{release}:qc")]["title"].is_string());

    // a stale save is a conflict
    let (s, _) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}"),
        json!({"base_version": 1, "title": "덮어쓰기"}),
        &kim,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // content is validated block JSON, never HTML or script links
    for bad in [
        json!({"type": "doc", "content": [{"type": "script", "content": []}]}),
        json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "x",
            "marks": [{"type": "link", "attrs": {"href": "javascript:alert(1)"}}]}]}]}),
        json!({"type": "doc", "content": [{"type": "paragraph", "attrs": {"onclick": "x"}, "content": []}]}),
        json!({"type": "doc", "content": [{"type": "entityEmbed", "attrs": {"kind": "user_secret", "id": "1"}}]}),
    ] {
        let (s, _) = status(
            &app,
            "POST",
            &format!("/api/staff/work/docs/{id}"),
            json!({"base_version": 2, "content": bad}),
            &owner,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }

    // full-text search finds the body
    let found = ok(
        &app,
        "GET",
        "/api/staff/work/docs/search?q=SFTP",
        json!({}),
        &kim,
    )
    .await;
    assert!(
        found["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == id.as_str())
    );

    // versions: the pre-save state was snapshotted; restore brings it back as a new version
    let vs = ok(
        &app,
        "GET",
        &format!("/api/staff/work/docs/{id}/versions"),
        json!({}),
        &owner,
    )
    .await;
    let first = vs["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["version"].as_i64().unwrap())
        .min()
        .unwrap();
    let restored = ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/restore-version"),
        json!({"version": first, "base_version": 2}),
        &owner,
    )
    .await;
    assert_eq!(restored["doc"]["version"], 3);
    assert!(!restored["doc"]["content"].to_string().contains("SFTP"));

    // restricted document: only the owner, ADMIN and grants; everyone else gets a 404
    ok(&app, "POST", &format!("/api/staff/work/docs/{id}/permissions"), json!({"staff_access": null,
        "grants": [{"principal_kind": "USER", "principal_id": kim.user.to_string(), "level": "VIEWER"}]}), &owner).await;
    for path in [
        format!("/api/staff/work/docs/{id}"),
        format!("/api/staff/work/docs/{id}/versions"),
        format!("/api/staff/work/notes?target_kind=doc&target_id={id}"),
    ] {
        let (s, _) = status(&app, "GET", &path, json!({}), &other).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
    let (s, _) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}"),
        json!({"base_version": 3, "title": "x"}),
        &other,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = status(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "doc", "target_id": id, "body": "몰래"}),
        &other,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let tree = ok(&app, "GET", "/api/staff/work/docs", json!({}), &other).await;
    assert!(
        !tree["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == id.as_str())
    );
    let found = ok(
        &app,
        "GET",
        "/api/staff/work/docs/search?q=%EB%A9%9C%EB%A1%A0",
        json!({}),
        &other,
    )
    .await;
    assert!(
        !found["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["id"] == id.as_str())
    );
    let e = ok(
        &app,
        "GET",
        &format!("/api/staff/work/embeds?refs=doc:{id}"),
        json!({}),
        &other,
    )
    .await;
    assert_eq!(e["items"][format!("doc:{id}")]["missing"], true);
    // entity search (link pickers, @doc): hidden for others, found by a granted viewer
    let path = format!("/api/staff/work/entities/search?kind=doc&q={id}");
    let found = ok(&app, "GET", &path, json!({}), &other).await;
    assert!(found["items"].as_array().unwrap().is_empty(), "{found}");
    let found = ok(&app, "GET", &path, json!({}), &kim).await;
    assert_eq!(found["items"].as_array().unwrap().len(), 1);
    // a viewer reads but cannot edit; ADMIN can
    ok(
        &app,
        "GET",
        &format!("/api/staff/work/docs/{id}"),
        json!({}),
        &kim,
    )
    .await;
    let (s, _) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}"),
        json!({"base_version": 3, "title": "x"}),
        &kim,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    ok(
        &app,
        "GET",
        &format!("/api/staff/work/docs/{id}"),
        json!({}),
        &admin,
    )
    .await;

    // comments: threads on a document the commenter can see
    let c1 = ok(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "doc", "target_id": id, "body": "원인 확인 필요",
        "anchor": {"block": "b-1", "quote": "SFTP"}}),
        &kim,
    )
    .await;
    let reply = ok(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "doc", "target_id": id, "body": "확인했어요",
        "parent_id": c1["id"]}),
        &owner,
    )
    .await;
    assert_eq!(reply["parent_id"], c1["id"]);
    let (s, code) = status(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "doc", "target_id": id, "body": "x",
        "parent_id": reply["id"]}),
        &owner,
    )
    .await;
    assert_eq!(
        (s, code.as_str()),
        (StatusCode::BAD_REQUEST, "NOTE_PARENT_INVALID")
    );
    let (s, _) = status(
        &app,
        "POST",
        &format!(
            "/api/staff/work/notes/{}/resolve",
            c1["id"].as_str().unwrap()
        ),
        json!({"resolved": true}),
        &other,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // a mention of someone who cannot open the document does not reach their inbox
    ok(
        &app,
        "POST",
        "/api/staff/work/notes",
        json!({"target_kind": "doc", "target_id": id, "body": "@other", "mentions": [other.user]}),
        &owner,
    )
    .await;
    let inbox = ok(
        &app,
        "GET",
        "/api/staff/work/inbox?category=MENTION",
        json!({}),
        &other,
    )
    .await;
    assert!(inbox["items"].as_array().unwrap().is_empty(), "{inbox}");

    // children inherit restriction; tree depth and cycles are enforced
    let child = ok(
        &app,
        "POST",
        "/api/staff/work/docs",
        json!({"title": "타임라인", "parent_id": id}),
        &owner,
    )
    .await;
    let cid = child["doc"]["id"].as_str().unwrap().to_string();
    assert_eq!(child["doc"]["staff_access"], Value::Null);
    let (s, _) = status(
        &app,
        "GET",
        &format!("/api/staff/work/docs/{cid}"),
        json!({}),
        &other,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, code) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/move"),
        json!({"parent_id": cid}),
        &owner,
    )
    .await;
    assert_eq!(
        (s, code.as_str()),
        (StatusCode::BAD_REQUEST, "DOC_MOVE_CYCLE")
    );
    let mut parent = cid.clone();
    let mut code = String::new();
    for n in 0..5 {
        let (s, v) = send(
            &app,
            "POST",
            "/api/staff/work/docs",
            json!({"title": format!("깊이 {n}"), "parent_id": parent}),
            Some(&owner),
        )
        .await;
        if s != StatusCode::OK {
            code = v["error"]["code"].as_str().unwrap_or("").into();
            break;
        }
        parent = v["doc"]["id"].as_str().unwrap().to_string();
    }
    assert_eq!(code, "DOC_TOO_DEEP");
    let kids = ok(
        &app,
        "GET",
        &format!("/api/staff/work/docs?parent={id}"),
        json!({}),
        &owner,
    )
    .await;
    assert_eq!(kids["items"].as_array().unwrap().len(), 1);

    // review: no self-approval (except ADMIN); rejection needs a reason
    ok(&app, "POST", &format!("/api/staff/work/docs/{id}/permissions"), json!({"staff_access": "EDITOR",
        "grants": [{"principal_kind": "USER", "principal_id": kim.user.to_string(), "level": "MANAGER"}]}), &owner).await;
    // an editor cannot approve
    ok(
        &app,
        "GET",
        &format!("/api/staff/work/docs/{id}"),
        json!({}),
        &other,
    )
    .await;
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/status"),
        json!({"action": "REQUEST_REVIEW"}),
        &owner,
    )
    .await;
    let (s, code) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/status"),
        json!({"action": "APPROVE"}),
        &owner,
    )
    .await;
    assert_eq!(
        (s, code.as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, "DOC_SELF_APPROVAL")
    );
    let (s, _) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/status"),
        json!({"action": "APPROVE"}),
        &other,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, code) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/status"),
        json!({"action": "REJECT"}),
        &kim,
    )
    .await;
    assert_eq!(
        (s, code.as_str()),
        (StatusCode::BAD_REQUEST, "REASON_REQUIRED")
    );
    let approved = ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/status"),
        json!({"action": "APPROVE"}),
        &kim,
    )
    .await;
    assert_eq!(approved["doc"]["status"], "APPROVED");
    assert_eq!(
        approved["doc"]["review"]["approved_by"],
        kim.user.to_string()
    );

    // archive / restore / delete (tombstone, archived only, owner or ADMIN)
    let (s, _) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/delete"),
        json!({}),
        &owner,
    )
    .await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/archive"),
        json!({}),
        &owner,
    )
    .await;
    let archived: bool =
        sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM work.docs WHERE id=$1")
            .bind(Uuid::parse_str(&cid).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(archived, "children are archived with their parent");
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/restore"),
        json!({}),
        &owner,
    )
    .await;
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/archive"),
        json!({}),
        &owner,
    )
    .await;
    let (s, _) = status(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/delete"),
        json!({}),
        &kim,
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    ok(
        &app,
        "POST",
        &format!("/api/staff/work/docs/{id}/delete"),
        json!({}),
        &owner,
    )
    .await;
    let (s, _) = status(
        &app,
        "GET",
        &format!("/api/staff/work/docs/{id}"),
        json!({}),
        &owner,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // every change is audited
    let actions: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT action FROM operations.audit_events WHERE resource_id=$1 ORDER BY 1",
    )
    .bind(Uuid::parse_str(&id).unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    for a in [
        "work.doc_created",
        "work.doc_permissions",
        "work.doc_archived",
        "work.doc_deleted",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a} in {actions:?}");
    }
}

#[sqlx::test]
async fn card_creates_linked_doc(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let op = staff_user(&app, &pool, "OPERATOR").await;
    let t = ok(
        &app,
        "POST",
        "/api/staff/work/tasks",
        json!({"title": "정산 불일치 조사", "type": "GENERAL", "priority": "HIGH"}),
        &op,
    )
    .await;
    let tid = t["task"]["id"].as_str().unwrap();
    let d = ok(
        &app,
        "POST",
        "/api/staff/work/docs",
        json!({"title": "조사 노트",
        "link": {"entity_kind": "task", "entity_id": tid}}),
        &op,
    )
    .await;
    let did = d["doc"]["id"].as_str().unwrap();
    assert!(
        d["cards"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == tid),
        "{d}"
    );
    let task = ok(
        &app,
        "GET",
        &format!("/api/staff/work/tasks/{tid}"),
        json!({}),
        &op,
    )
    .await;
    assert!(
        task["task"]["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["kind"] == "doc" && l["id"] == did),
        "{task}"
    );
    // card embed inside the document
    let e = ok(
        &app,
        "GET",
        &format!("/api/staff/work/embeds?refs=task:{tid},doc:{did}"),
        json!({}),
        &op,
    )
    .await;
    assert_eq!(
        e["items"][format!("task:{tid}")]["title"],
        "정산 불일치 조사"
    );
    assert_eq!(e["items"][format!("task:{tid}")]["column"], "Inbox");
    assert_eq!(e["items"][format!("doc:{did}")]["title"], "조사 노트");
}
