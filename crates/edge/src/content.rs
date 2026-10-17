//! Studio notices, events and maintenance windows served from D1 (binding
//! `CONTENT_DB`) at the edge, without a round trip to the private API.
//!
//! - `GET /api/notices`, `GET /api/notices/{id}`, `GET /api/events`,
//!   `GET /api/events/{id}`: published, not deleted rows. Event status
//!   (upcoming / ongoing / ended) is computed from KST dates on read.
//! - `GET /api/status`: the maintenance window in progress and the next one
//!   starting within 72 hours (Studio maintenance screen and banner). The
//!   `MAINTENANCE_MODE` emergency switch (Wrangler secret or var) forces an
//!   active emergency window without D1.
//! - `GET|POST /api/content/{notices|events|maintenance}`, `PUT|DELETE
//!   .../{id}`: content administration by an active ADMIN session (CSRF on
//!   writes, checked by the private API) or `Authorization: Bearer
//!   <CONTENT_ADMIN_TOKEN>` for emergency scripts. The admin list includes
//!   scheduled and removed rows. Deletes are soft (`deleted_at`); saving a
//!   removed row publishes it again.
//!   Same contract as the static Studio Worker (web/studio/worker.ts).
use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use worker::{
    D1Database, Env, Headers, Method, Request, RequestInit, Response, Result, console_error,
    wasm_bindgen::JsValue,
};

const NOTICE_COLUMNS: &str = "id, title, body, pinned, published_at, updated_at";
const EVENT_COLUMNS: &str =
    "id, title, summary, body, place, starts_on, ends_on, link_url, published_at, updated_at";
const MAINTENANCE_COLUMNS: &str =
    "id, title, body, starts_at, ends_at, kind, end_unknown, published_at, updated_at";
/// The Studio banner announces a window from 72 hours before it starts.
const NOTICE_AHEAD_SECS: i64 = 72 * 3600;
/// Maintenance rows are remembered per isolate for a few seconds: every tab
/// polls `/api/status` and every proxied API call checks for maintenance.
/// Active/upcoming is always recomputed from the current time.
const STATUS_TTL_MS: f64 = 10_000.0;

