//! 공지·이벤트·서버 점검 API(예전 Cloudflare Worker + D1)와 사이트별 규칙(관리자·설문 화면).
//! 점검 일정 캐시는 프로세스 전역이라 한 테스트 함수에서 순서대로 확인한다.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

mod support;
use support::*;

const ADMIN_ORIGIN: &str = "https://admin.audeniq.test";

async fn raw(
    app: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Value,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("x-audeniq-service", SECRET)
        .header("content-type", "application/json");
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let r = app
        .clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[sqlx::test]
async fn content_maintenance_and_site_rules(pool: PgPool) {
    // SAFETY: 이 테스트 바이너리에는 테스트가 하나뿐이고, 앱을 만들기 전에 설정한다
    unsafe { std::env::set_var("ADMIN_ORIGIN", ADMIN_ORIGIN) };
    audeniq_core::site_content::reset_maintenance_cache();
    let (app, _) = app(pool.clone()).await;
    let admin = user(&app).await;
    sqlx::query("INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES($1,'ADMIN','test-operator')")
        .bind(admin.user)
        .execute(&pool)
        .await
        .unwrap();
    let artist = user(&app).await;

    // ---- 공지: 공개 목록, 관리 권한, 예약 글, 수정·삭제
    let (s, v) = call(&app, "GET", "/api/notices", json!(null), None).await;
    assert_eq!((s, v), (StatusCode::OK, json!({"items": []})));
    let notice = json!({"id":"n-welcome","title":"환영합니다","body":"첫 공지","pinned":true});
    let (s, _) = call(
        &app,
        "POST",
        "/api/content/notices",
        notice.clone(),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = call(
        &app,
        "POST",
        "/api/content/notices",
        notice.clone(),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["pinned"], true);
    let (s, v) = call(&app, "POST", "/api/content/notices", notice, Some(&admin)).await;
    assert_eq!(
        (s, v["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("ID_TAKEN"))
    );
    let (s, v) = call(
        &app,
        "POST",
        "/api/content/notices",
        json!({"title":""}),
        Some(&admin),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].clone()),
        (StatusCode::BAD_REQUEST, json!("TITLE_REQUIRED"))
    );
    let later = json!({"title":"예약","published_at":"2099-01-01T00:00:00Z"});
    let (s, v) = call(&app, "POST", "/api/content/notices", later, Some(&admin)).await;
    assert_eq!(s, StatusCode::CREATED);
    assert!(v["id"].as_str().unwrap().starts_with("n-"));
    let (_, v) = call(&app, "GET", "/api/notices", json!(null), None).await;
    assert_eq!(v["items"].as_array().unwrap().len(), 1, "예약 글은 공개 전");
    let (_, v) = call(
        &app,
        "GET",
        "/api/content/notices",
        json!(null),
        Some(&admin),
    )
    .await;
    assert_eq!(
        v["items"].as_array().unwrap().len(),
        2,
        "관리 목록은 예약 글 포함"
    );
    let (s, v) = call(
        &app,
        "PUT",
        "/api/content/notices/n-welcome",
        json!({"title":"바뀐 제목"}),
        Some(&admin),
    )
    .await;
    assert_eq!(
        (s, v["title"].clone()),
        (StatusCode::OK, json!("바뀐 제목"))
    );
    let (s, _) = call(
        &app,
        "DELETE",
        "/api/content/notices/n-welcome",
        json!(null),
        Some(&admin),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&app, "GET", "/api/notices/n-welcome", json!(null), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // 수정하면 복구된다
    call(
        &app,
        "PUT",
        "/api/content/notices/n-welcome",
        json!({"title":"복구"}),
        Some(&admin),
    )
    .await;
    let (s, _) = call(&app, "GET", "/api/notices/n-welcome", json!(null), None).await;
    assert_eq!(s, StatusCode::OK);

    // ---- 이벤트: 진행 상태
    let ev = json!({"id":"e-1","title":"쇼케이스","starts_on":"2020-01-01","ends_on":"2020-01-02","link_url":"https://a.example/x"});
    let (s, v) = call(&app, "POST", "/api/content/events", ev, Some(&admin)).await;
    assert_eq!(
        (s, v["status"].clone()),
        (StatusCode::CREATED, json!("ended")),
        "{v}"
    );
    let (_, v) = call(&app, "GET", "/api/events/e-1", json!(null), None).await;
    assert_eq!(v["starts_on"], "2020-01-01");

    // ---- 관리자 사이트: 경로 허용 목록과 Origin
    let site = ("x-audeniq-site", "admin");
    let (s, _) = raw(
        &app,
        "GET",
        "/api/orgs",
        &[site, ("cookie", &admin.cookie)],
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, v) = raw(
        &app,
        "GET",
        "/api/staff/me",
        &[site, ("cookie", &admin.cookie)],
        json!(null),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let write = |origin: &'static str| {
        let (app, admin) = (app.clone(), admin.clone());
        async move {
            raw(
                &app,
                "POST",
                "/api/content/notices",
                &[
                    site,
                    ("origin", origin),
                    ("cookie", &admin.cookie),
                    ("x-csrf-token", &admin.csrf),
                ],
                json!({"title":"관리자 화면에서"}),
            )
            .await
        }
    };
    assert_eq!(write(ADMIN_ORIGIN).await.0, StatusCode::CREATED);
    // 같은 사이트의 스튜디오 화면이 관리자 주소로 쓰기를 보내면 막는다
    assert_eq!(write(ORIGIN).await.0, StatusCode::FORBIDDEN);

    // ---- 서버 점검: 일반 API는 503, 세션·콘텐츠·관리자 사이트는 유지
    let (s, v) = call(&app, "GET", "/api/status", json!(null), None).await;
    assert_eq!(
        (s, v["maintenance"]["active"].clone()),
        (StatusCode::OK, Value::Null)
    );
    let m = json!({"id":"m-now","title":"DB 작업","starts_at":"2020-01-01T00:00:00Z","ends_at":"2099-01-01T00:00:00Z","kind":"emergency","end_unknown":true});
    let (s, v) = call(&app, "POST", "/api/content/maintenance", m, Some(&admin)).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let (_, v) = call(&app, "GET", "/api/status", json!(null), None).await;
    assert_eq!(v["maintenance"]["active"]["id"], "m-now");
    let (s, v) = call(
        &app,
        "POST",
        "/api/orgs",
        json!({"name":"x","kind":"LABEL"}),
        Some(&artist),
    )
    .await;
    assert_eq!(
        (s, v["error"]["code"].clone()),
        (StatusCode::SERVICE_UNAVAILABLE, json!("MAINTENANCE"))
    );
    assert_eq!(v["maintenance"]["title"], "DB 작업");
    let (s, _) = call(&app, "GET", "/api/me", json!(null), Some(&artist)).await;
    assert_eq!(s, StatusCode::OK);
    // AI clients get their own protocol's error shape, not a transport failure.
    let (s, v) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[],
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_release","arguments":{}}}),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["error"]["code"], -32000);
    assert!(v["error"]["message"].as_str().unwrap().contains("DB 작업"));
    // The request's id comes back, and discovery still works.
    let (s, v) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[],
        json!({"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"list_releases","arguments":{}}}),
    )
    .await;
    assert_eq!(
        (s, v["id"].clone()),
        (StatusCode::SERVICE_UNAVAILABLE, json!(42))
    );
    let (s, _) = raw(
        &app,
        "POST",
        "/api/mcp",
        &[],
        json!({"jsonrpc":"2.0","id":43,"method":"tools/call","params":{"name":"get_service_status","arguments":{}}}),
    )
    .await;
    assert_ne!(
        s,
        StatusCode::SERVICE_UNAVAILABLE,
        "the status tool explains the maintenance"
    );
    let (s, v) = raw(&app, "POST", "/api/oauth/token", &[], json!(null)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["error"], "temporarily_unavailable");
    let (s, _) = call(&app, "GET", "/api/notices", json!(null), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = raw(
        &app,
        "GET",
        "/api/staff/releases",
        &[site, ("cookie", &admin.cookie)],
        json!(null),
    )
    .await;
    assert_ne!(s, StatusCode::SERVICE_UNAVAILABLE);
    // 점검을 지우면 바로 풀린다 (이 프로세스의 캐시를 비운다)
    call(
        &app,
        "DELETE",
        "/api/content/maintenance/m-now",
        json!(null),
        Some(&admin),
    )
    .await;
    let (s, _) = call(&app, "GET", "/api/orgs", json!(null), Some(&artist)).await;
    assert_eq!(s, StatusCode::OK);

    // ---- 운영자 CLI (audeniq-admin maintenance): API·로그인 없이 DB로 시작·연장·종료
    use audeniq_core::{error::Error, site_content as sc};
    let started = sc::cli_start(&pool, "ops-kim", Some(30), None, None)
        .await
        .unwrap();
    assert_eq!(
        (started["kind"].clone(), started["end_unknown"].clone()),
        (json!("emergency"), json!(false))
    );
    assert_eq!(started["body"], sc::EMERGENCY_BODY);
    // 다른 프로세스(CLI)가 쓴 행: 도는 API는 STATUS_TTL 뒤에 본다
    sc::reset_maintenance_cache();
    let (s, v) = call(&app, "GET", "/api/orgs", json!(null), Some(&artist)).await;
    assert_eq!(
        (s, v["error"]["code"].clone()),
        (StatusCode::SERVICE_UNAVAILABLE, json!("MAINTENANCE"))
    );
    assert!(matches!(
        sc::cli_start(&pool, "ops-kim", None, None, None).await,
        Err(Error::PolicyGate("MAINTENANCE_ALREADY_ACTIVE"))
    ));
    let before =
        chrono::DateTime::parse_from_rfc3339(started["ends_at"].as_str().unwrap()).unwrap();
    let extended = sc::cli_extend(&pool, "ops-kim", 30).await.unwrap();
    let after = chrono::DateTime::parse_from_rfc3339(
        extended["maintenance"]["active"]["ends_at"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!((after - before).num_minutes(), 30);
    let st = sc::cli_status(&pool).await.unwrap();
    assert_eq!(st["maintenance"]["active"]["id"], started["id"]);
    let stopped = sc::cli_stop(&pool, "ops-kim").await.unwrap();
    assert_eq!(stopped["stopped"], json!([started["id"]]));
    sc::reset_maintenance_cache();
    let (s, _) = call(&app, "GET", "/api/orgs", json!(null), Some(&artist)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(matches!(
        sc::cli_stop(&pool, "ops-kim").await,
        Err(Error::PolicyGate("MAINTENANCE_NOT_ACTIVE"))
    ));
    assert!(matches!(
        sc::cli_start(&pool, "ops-kim", Some(0), None, None).await,
        Err(Error::InvalidCode("MINUTES_INVALID"))
    ));

    // 예약 점검: 예고 배너로 보이고, 취소하면 사라진다. 진행 중인 점검은 cancel이 아니라 stop
    let planned = sc::cli_create(
        &pool,
        "ops-kim",
        "maintenance",
        json!({"title":"DB 업그레이드","starts_at":sc::cli_time("2099-01-01 02:00").unwrap(),
               "ends_at":"2099-01-01T19:00:00Z","kind":"scheduled"}),
    )
    .await
    .unwrap();
    assert_eq!(planned["starts_at"], "2098-12-31T17:00:00Z");
    let listed = sc::cli_list(&pool, "maintenance").await.unwrap();
    assert!(
        listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == planned["id"])
    );
    sc::cli_remove(
        &pool,
        "ops-kim",
        "maintenance",
        planned["id"].as_str().unwrap(),
    )
    .await
    .unwrap();
    let running = sc::cli_start(&pool, "ops-kim", None, None, None)
        .await
        .unwrap();
    assert_eq!(running["end_unknown"], true);
    assert!(matches!(
        sc::cli_remove(
            &pool,
            "ops-kim",
            "maintenance",
            running["id"].as_str().unwrap()
        )
        .await,
        Err(Error::PolicyGate("MAINTENANCE_ACTIVE_USE_STOP"))
    ));

    // 긴급 공지: 점검 중에도 공개 목록에 보이고, 내리면 사라진다
    let notice = sc::cli_create(
        &pool,
        "ops-kim",
        "notices",
        json!({"title":"접속 장애 안내","body":"복구 중이에요.","pinned":true}),
    )
    .await
    .unwrap();
    sc::reset_maintenance_cache();
    let (s, v) = call(&app, "GET", "/api/notices", json!(null), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v.to_string().contains("접속 장애 안내"), "{v}");
    assert!(matches!(
        sc::cli_create(&pool, "ops-kim", "notices", json!({"title":"","body":"x"})).await,
        Err(Error::InvalidCode("TITLE_REQUIRED"))
    ));
    assert!(matches!(
        sc::cli_create(&pool, "ops-kim", "events", json!({})).await,
        Err(Error::InvalidCode("CONTENT_KIND_INVALID"))
    ));
    sc::cli_remove(&pool, "ops-kim", "notices", notice["id"].as_str().unwrap())
        .await
        .unwrap();
    let (_, v) = call(&app, "GET", "/api/notices", json!(null), None).await;
    assert!(!v.to_string().contains("접속 장애 안내"), "{v}");
    sc::cli_stop(&pool, "ops-kim").await.unwrap();

    let audited: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM operations.audit_events WHERE actor_service='audeniq-admin:ops-kim' ORDER BY occurred_at, action",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        audited,
        [
            "maintenance.started",
            "maintenance.extended",
            "maintenance.stopped",
            "content.maintenance.created",
            "content.maintenance.removed",
            "maintenance.started",
            "content.notices.created",
            "content.notices.removed",
            "maintenance.stopped",
        ]
    );
}
