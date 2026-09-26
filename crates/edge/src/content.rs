//! Studio notices and events served from D1 (binding `CONTENT_DB`) at the
//! edge, without a round trip to the private API.
//!
//! - `GET /api/notices`, `GET /api/notices/{id}`, `GET /api/events`,
//!   `GET /api/events/{id}`: published, not deleted rows. Event status
//!   (upcoming / ongoing / ended) is computed from KST dates on read.
//! - `GET|POST /api/content/{notices|events}`, `PUT|DELETE .../{id}`: content
//!   administration with `Authorization: Bearer <CONTENT_ADMIN_TOKEN>`
//!   (Wrangler secret). The admin list includes scheduled and removed rows.
//!   Deletes are soft (`deleted_at`); saving a removed row publishes it again.
//!   Same contract as the static Studio Worker (web/studio/worker.js).
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use worker::{D1Database, Env, Method, Request, Response, Result, wasm_bindgen::JsValue};

const NOTICE_COLUMNS: &str = "id, title, body, pinned, published_at, updated_at";
const EVENT_COLUMNS: &str =
    "id, title, summary, body, place, starts_on, ends_on, link_url, published_at, updated_at";

#[derive(Debug, PartialEq)]
pub enum Route<'a> {
    List(&'a str),
    Get(&'a str, &'a str),
    AdminList(&'a str),
    Create(&'a str),
    Update(&'a str, &'a str),
    Delete(&'a str, &'a str),
}

/// Which content route (if any) a request path/method is.
pub fn route<'a>(method: &Method, path: &'a str) -> Option<Route<'a>> {
    let (admin, rest) = match path.strip_prefix("/api/content/") {
        Some(r) => (true, r),
        None => (false, path.strip_prefix("/api/")?),
    };
    let mut parts = rest.split('/');
    let table = match parts.next()? {
        "notices" => "notices",
        "events" => "events",
        _ => return None,
    };
    let id = parts.next().filter(|s| !s.is_empty());
    if parts.next().is_some() || id.is_some_and(|i| !valid_id(i)) {
        return None;
    }
    match (admin, method, id) {
        (false, Method::Get | Method::Head, None) => Some(Route::List(table)),
        (false, Method::Get | Method::Head, Some(id)) => Some(Route::Get(table, id)),
        (true, Method::Get | Method::Head, None) => Some(Route::AdminList(table)),
        (true, Method::Post, None) => Some(Route::Create(table)),
        (true, Method::Put, Some(id)) => Some(Route::Update(table, id)),
        (true, Method::Delete, Some(id)) => Some(Route::Delete(table, id)),
        _ => None,
    }
}

pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && (1..=12).contains(&d[5..7].parse::<u8>().unwrap_or(0))
        && (1..=31).contains(&d[8..10].parse::<u8>().unwrap_or(0))
}

fn valid_timestamp(t: &str) -> bool {
    // 2026-09-26T00:00:00Z
    t.len() == 20 && valid_date(&t[..10]) && t.as_bytes()[10] == b'T' && t.ends_with('Z')
}

