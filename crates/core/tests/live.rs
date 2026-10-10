//! Live updates (SSE): a staff reply reaches the artist's open stream within a
//! few seconds (notification + inquiry change, linking straight to the
//! thread), a customer message reaches the staff stream marked as from the
//! customer, and nobody else can open either stream.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::time::Duration;
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

struct Live {
    body: Body,
    buf: String,
}

async fn open(app: &Router, path: &str, u: &User) -> (StatusCode, Option<Live>) {
    let req = Request::builder()
        .method("GET")
        .uri(path)
        .header("x-audeniq-service", SECRET)
        .header("origin", ORIGIN)
        .header("accept", "text/event-stream")
        .header("cookie", &u.cookie)
        .body(Body::empty())
        .unwrap();
    let r = app.clone().oneshot(req).await.unwrap();
    let status = r.status();
    if status != StatusCode::OK {
        return (status, None);
    }
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    (
        status,
        Some(Live {
            body: r.into_body(),
            buf: String::new(),
        }),
    )
}

impl Live {
    /// Next named event (skipping keep-alive comments), or None after `wait`.
    async fn next(&mut self, wait: Duration) -> Option<(String, Value)> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(end) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..end + 2).collect();
                let mut name = String::new();
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        name = v.trim().to_string();
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim());
                    }
                }
                if name.is_empty() {
                    continue;
                }
                return Some((name, serde_json::from_str(&data).unwrap_or(Value::Null)));
            }
            let left = deadline.checked_duration_since(tokio::time::Instant::now())?;
            let frame = tokio::time::timeout(left, self.body.frame())
                .await
                .ok()??
                .ok()?;
            if let Ok(bytes) = frame.into_data() {
                self.buf.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
    }
    async fn expect(&mut self, name: &str) -> Value {
        loop {
            let (n, d) = self
                .next(Duration::from_secs(10))
                .await
                .unwrap_or_else(|| panic!("no `{name}` event"));
            if n == name {
                return d;
            }
        }
    }
}

#[sqlx::test]
async fn staff_reply_reaches_the_artist_and_customer_message_reaches_staff(pool: PgPool) {
    let (app, _) = app(pool.clone()).await;
    let artist = user(&app).await;
    let outsider = user(&app).await;
    let support = staff_user(&app, &pool, "SUPPORT").await;

    let (s, q) = send(&app, "POST", &format!("/api/orgs/{}/inquiries", artist.org),
        json!({"category": "RELEASE", "release_id": null, "subject": "심사 일정 문의", "body": "언제 끝나나요?"}), Some(&artist)).await;
    assert_eq!(s, StatusCode::OK, "{q}");
    let inquiry: Uuid = sqlx::query_scalar("SELECT id FROM portal.inquiries WHERE org_id=$1")
        .bind(artist.org)
        .fetch_one(&pool)
        .await
        .unwrap();

    // Only members open a workspace stream; only staff open the staff stream.
    let (s, _) = open(&app, &format!("/api/orgs/{}/live", artist.org), &outsider).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = open(&app, "/api/staff/live", &artist).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (_, artist_live) = open(&app, &format!("/api/orgs/{}/live", artist.org), &artist).await;
    let mut artist_live = artist_live.unwrap();
    artist_live.expect("ready").await;
    let (_, staff_live) = open(&app, "/api/staff/live", &support).await;
    let mut staff_live = staff_live.unwrap();
    assert_eq!(staff_live.expect("ready").await["open"], 1);

    // Staff answers → the artist hears about it at once.
    let (s, r) = send(
        &app,
        "POST",
        &format!("/api/staff/inquiries/{inquiry}/reply"),
        json!({"body": "영업일 1~2일 안에 결과가 나와요."}),
        Some(&support),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let n = artist_live.expect("notifications").await;
    assert_eq!(n["new"], true);
    assert_eq!(n["unread"], 1);
    let i = artist_live.expect("inquiries").await;
    assert_eq!(i["inquiry_id"], inquiry.to_string());
    assert_eq!(i["staff"], true);
    // the notification opens this thread
    let link: String = sqlx::query_scalar(
        "SELECT link FROM portal.notifications WHERE org_id=$1 AND kind='INQUIRY'",
    )
    .bind(artist.org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(link, format!("/inquiries?open={inquiry}"));
    // the stream carries no customer content (subject/body stay behind the normal API)
    assert!(i.get("subject").is_none() && i.get("body").is_none());

    // The artist writes back → staff sees a customer message.
    staff_live.expect("inquiries").await; // the staff reply itself
    let (s, _) = send(
        &app,
        "POST",
        &format!("/api/orgs/{}/inquiries/{inquiry}/messages", artist.org),
        json!({"body": "감사합니다. 한 가지 더 여쭤봐도 될까요?"}),
        Some(&artist),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let e = staff_live.expect("inquiries").await;
    assert_eq!(e["customer"], true);
    assert_eq!(e["inquiry_id"], inquiry.to_string());
    assert_eq!(e["subject"], "심사 일정 문의");
}