#[derive(Debug, PartialEq)]
pub enum Route<'a> {
    List(&'a str),
    Get(&'a str, &'a str),
    AdminList(&'a str),
    Create(&'a str),
    Update(&'a str, &'a str),
    Delete(&'a str, &'a str),
    /// `GET /api/status`: maintenance in progress and announced.
    Status,
}

/// Which content route (if any) a request path/method is.
pub fn route<'a>(method: &Method, path: &'a str) -> Option<Route<'a>> {
    let (admin, rest) = match path.strip_prefix("/api/content/") {
        Some(r) => (true, r),
        None => (false, path.strip_prefix("/api/")?),
    };
    if !admin && rest == "status" {
        return matches!(method, Method::Get | Method::Head).then_some(Route::Status);
    }
    let mut parts = rest.split('/');
    let table = match parts.next()? {
        "notices" => "notices",
        "events" => "events",
        // Maintenance windows are public only through /api/status.
        "maintenance" if admin => "maintenance",
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceInput {
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub starts_at: String,
    pub ends_at: String,
    /// Empty or missing: announce from now.
    #[serde(default)]
    pub published_at: Option<String>,
    /// `scheduled` (default) or `emergency` (started from the admin screen).
    #[serde(default)]
    pub kind: Option<String>,
    /// The end time is not known; `ends_at` is a placeholder.
    #[serde(default)]
    pub end_unknown: bool,
}

pub fn validate_maintenance(
    i: MaintenanceInput,
    now: &str,
) -> std::result::Result<Row, &'static str> {
    let kind = match i.kind.as_deref() {
        None | Some("scheduled") => "scheduled",
        Some("emergency") => "emergency",
        Some(_) => return Err("INVALID_INPUT"),
    };
    let title = clean(&i.title, 200, false)?;
    if title.is_empty() {
        return Err("TITLE_REQUIRED");
    }
    let body = clean(&i.body, 2_000, true)?;
    if !valid_timestamp(&i.starts_at) || !valid_timestamp(&i.ends_at) || i.ends_at <= i.starts_at {
        return Err("DATES_INVALID");
    }
    let published_at = match i.published_at.filter(|p| !p.is_empty()) {
        None => now.to_string(),
        Some(p) if valid_timestamp(&p) => p,
        Some(_) => return Err("PUBLISHED_AT_INVALID"),
    };
    if i.id.as_deref().is_some_and(|id| !valid_id(id)) {
        return Err("ID_INVALID");
    }
    Ok(Row {
        id: i.id,
        values: vec![
            Some(title),
            Some(body),
            Some(i.starts_at),
            Some(i.ends_at),
            Some(kind.into()),
            Some(if i.end_unknown { "1" } else { "0" }.into()),
            Some(published_at),
        ],
    })
}

/// Seconds since the Unix epoch for `YYYY-MM-DDTHH:MM:SSZ`.
fn epoch_secs(t: &str) -> Option<i64> {
    if !valid_timestamp(t) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    // Days from civil (proleptic Gregorian).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// D1 row → response shape (`kind` normalized, `end_unknown` boolean).
fn present_maintenance(mut v: Value) -> Value {
    let emergency = v["kind"].as_str() == Some("emergency");
    v["kind"] = json!(if emergency { "emergency" } else { "scheduled" });
    let unknown = v["end_unknown"].as_f64() == Some(1.0) || v["end_unknown"] == json!(true);
    v["end_unknown"] = json!(unknown);
    v
}

/// The window in progress (earliest start) and the next announced one
/// starting within 72 hours. Rows must already be presented.
pub fn pick_status(rows: &[Value], now: &str) -> Value {
    let field = |r: &Value, k: &str| r[k].as_str().unwrap_or_default().to_string();
    let mut live: Vec<&Value> = rows
        .iter()
        .filter(|r| field(r, "published_at").as_str() <= now && field(r, "ends_at").as_str() > now)
        .collect();
    live.sort_by_key(|r| field(r, "starts_at"));
    let active = live
        .iter()
        .find(|r| field(r, "starts_at").as_str() <= now)
        .map(|r| (*r).clone());
    let now_secs = epoch_secs(now).unwrap_or(0);
    let upcoming = live
        .iter()
        .find(|r| {
            let starts = field(r, "starts_at");
            starts.as_str() > now
                && epoch_secs(&starts).is_some_and(|s| s - now_secs <= NOTICE_AHEAD_SECS)
        })
        .map(|r| (*r).clone());
    json!({ "now": now, "maintenance": { "active": active, "upcoming": upcoming } })
}

/// Emergency switch: `MAINTENANCE_MODE` set to anything but empty/0/off/
/// false/no starts an emergency window now. Optional `MAINTENANCE_TITLE`,
/// `MAINTENANCE_MESSAGE`, `MAINTENANCE_UNTIL` (UTC timestamp).
pub fn env_maintenance(
    mode: &str,
    title: &str,
    message: &str,
    until: &str,
    now: &str,
) -> Option<Value> {
    let mode = mode.trim().to_ascii_lowercase();
    if matches!(mode.as_str(), "" | "0" | "off" | "false" | "no") {
        return None;
    }
    let until = until.trim();
    let known = valid_timestamp(until) && until > now;
    let title = title.trim();
    Some(json!({
        "id": "emergency-switch",
        "title": if title.is_empty() { "긴급 서버 점검" } else { title },
        "body": message.trim(),
        "starts_at": now,
        "ends_at": if known { until } else { "9999-12-31T00:00:00Z" },
        "kind": "emergency",
        "end_unknown": !known,
        "published_at": now,
        "updated_at": now,
    }))
}

/// A Wrangler secret or plain var; missing is empty.
fn env_text(env: &Env, name: &str) -> String {
    env.secret(name)
        .map(|s| s.to_string())
        .or_else(|_| env.var(name).map(|v| v.to_string()))
        .unwrap_or_default()
}

fn forced_maintenance(env: &Env, now: &str) -> Option<Value> {
    env_maintenance(
        &env_text(env, "MAINTENANCE_MODE"),
        &env_text(env, "MAINTENANCE_TITLE"),
        &env_text(env, "MAINTENANCE_MESSAGE"),
        &env_text(env, "MAINTENANCE_UNTIL"),
        now,
    )
}

thread_local! {
    static MAINTENANCE_ROWS: RefCell<Option<(f64, Vec<Value>)>> = const { RefCell::new(None) };
}

fn reset_maintenance_cache() {
    MAINTENANCE_ROWS.with(|c| *c.borrow_mut() = None);
}

/// Windows not yet over. A missing binding or table (before the migration)
/// reads as none, so Studio keeps working.
async fn maintenance_rows(env: &Env, now: &str) -> Vec<Value> {
    let at = worker::js_sys::Date::now();
    if let Some(rows) = MAINTENANCE_ROWS.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(t, _)| at - t < STATUS_TTL_MS)
            .map(|(_, rows)| rows.clone())
    }) {
        return rows;
    }
    let read = async {
        env.d1("CONTENT_DB")?
            .prepare(format!(
                "SELECT {MAINTENANCE_COLUMNS} FROM maintenance WHERE deleted_at IS NULL AND ends_at > ?1 ORDER BY starts_at LIMIT 20"
            ))
            .bind(&[now.into()])?
            .all()
            .await?
            .results::<Value>()
    };
    let rows: Vec<Value> = match read.await {
        Ok(rows) => rows.into_iter().map(present_maintenance).collect(),
        Err(e) => {
            console_error!("status error: {e}");
            Vec::new()
        }
    };
    MAINTENANCE_ROWS.with(|c| *c.borrow_mut() = Some((at, rows.clone())));
    rows
}