fn clean(s: &str, max: usize, multiline: bool) -> std::result::Result<String, &'static str> {
    let t = s.trim();
    if t.chars().count() > max {
        return Err("TOO_LONG");
    }
    let bad = t.chars().any(|c| {
        (c.is_control() && !(multiline && (c == '\n' || c == '\t')))
            || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
    });
    if bad {
        Err("TEXT_INVALID_CHARACTERS")
    } else {
        Ok(t.to_string())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoticeInput {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
    pub published_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventInput {
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub body: String,
    #[serde(default)]
    pub place: String,
    pub starts_on: String,
    pub ends_on: Option<String>,
    pub link_url: Option<String>,
    pub published_at: String,
}

/// Validated row ready for binding (column order matches the INSERT).
#[derive(Debug, PartialEq)]
pub struct Row {
    pub id: Option<String>,
    pub values: Vec<Option<String>>,
}

pub fn validate_notice(i: NoticeInput) -> std::result::Result<Row, &'static str> {
    let title = clean(&i.title, 200, false)?;
    if title.is_empty() {
        return Err("TITLE_REQUIRED");
    }
    let body = clean(&i.body, 20_000, true)?;
    if !valid_timestamp(&i.published_at) {
        return Err("PUBLISHED_AT_INVALID");
    }
    if i.id.as_deref().is_some_and(|id| !valid_id(id)) {
        return Err("ID_INVALID");
    }
    Ok(Row {
        id: i.id,
        values: vec![
            Some(title),
            Some(body),
            Some(if i.pinned { "1" } else { "0" }.into()),
            Some(i.published_at),
        ],
    })
}

pub fn validate_event(i: EventInput) -> std::result::Result<Row, &'static str> {
    let title = clean(&i.title, 200, false)?;
    if title.is_empty() {
        return Err("TITLE_REQUIRED");
    }
    let summary = clean(&i.summary, 300, false)?;
    let body = clean(&i.body, 20_000, true)?;
    let place = clean(&i.place, 120, false)?;
    if !valid_date(&i.starts_on)
        || i.ends_on
            .as_deref()
            .is_some_and(|e| !valid_date(e) || e < i.starts_on.as_str())
    {
        return Err("DATES_INVALID");
    }
    if !valid_timestamp(&i.published_at) {
        return Err("PUBLISHED_AT_INVALID");
    }
    if let Some(u) = i.link_url.as_deref().filter(|u| !u.is_empty())
        && (!u.starts_with("https://") || u.len() > 500 || u.chars().any(char::is_whitespace))
    {
        return Err("LINK_URL_INVALID");
    }
    if i.id.as_deref().is_some_and(|id| !valid_id(id)) {
        return Err("ID_INVALID");
    }
    Ok(Row {
        id: i.id,
        values: vec![
            Some(title),
            Some(summary),
            Some(body),
            Some(place),
            Some(i.starts_on),
            i.ends_on.filter(|e| !e.is_empty()),
            i.link_url.filter(|u| !u.is_empty()),
            Some(i.published_at),
        ],
    })
}

/// upcoming / ongoing / ended for a KST calendar day.
pub fn event_status(starts_on: &str, ends_on: Option<&str>, today: &str) -> &'static str {
    let end = ends_on.unwrap_or(starts_on);
    if today < starts_on {
        "upcoming"
    } else if today <= end {
        "ongoing"
    } else {
        "ended"
    }
}

/// Constant-time bearer token comparison.
pub fn bearer_ok(header: Option<&str>, secret: &str) -> bool {
    let Some(token) = header.and_then(|h| h.strip_prefix("Bearer ")) else {
        return false;
    };
    if secret.len() < 32 || token.len() != secret.len() {
        return false;
    }
    token
        .bytes()
        .zip(secret.bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn now_utc() -> String {
    let d = js_sys_date();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        d.get_utc_full_year(),
        d.get_utc_month() + 1,
        d.get_utc_date(),
        d.get_utc_hours(),
        d.get_utc_minutes(),
        d.get_utc_seconds()
    )
}

fn today_kst() -> String {
    let d = worker::js_sys::Date::new(&JsValue::from_f64(
        worker::js_sys::Date::now() + 9.0 * 3_600_000.0,
    ));
    format!(
        "{:04}-{:02}-{:02}",
        d.get_utc_full_year(),
        d.get_utc_month() + 1,
        d.get_utc_date()
    )
}

fn js_sys_date() -> worker::js_sys::Date {
    worker::js_sys::Date::new_0()
}

fn json_response(v: &Value, status: u16, cache: &str) -> Result<Response> {
    let mut r = Response::from_json(v)?.with_status(status);
    r.headers_mut().set("Cache-Control", cache)?;
    r.headers_mut().set("X-Content-Type-Options", "nosniff")?;
    Ok(r)
}

fn error(status: u16, code: &str) -> Result<Response> {
    json_response(
        &json!({"error":{"code":code,"message":code}}),
        status,
        "no-store",
    )
}

#[derive(Serialize, Deserialize)]
struct EventRow {
    id: String,
    title: String,
    summary: String,
    body: String,
    place: String,
    starts_on: String,
    ends_on: Option<String>,
    link_url: Option<String>,
    published_at: String,
    updated_at: String,
}

fn with_status(e: EventRow, today: &str) -> Value {
    let status = event_status(&e.starts_on, e.ends_on.as_deref(), today);
    let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
    v["status"] = json!(status);
    v
}

pub async fn handle(mut req: Request, env: &Env, r: Route<'_>) -> Result<Response> {
    let db: D1Database = match env.d1("CONTENT_DB") {
        Ok(db) => db,
        Err(_) => return error(503, "CONTENT_UNAVAILABLE"),
    };
    let now = now_utc();
    match r {
        Route::List("notices") => {
            let rows = db
                .prepare(format!(
                    "SELECT {NOTICE_COLUMNS} FROM notices WHERE deleted_at IS NULL AND published_at <= ?1 ORDER BY pinned DESC, published_at DESC LIMIT 100"
                ))
                .bind(&[now.into()])?
                .all()
                .await?
                .results::<Value>()?;
            let items: Vec<Value> = rows
                .into_iter()
                .map(|mut v| {
                    v["pinned"] = json!(v["pinned"].as_f64() == Some(1.0));
                    v
                })
                .collect();
            json_response(&json!({ "items": items }), 200, "public, max-age=60")
        }
        Route::Get("notices", id) => {
            let row = db
                .prepare(format!("SELECT {NOTICE_COLUMNS} FROM notices WHERE id = ?1 AND deleted_at IS NULL AND published_at <= ?2"))
                .bind(&[id.into(), now.into()])?
                .first::<Value>(None)
                .await?;
            match row {
                Some(mut v) => {
                    v["pinned"] = json!(v["pinned"].as_f64() == Some(1.0));
                    json_response(&v, 200, "public, max-age=60")
                }
                None => error(404, "NOT_FOUND"),
            }
        }
        Route::List(_) => {
            let today = today_kst();
            let rows = db
                .prepare(format!(
                    "SELECT {EVENT_COLUMNS} FROM events WHERE deleted_at IS NULL AND published_at <= ?1 ORDER BY starts_on DESC LIMIT 100"
                ))
                .bind(&[now.into()])?
                .all()
                .await?
                .results::<EventRow>()?;
            let items: Vec<Value> = rows.into_iter().map(|e| with_status(e, &today)).collect();
            json_response(&json!({ "items": items }), 200, "public, max-age=60")
        }
        Route::Get(_, id) => {
            let row = db
                .prepare(format!("SELECT {EVENT_COLUMNS} FROM events WHERE id = ?1 AND deleted_at IS NULL AND published_at <= ?2"))
                .bind(&[id.into(), now.into()])?
                .first::<EventRow>(None)
                .await?;
            match row {
                Some(e) => json_response(&with_status(e, &today_kst()), 200, "public, max-age=60"),
                None => error(404, "NOT_FOUND"),
            }
        }
        admin => {
            let secret = env
                .secret("CONTENT_ADMIN_TOKEN")
                .map(|s| s.to_string())
                .unwrap_or_default();
            if !bearer_ok(req.headers().get("authorization")?.as_deref(), &secret) {
                return error(401, "UNAUTHENTICATED");
            }
            if let Route::AdminList(table) = admin {
                let today = today_kst();
                let rows = db
                    .prepare(format!(
                        "SELECT {}, created_at, deleted_at FROM {table} ORDER BY {} LIMIT 500",
                        if table == "notices" {
                            NOTICE_COLUMNS
                        } else {
                            EVENT_COLUMNS
                        },
                        if table == "notices" {
                            "pinned DESC, published_at DESC"
                        } else {
                            "starts_on DESC"
                        }
                    ))
                    .all()
                    .await?
                    .results::<Value>()?;
                let items: Vec<Value> = rows
                    .into_iter()
                    .map(|mut v| {
                        if table == "notices" {
                            v["pinned"] = json!(v["pinned"].as_f64() == Some(1.0));
                        } else {
                            let status = event_status(
                                v["starts_on"].as_str().unwrap_or_default(),
                                v["ends_on"].as_str(),
                                &today,
                            );
                            v["status"] = json!(status);
                        }
                        v
                    })
                    .collect();
                return json_response(&json!({ "items": items, "now": now }), 200, "no-store");
            }
            let (table, id, body) = match admin {
                Route::Create(t) => (t, None, Some(req.text().await?)),
                Route::Update(t, id) => (t, Some(id.to_string()), Some(req.text().await?)),
                Route::Delete(t, id) => (t, Some(id.to_string()), None),
                _ => return error(404, "NOT_FOUND"),
            };
            let Some(body) = body else {
                let meta = db
                    .prepare(format!("UPDATE {table} SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL"))
                    .bind(&[now.into(), id.clone().unwrap_or_default().into()])?
                    .run()
                    .await?
                    .meta()?;
                let changed = meta.and_then(|m| m.changes).unwrap_or(0);
                return if changed == 1 {
                    json_response(&json!({"id":id,"deleted":true}), 200, "no-store")
                } else {
                    error(404, "NOT_FOUND")
                };
            };
            if body.len() > 64 * 1024 {
                return error(413, "PAYLOAD_TOO_LARGE");
            }
            let row = if table == "notices" {
                serde_json::from_str::<NoticeInput>(&body)
                    .map_err(|_| "INVALID_INPUT")
                    .and_then(validate_notice)
            } else {
                serde_json::from_str::<EventInput>(&body)
                    .map_err(|_| "INVALID_INPUT")
                    .and_then(validate_event)
            };
            let row = match row {
                Ok(r) => r,
                Err(code) => return error(400, code),
            };
            let columns: &[&str] = if table == "notices" {
                &["title", "body", "pinned", "published_at"]
            } else {
                &[
                    "title",
                    "summary",
                    "body",
                    "place",
                    "starts_on",
                    "ends_on",
                    "link_url",
                    "published_at",
                ]
            };
            let mut values: Vec<JsValue> = row
                .values
                .into_iter()
                .map(|v| v.map(JsValue::from).unwrap_or(JsValue::NULL))
                .collect();
            let (sql, row_id) = match id {
                None => {
                    let new_id = row
                        .id
                        .clone()
                        .unwrap_or_else(|| format!("c-{}", worker::js_sys::Date::now() as u64));
                    let placeholders: Vec<String> =
                        (1..=columns.len() + 1).map(|n| format!("?{n}")).collect();
                    values.insert(0, JsValue::from(new_id.clone()));
                    (
                        format!(
                            "INSERT INTO {table} (id, {}) VALUES ({})",
                            columns.join(", "),
                            placeholders.join(", ")
                        ),
                        new_id,
                    )
                }
                Some(id) => {
                    let sets: Vec<String> = columns
                        .iter()
                        .enumerate()
                        .map(|(n, c)| format!("{c} = ?{}", n + 1))
                        .collect();
                    values.push(JsValue::from(now.clone()));
                    values.push(JsValue::from(id.clone()));
                    (
                        format!(
                            "UPDATE {table} SET {}, updated_at = ?{}, deleted_at = NULL WHERE id = ?{}",
                            sets.join(", "),
                            columns.len() + 1,
                            columns.len() + 2
                        ),
                        id,
                    )
                }
            };
            let result = db.prepare(sql).bind(&values)?.run().await;
            match result {
                Ok(r) if r.meta()?.and_then(|m| m.changes).unwrap_or(0) == 1 => {
                    json_response(&json!({"id":row_id}), 200, "no-store")
                }
                Ok(_) => error(404, "NOT_FOUND"),
                Err(_) => error(409, "CONFLICT"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(
            route(&Method::Get, "/api/notices"),
            Some(Route::List("notices"))
        );
        assert_eq!(
            route(&Method::Get, "/api/events/launch-promotion"),
            Some(Route::Get("events", "launch-promotion"))
        );
        assert_eq!(
            route(&Method::Post, "/api/content/notices"),
            Some(Route::Create("notices"))
        );
        assert_eq!(
            route(&Method::Put, "/api/content/events/a-1"),
            Some(Route::Update("events", "a-1"))
        );
        assert_eq!(
            route(&Method::Delete, "/api/content/notices/a"),
            Some(Route::Delete("notices", "a"))
        );
        // writes only under /api/content, reads only under /api
        assert_eq!(route(&Method::Post, "/api/notices"), None);
        assert_eq!(
            route(&Method::Get, "/api/content/notices"),
            Some(Route::AdminList("notices"))
        );
        assert_eq!(route(&Method::Put, "/api/content/notices"), None);
        assert_eq!(route(&Method::Get, "/api/notices/Bad_ID"), None);
        assert_eq!(route(&Method::Get, "/api/notices/a/b"), None);
        assert_eq!(route(&Method::Get, "/api/orgs/x/releases"), None);
    }

    #[test]
    fn validation() {
        let n = |title: &str, published: &str| NoticeInput {
            id: Some("launch".into()),
            title: title.into(),
            body: "본문\n둘째 줄".into(),
            pinned: true,
            published_at: published.into(),
        };
        assert!(validate_notice(n("공지", "2026-09-26T00:00:00Z")).is_ok());
        assert_eq!(
            validate_notice(n("", "2026-09-26T00:00:00Z")),
            Err("TITLE_REQUIRED")
        );
        assert_eq!(
            validate_notice(n("공지", "2026-09-26")),
            Err("PUBLISHED_AT_INVALID")
        );
        assert_eq!(
            validate_notice(n("a\u{202e}b", "2026-09-26T00:00:00Z")),
            Err("TEXT_INVALID_CHARACTERS")
        );
        let e = |starts: &str, ends: Option<&str>, link: Option<&str>| EventInput {
            id: None,
            title: "워크숍".into(),
            summary: "".into(),
            body: "내용".into(),
            place: "온라인".into(),
            starts_on: starts.into(),
            ends_on: ends.map(Into::into),
            link_url: link.map(Into::into),
            published_at: "2026-09-01T00:00:00Z".into(),
        };
        assert!(
            validate_event(e(
                "2026-09-20",
                Some("2026-09-27"),
                Some("https://audeniq.com/e")
            ))
            .is_ok()
        );
        assert_eq!(
            validate_event(e("2026-09-20", Some("2026-09-19"), None)),
            Err("DATES_INVALID")
        );
        assert_eq!(
            validate_event(e("2026-13-01", None, None)),
            Err("DATES_INVALID")
        );
        assert_eq!(
            validate_event(e("2026-09-20", None, Some("javascript:alert(1)"))),
            Err("LINK_URL_INVALID")
        );
    }

    #[test]
    fn statuses_and_tokens() {
        assert_eq!(
            event_status("2026-10-01", Some("2026-10-31"), "2026-09-26"),
            "upcoming"
        );
        assert_eq!(
            event_status("2026-09-20", Some("2026-09-27"), "2026-09-26"),
            "ongoing"
        );
        assert_eq!(event_status("2026-09-26", None, "2026-09-26"), "ongoing");
        assert_eq!(
            event_status("2026-08-01", Some("2026-08-31"), "2026-09-26"),
            "ended"
        );
        let secret = "s".repeat(40);
        assert!(bearer_ok(Some(&format!("Bearer {secret}")), &secret));
        assert!(!bearer_ok(Some("Bearer short"), &secret));
        assert!(!bearer_ok(None, &secret));
        assert!(!bearer_ok(
            Some(&format!("Bearer {}", "s".repeat(10))),
            "s".repeat(10).as_str()
        ));
    }
}