/// Emergency switch + D1 schedule.
pub async fn service_status(env: &Env, now: &str) -> Value {
    let forced = forced_maintenance(env, now);
    let mut status = pick_status(&maintenance_rows(env, now).await, now);
    if let Some(f) = forced {
        status["maintenance"]["active"] = f;
    }
    status
}

/// The maintenance window in progress, if any. The emergency switch skips D1.
pub async fn active_maintenance(env: &Env) -> Option<Value> {
    let now = now_utc();
    if let Some(f) = forced_maintenance(env, &now) {
        return Some(f);
    }
    let status = service_status(env, &now).await;
    Some(status["maintenance"]["active"].clone()).filter(|a| !a.is_null())
}

/// While maintenance runs, the edge answers private API calls itself (no
/// writes reach the database); Studio switches to its maintenance screen.
pub fn maintenance_response(window: &Value) -> Result<Response> {
    let mut r = Response::from_json(&json!({
        "error": { "code": "MAINTENANCE", "message": window["title"] },
        "maintenance": window,
    }))?
    .with_status(503);
    r.headers_mut().set("Cache-Control", "no-store")?;
    r.headers_mut().set("Retry-After", "60")?;
    r.headers_mut().set("X-Content-Type-Options", "nosniff")?;
    Ok(r)
}

/// Session checks and sign-in stay available during maintenance so ADMIN can
/// manage content (and end the window); everything else is blocked.
pub fn allowed_during_maintenance(method: &Method, path: &str) -> bool {
    match method {
        Method::Get => matches!(
            path,
            "/api/me"
                | "/api/staff/me"
                | "/api/auth/oauth/providers"
                | "/api/auth/oauth/google/callback"
        ),
        Method::Post => matches!(
            path,
            "/api/auth/login"
                | "/api/auth/logout"
                | "/api/auth/csrf"
                | "/api/auth/passkey/options"
                | "/api/auth/passkey/login"
                | "/api/auth/oauth/google/start"
                | "/api/auth/oauth/apple/start"
                | "/api/auth/oauth/apple/callback"
        ),
        _ => false,
    }
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

async fn authorize_content(req: &Request, env: &Env) -> Result<Option<Response>> {
    if let Some(header) = req.headers().get("authorization")? {
        let secret = env
            .secret("CONTENT_ADMIN_TOKEN")
            .map(|s| s.to_string())
            .unwrap_or_default();
        return if bearer_ok(Some(&header), &secret) {
            Ok(None)
        } else {
            Ok(Some(error(401, "UNAUTHENTICATED")?))
        };
    }
    let read = matches!(req.method(), Method::Get | Method::Head);
    let origin = req.url()?.origin().ascii_serialization();
    if !read
        && (req.headers().get("origin")?.as_deref() != Some(&origin)
            || req
                .headers()
                .get("sec-fetch-site")?
                .is_some_and(|s| s != "same-origin"))
    {
        return Ok(Some(error(403, "FORBIDDEN")?));
    }
    let headers = Headers::new();
    for key in ["cookie", "x-csrf-token"] {
        if let Some(value) = req.headers().get(key)? {
            headers.set(key, &value)?;
        }
    }
    headers.set("origin", &origin)?;
    headers.set("sec-fetch-site", "same-origin")?;
    headers.set(
        "x-audeniq-service",
        &env.secret("EDGE_SERVICE_SECRET")?.to_string(),
    )?;
    let check = Request::new_with_init(
        "http://audeniq-api.internal/api/staff/content-access",
        RequestInit::new()
            .with_method(if read { Method::Get } else { Method::Post })
            .with_headers(headers),
    )?;
    let response = env.service("PRIVATE_API")?.fetch_request(check).await?;
    if response.status_code() == 200 {
        Ok(None)
    } else {
        Ok(Some(response))
    }
}

pub async fn handle(mut req: Request, env: &Env, r: Route<'_>) -> Result<Response> {
    if r == Route::Status {
        return json_response(&service_status(env, &now_utc()).await, 200, "no-store");
    }
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
            if let Some(denied) = authorize_content(&req, env).await? {
                return Ok(denied);
            }
            if let Route::AdminList(table) = admin {
                let today = today_kst();
                let (columns, order) = match table {
                    "notices" => (NOTICE_COLUMNS, "pinned DESC, published_at DESC"),
                    "maintenance" => (MAINTENANCE_COLUMNS, "starts_at DESC"),
                    _ => (EVENT_COLUMNS, "starts_on DESC"),
                };
                let rows = db
                    .prepare(format!(
                        "SELECT {columns}, created_at, deleted_at FROM {table} ORDER BY {order} LIMIT 500"
                    ))
                    .all()
                    .await?
                    .results::<Value>()?;
                let items: Vec<Value> = rows
                    .into_iter()
                    .map(|mut v| {
                        match table {
                            "notices" => {
                                v["pinned"] = json!(v["pinned"].as_f64() == Some(1.0));
                            }
                            "maintenance" => v = present_maintenance(v),
                            _ => {
                                let status = event_status(
                                    v["starts_on"].as_str().unwrap_or_default(),
                                    v["ends_on"].as_str(),
                                    &today,
                                );
                                v["status"] = json!(status);
                            }
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
            if table == "maintenance" {
                reset_maintenance_cache();
            }
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
            let row = match table {
                "notices" => serde_json::from_str::<NoticeInput>(&body)
                    .map_err(|_| "INVALID_INPUT")
                    .and_then(validate_notice),
                "maintenance" => serde_json::from_str::<MaintenanceInput>(&body)
                    .map_err(|_| "INVALID_INPUT")
                    .and_then(|i| validate_maintenance(i, &now)),
                _ => serde_json::from_str::<EventInput>(&body)
                    .map_err(|_| "INVALID_INPUT")
                    .and_then(validate_event),
            };
            let row = match row {
                Ok(r) => r,
                Err(code) => return error(400, code),
            };
            let columns: &[&str] = match table {
                "notices" => &["title", "body", "pinned", "published_at"],
                "maintenance" => &[
                    "title",
                    "body",
                    "starts_at",
                    "ends_at",
                    "kind",
                    "end_unknown",
                    "published_at",
                ],
                _ => &[
                    "title",
                    "summary",
                    "body",
                    "place",
                    "starts_on",
                    "ends_on",
                    "link_url",
                    "published_at",
                ],
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
        // maintenance: public only through /api/status, managed under /api/content
        assert_eq!(route(&Method::Get, "/api/status"), Some(Route::Status));
        assert_eq!(route(&Method::Head, "/api/status"), Some(Route::Status));
        assert_eq!(route(&Method::Post, "/api/status"), None);
        assert_eq!(route(&Method::Get, "/api/content/status"), None);
        assert_eq!(route(&Method::Get, "/api/maintenance"), None);
        assert_eq!(route(&Method::Get, "/api/maintenance/m-1"), None);
        assert_eq!(
            route(&Method::Get, "/api/content/maintenance"),
            Some(Route::AdminList("maintenance"))
        );
        assert_eq!(
            route(&Method::Post, "/api/content/maintenance"),
            Some(Route::Create("maintenance"))
        );
        assert_eq!(
            route(&Method::Put, "/api/content/maintenance/m-1"),
            Some(Route::Update("maintenance", "m-1"))
        );
        assert_eq!(
            route(&Method::Delete, "/api/content/maintenance/m-1"),
            Some(Route::Delete("maintenance", "m-1"))
        );
    }

    #[test]
    fn maintenance_validation() {
        let now = "2026-10-10T00:00:00Z";
        let m = |starts: &str, ends: &str, kind: Option<&str>| MaintenanceInput {
            id: None,
            title: "DB 점검".into(),
            body: "발매 접수가 잠시 멈춰요.".into(),
            starts_at: starts.into(),
            ends_at: ends.into(),
            published_at: None,
            kind: kind.map(Into::into),
            end_unknown: false,
        };
        let row =
            validate_maintenance(m("2026-10-11T17:00:00Z", "2026-10-11T19:00:00Z", None), now)
                .unwrap();
        assert_eq!(row.values[4].as_deref(), Some("scheduled"));
        assert_eq!(row.values[5].as_deref(), Some("0"));
        assert_eq!(
            row.values[6].as_deref(),
            Some(now),
            "empty published_at = now"
        );
        let emergency = MaintenanceInput {
            end_unknown: true,
            published_at: Some("2026-10-09T00:00:00Z".into()),
            ..m(
                "2026-10-10T00:00:00Z",
                "9999-12-31T00:00:00Z",
                Some("emergency"),
            )
        };
        let row = validate_maintenance(emergency, now).unwrap();
        assert_eq!(row.values[4].as_deref(), Some("emergency"));
        assert_eq!(row.values[5].as_deref(), Some("1"));
        assert_eq!(row.values[6].as_deref(), Some("2026-10-09T00:00:00Z"));
        assert_eq!(
            validate_maintenance(m("2026-10-11T19:00:00Z", "2026-10-11T17:00:00Z", None), now),
            Err("DATES_INVALID")
        );
        assert_eq!(
            validate_maintenance(m("2026-10-11", "2026-10-12", None), now),
            Err("DATES_INVALID")
        );
        assert_eq!(
            validate_maintenance(
                m(
                    "2026-10-11T17:00:00Z",
                    "2026-10-11T19:00:00Z",
                    Some("weekly")
                ),
                now
            ),
            Err("INVALID_INPUT")
        );
        assert!(
            serde_json::from_str::<MaintenanceInput>(
                r#"{"title":"a","starts_at":"2026-10-11T17:00:00Z","ends_at":"2026-10-11T19:00:00Z","extra":1}"#
            )
            .is_err()
        );
    }

    #[test]
    fn maintenance_status() {
        assert_eq!(epoch_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_secs("2026-10-10T00:00:00Z"), Some(1_791_590_400));
        assert_eq!(epoch_secs("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(epoch_secs("2026-10-10T24:00:00Z"), None);
        let w = |id: &str, starts: &str, ends: &str, published: &str| {
            present_maintenance(json!({
                "id": id, "title": id, "body": "", "starts_at": starts, "ends_at": ends,
                "kind": null, "end_unknown": 0, "published_at": published,
                "updated_at": published,
            }))
        };
        let now = "2026-10-10T12:00:00Z";
        let rows = [
            w(
                "later",
                "2026-10-11T00:00:00Z",
                "2026-10-11T02:00:00Z",
                "2026-10-01T00:00:00Z",
            ),
            w(
                "far",
                "2026-10-20T00:00:00Z",
                "2026-10-20T02:00:00Z",
                "2026-10-01T00:00:00Z",
            ),
            w(
                "hidden",
                "2026-10-10T13:00:00Z",
                "2026-10-10T14:00:00Z",
                "2026-10-10T12:30:00Z",
            ),
            w(
                "now",
                "2026-10-10T11:00:00Z",
                "2026-10-10T13:00:00Z",
                "2026-10-01T00:00:00Z",
            ),
            w(
                "over",
                "2026-10-10T09:00:00Z",
                "2026-10-10T12:00:00Z",
                "2026-10-01T00:00:00Z",
            ),
        ];
        let s = pick_status(&rows, now);
        assert_eq!(s["now"], now);
        assert_eq!(s["maintenance"]["active"]["id"], "now");
        assert_eq!(s["maintenance"]["active"]["kind"], "scheduled");
        assert_eq!(s["maintenance"]["active"]["end_unknown"], false);
        assert_eq!(s["maintenance"]["upcoming"]["id"], "later");
        let s = pick_status(&rows[1..2], now);
        assert!(s["maintenance"]["active"].is_null());
        assert!(
            s["maintenance"]["upcoming"].is_null(),
            "more than 72h ahead"
        );

        for off in ["", " ", "0", "off", "FALSE", "no"] {
            assert_eq!(env_maintenance(off, "", "", "", now), None, "{off:?}");
        }
        let f = env_maintenance("on", "", " DB 복구 중 ", "", now).unwrap();
        assert_eq!(f["kind"], "emergency");
        assert_eq!(f["title"], "긴급 서버 점검");
        assert_eq!(f["body"], "DB 복구 중");
        assert_eq!(f["end_unknown"], true);
        assert_eq!(f["starts_at"], now);
        let f = env_maintenance("1", "점검", "", "2026-10-10T15:00:00Z", now).unwrap();
        assert_eq!(f["ends_at"], "2026-10-10T15:00:00Z");
        assert_eq!(f["end_unknown"], false);
        let f = env_maintenance("1", "", "", "2026-10-10T11:00:00Z", now).unwrap();
        assert_eq!(
            f["end_unknown"], true,
            "a past MAINTENANCE_UNTIL is unknown"
        );

        assert!(allowed_during_maintenance(&Method::Get, "/api/me"));
        assert!(allowed_during_maintenance(&Method::Get, "/api/staff/me"));
        assert!(allowed_during_maintenance(&Method::Post, "/api/auth/login"));
        assert!(allowed_during_maintenance(&Method::Post, "/api/auth/csrf"));
        assert!(!allowed_during_maintenance(&Method::Post, "/api/me"));
        assert!(!allowed_during_maintenance(
            &Method::Post,
            "/api/auth/register"
        ));
        assert!(!allowed_during_maintenance(&Method::Get, "/api/orgs"));
        assert!(!allowed_during_maintenance(
            &Method::Post,
            "/api/partner-hooks/D-5"
        ));
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
