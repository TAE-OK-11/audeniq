//! 공지·이벤트·서버 점검 (예전 web/studio/worker.js + D1, 경로·입력 규칙 동일)
//!
//! 공개 (로그인 없이 읽기)
//!   GET  /api/notices, /api/notices/{id}, /api/events, /api/events/{id}
//!   GET  /api/status        진행 중·예정된 서버 점검 (스튜디오의 점검 화면·예고 배너)
//! 관리 (ADMIN 스태프 세션 + 쓰기는 CSRF; 비상 스크립트는 Bearer CONTENT_ADMIN_TOKEN)
//!   GET    /api/content/{notices|events|maintenance}        예약·숨김 글까지 전체 목록
//!   POST   /api/content/{table}                             새 글
//!   PUT    /api/content/{table}/{id}                        수정 (삭제된 글은 복구)
//!   DELETE /api/content/{table}/{id}                        삭제 (deleted_at 기록)
//! 점검 중 차단 ([`gate`])
//!   점검 중에는 일반 API를 503 MAINTENANCE로 막는다. 세션 확인·로그인, 콘텐츠 API, 관리자 사이트는 유지한다.
//! 비상 스위치 (DB·관리 화면이 안 될 때, 환경변수 — 바꾼 뒤 API 재시작)
//!   MAINTENANCE_MODE=on (선택: MAINTENANCE_TITLE, MAINTENANCE_MESSAGE, MAINTENANCE_UNTIL=ISO 시각)
use crate::{api::AppState, error::Error, staff};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, SecondsFormat, Utc};
use serde_json::{Map, Value, json};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const PUBLIC_CACHE: &str = "public, max-age=30";
/// 예고 배너는 시작 72시간 전부터
const NOTICE_AHEAD_HOURS: i64 = 72;
/// 점검 일정은 프로세스마다 10초 기억한다. /api/status는 모든 탭이 1분마다 부르고,
/// 점검 차단 검사는 모든 API 요청마다 돈다. 판정은 항상 현재 시각으로 다시 계산한다.
const STATUS_TTL: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Table {
    Notices,
    Events,
    Maintenance,
}
impl Table {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "notices" => Self::Notices,
            "events" => Self::Events,
            "maintenance" => Self::Maintenance,
            _ => return None,
        })
    }
    fn name(self) -> &'static str {
        match self {
            Self::Notices => "notices",
            Self::Events => "events",
            Self::Maintenance => "maintenance",
        }
    }
    fn id_prefix(self) -> &'static str {
        match self {
            Self::Notices => "n",
            Self::Events => "e",
            Self::Maintenance => "m",
        }
    }
    /// 행 → 응답 JSON (시각은 초 단위 UTC `...Z`, 날짜는 `YYYY-MM-DD`)
    fn json(self) -> &'static str {
        match self {
            Self::Notices => {
                "jsonb_build_object('id',id,'title',title,'body',body,'pinned',pinned,\
                 'published_at',to_char(published_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),\
                 'updated_at',to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'))"
            }
            Self::Events => {
                "jsonb_build_object('id',id,'title',title,'summary',summary,'body',body,'place',place,\
                 'starts_on',to_char(starts_on,'YYYY-MM-DD'),'ends_on',to_char(ends_on,'YYYY-MM-DD'),'link_url',link_url,\
                 'published_at',to_char(published_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),\
                 'updated_at',to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'))"
            }
            Self::Maintenance => {
                "jsonb_build_object('id',id,'title',title,'body',body,\
                 'starts_at',to_char(starts_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),\
                 'ends_at',to_char(ends_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),\
                 'kind',kind,'end_unknown',end_unknown,\
                 'published_at',to_char(published_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),\
                 'updated_at',to_char(updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'))"
            }
        }
    }
    fn order(self) -> &'static str {
        match self {
            Self::Notices => "pinned DESC, published_at DESC",
            Self::Events => "starts_on DESC",
            Self::Maintenance => "starts_at DESC",
        }
    }
}

// ---------------------------------------------------------------- 응답

fn reply(status: StatusCode, body: Value, cache: &'static str) -> Response {
    let mut r = (status, Json(body)).into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    r
}
pub(crate) fn fail(status: StatusCode, code: &'static str) -> Response {
    reply(
        status,
        json!({"error":{"code":code,"message":code}}),
        "no-store",
    )
}
fn from_error(e: Error) -> Response {
    e.into_response()
}

// ---------------------------------------------------------------- 시각

pub fn now_utc() -> DateTime<Utc> {
    // 초 단위로 자른다 (응답·비교 모두 초 단위)
    let n = Utc::now();
    DateTime::from_timestamp(n.timestamp(), 0).unwrap_or(n)
}
fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Secs, true)
}
pub fn today_kst(now: DateTime<Utc>) -> String {
    let kst = FixedOffset::east_opt(9 * 3600).expect("valid offset");
    now.with_timezone(&kst).format("%Y-%m-%d").to_string()
}
/// `YYYY-MM-DDTHH:MM:SSZ` 만 받는다 (Worker의 TS_RE와 같음)
fn parse_ts(s: &str) -> Option<DateTime<Utc>> {
    let b = s.as_bytes();
    if b.len() != 20 || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|t| t.with_timezone(&Utc))
}
fn parse_date(s: &str) -> Option<NaiveDate> {
    if s.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

pub fn event_status(starts_on: &str, ends_on: Option<&str>, today: &str) -> &'static str {
    if today < starts_on {
        "upcoming"
    } else if today <= ends_on.unwrap_or(starts_on) {
        "ongoing"
    } else {
        "ended"
    }
}
fn with_event_status(mut v: Value, today: &str) -> Value {
    let starts = v["starts_on"].as_str().unwrap_or_default().to_owned();
    let ends = v["ends_on"].as_str().map(str::to_owned);
    v["status"] = json!(event_status(&starts, ends.as_deref(), today));
    v
}

// ---------------------------------------------------------------- 입력 검증

pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// 제어문자(여러 줄 글은 줄바꿈·탭 허용)와 보이지 않는 방향 제어 문자
fn bad_char(c: char, multiline: bool) -> bool {
    let n = c as u32;
    if multiline && (c == '\n' || c == '\t') {
        return false;
    }
    n <= 0x1f
        || (0x7f..=0x9f).contains(&n)
        || (0x200b..=0x200f).contains(&n)
        || (0x202a..=0x202e).contains(&n)
        || (0x2066..=0x2069).contains(&n)
        || n == 0xfeff
}

fn text(
    v: Option<&Value>,
    max: usize,
    multiline: bool,
    required: bool,
) -> Result<String, &'static str> {
    let raw = match v {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return Err("INVALID_INPUT"),
    };
    let t = raw.replace("\r\n", "\n").replace('\r', "\n");
    let t = t.trim();
    if required && t.is_empty() {
        return Err("TITLE_REQUIRED");
    }
    if t.chars().count() > max {
        return Err("TOO_LONG");
    }
    if t.chars().any(|c| bad_char(c, multiline)) {
        return Err("TEXT_INVALID_CHARACTERS");
    }
    Ok(t.to_owned())
}

fn check_keys<'a>(
    input: &'a Value,
    allowed: &[&str],
) -> Result<&'a Map<String, Value>, &'static str> {
    let m = input.as_object().ok_or("INVALID_INPUT")?;
    if m.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err("INVALID_INPUT");
    }
    Ok(m)
}
fn check_id(v: Option<&Value>) -> Result<Option<String>, &'static str> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) if valid_id(s) => Ok(Some(s.clone())),
        _ => Err("ID_INVALID"),
    }
}
fn published_at(v: Option<&Value>, now: DateTime<Utc>) -> Result<DateTime<Utc>, &'static str> {
    match v {
        None | Some(Value::Null) => Ok(now),
        Some(Value::String(s)) if s.is_empty() => Ok(now),
        Some(Value::String(s)) => parse_ts(s).ok_or("PUBLISHED_AT_INVALID"),
        _ => Err("PUBLISHED_AT_INVALID"),
    }
}
fn flag(v: Option<&Value>) -> Result<bool, &'static str> {
    match v {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        _ => Err("INVALID_INPUT"),
    }
}
/// 빈 값(null·"")은 없음으로
fn optional_str(v: Option<&Value>) -> Option<&Value> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        other => other,
    }
}

#[derive(Debug, PartialEq)]
pub enum Row {
    Notice {
        title: String,
        body: String,
        pinned: bool,
        published_at: DateTime<Utc>,
    },
    Event {
        title: String,
        summary: String,
        body: String,
        place: String,
        starts_on: NaiveDate,
        ends_on: Option<NaiveDate>,
        link_url: Option<String>,
        published_at: DateTime<Utc>,
    },
    Maintenance {
        title: String,
        body: String,
        starts_at: DateTime<Utc>,
        ends_at: DateTime<Utc>,
        kind: &'static str,
        end_unknown: bool,
        published_at: DateTime<Utc>,
    },
}

/// 입력 검증 → (요청한 id, 행). 규칙은 마이그레이션 0086의 CHECK와 같다.
pub fn validate(
    table: Table,
    input: &Value,
    now: DateTime<Utc>,
) -> Result<(Option<String>, Row), &'static str> {
    match table {
        Table::Notices => {
            let m = check_keys(input, &["id", "title", "body", "pinned", "published_at"])?;
            let row = Row::Notice {
                title: text(m.get("title"), 200, false, true)?,
                body: text(m.get("body"), 20000, true, false)?,
                pinned: flag(m.get("pinned"))?,
                published_at: published_at(m.get("published_at"), now)?,
            };
            Ok((check_id(m.get("id"))?, row))
        }
        Table::Maintenance => {
            let m = check_keys(
                input,
                &[
                    "id",
                    "title",
                    "body",
                    "starts_at",
                    "ends_at",
                    "published_at",
                    "kind",
                    "end_unknown",
                ],
            )?;
            let kind = match m.get("kind") {
                None | Some(Value::Null) => "scheduled",
                Some(Value::String(k)) if k == "scheduled" => "scheduled",
                Some(Value::String(k)) if k == "emergency" => "emergency",
                _ => return Err("INVALID_INPUT"),
            };
            let end_unknown = flag(m.get("end_unknown"))?;
            let starts = m
                .get("starts_at")
                .and_then(Value::as_str)
                .and_then(parse_ts);
            let ends = m.get("ends_at").and_then(Value::as_str).and_then(parse_ts);
            let (Some(starts_at), Some(ends_at)) = (starts, ends) else {
                return Err("DATES_INVALID");
            };
            if ends_at <= starts_at {
                return Err("DATES_INVALID");
            }
            let row = Row::Maintenance {
                title: text(m.get("title"), 200, false, true)?,
                body: text(m.get("body"), 2000, true, false)?,
                starts_at,
                ends_at,
                kind,
                end_unknown,
                published_at: published_at(m.get("published_at"), now)?,
            };
            Ok((check_id(m.get("id"))?, row))
        }
        Table::Events => {
            let m = check_keys(
                input,
                &[
                    "id",
                    "title",
                    "summary",
                    "body",
                    "place",
                    "starts_on",
                    "ends_on",
                    "link_url",
                    "published_at",
                ],
            )?;
            let starts_on = m
                .get("starts_on")
                .and_then(Value::as_str)
                .and_then(parse_date)
                .ok_or("DATES_INVALID")?;
            let ends_on = match optional_str(m.get("ends_on")) {
                None => None,
                Some(v) => Some(
                    v.as_str()
                        .and_then(parse_date)
                        .filter(|d| *d >= starts_on)
                        .ok_or("DATES_INVALID")?,
                ),
            };
            let link_url = match optional_str(m.get("link_url")) {
                None => None,
                Some(Value::String(s))
                    if s.starts_with("https://")
                        && s.len() <= 500
                        && !s.chars().any(char::is_whitespace) =>
                {
                    Some(s.clone())
                }
                Some(_) => return Err("LINK_URL_INVALID"),
            };
            let row = Row::Event {
                title: text(m.get("title"), 200, false, true)?,
                summary: text(m.get("summary"), 300, false, false)?,
                body: text(m.get("body"), 20000, true, false)?,
                place: text(m.get("place"), 120, false, false)?,
                starts_on,
                ends_on,
                link_url,
                published_at: published_at(m.get("published_at"), now)?,
            };
            Ok((check_id(m.get("id"))?, row))
        }
    }
}

// ---------------------------------------------------------------- 서버 점검

fn env_text(name: &str) -> String {
    std::env::var(name).unwrap_or_default().trim().to_owned()
}

/// 비상 스위치(MAINTENANCE_MODE)가 켜져 있으면 지금부터의 긴급 점검
pub fn env_maintenance(
    mode: &str,
    title: &str,
    message: &str,
    until: &str,
    now: DateTime<Utc>,
) -> Option<Value> {
    if matches!(
        mode.trim().to_lowercase().as_str(),
        "" | "0" | "off" | "false" | "no"
    ) {
        return None;
    }
    let until = parse_ts(until.trim()).filter(|t| *t > now);
    let now_s = iso(now);
    Some(json!({
        "id": "emergency-switch",
        "title": if title.is_empty() { "긴급 서버 점검" } else { title },
        "body": message,
        "starts_at": now_s,
        "ends_at": until.map(iso).unwrap_or_else(|| "9999-12-31T00:00:00Z".into()),
        "kind": "emergency",
        "end_unknown": until.is_none(),
        "published_at": now_s,
        "updated_at": now_s,
    }))
}
fn forced_maintenance(now: DateTime<Utc>) -> Option<Value> {
    env_maintenance(
        &env_text("MAINTENANCE_MODE"),
        &env_text("MAINTENANCE_TITLE"),
        &env_text("MAINTENANCE_MESSAGE"),
        &env_text("MAINTENANCE_UNTIL"),
        now,
    )
}

fn ts_of(v: &Value, key: &str) -> Option<DateTime<Utc>> {
    v[key].as_str().and_then(parse_ts)
}

/// 지금 진행 중인 점검과, 72시간 안에 시작하는 예고된 점검
pub fn pick_status(rows: &[Value], now: DateTime<Utc>) -> Value {
    let live = rows.iter().filter_map(|r| {
        let (s, e, p) = (
            ts_of(r, "starts_at")?,
            ts_of(r, "ends_at")?,
            ts_of(r, "published_at")?,
        );
        (p <= now && e > now).then_some((s, r))
    });
    let mut active: Option<(DateTime<Utc>, &Value)> = None;
    let mut upcoming: Option<(DateTime<Utc>, &Value)> = None;
    for (s, r) in live {
        let slot = if s <= now {
            &mut active
        } else if s - now <= Duration::hours(NOTICE_AHEAD_HOURS) {
            &mut upcoming
        } else {
            continue;
        };
        if slot.is_none_or(|(best, _)| s < best) {
            *slot = Some((s, r));
        }
    }
    json!({"now": iso(now), "maintenance": {
        "active": active.map(|(_, r)| r.clone()),
        "upcoming": upcoming.map(|(_, r)| r.clone()),
    }})
}

// Arc: every gated API request reads this; a hit is a refcount bump, not a
// deep copy of the rows.
static ROWS: Mutex<Option<(Instant, Arc<[Value]>)>> = Mutex::new(None);
pub fn reset_maintenance_cache() {
    *ROWS.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

async fn maintenance_rows(s: &AppState, now: DateTime<Utc>) -> Arc<[Value]> {
    if let Some((at, rows)) = ROWS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
        && at.elapsed() < STATUS_TTL
    {
        return rows.clone();
    }
    let sql = format!(
        "SELECT {} FROM content.maintenance WHERE deleted_at IS NULL AND ends_at > $1 ORDER BY starts_at LIMIT 20",
        Table::Maintenance.json()
    );
    // 표·열 이름은 이 파일의 상수뿐 (사용자 입력 없음 — 이 파일의 모든 AssertSqlSafe 동일)
    // DB를 못 읽어도(마이그레이션 전·장애) 스튜디오는 점검 없음으로 동작한다
    let rows: Vec<Value> = match sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(now)
        .fetch_all(&s.pool)
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "maintenance status unavailable");
            return Arc::from([]);
        }
    };
    let rows: Arc<[Value]> = rows.into();
    *ROWS.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), rows.clone()));
    rows
}

pub async fn service_status(s: &AppState, now: DateTime<Utc>) -> Value {
    let mut status = pick_status(&maintenance_rows(s, now).await, now);
    if let Some(forced) = forced_maintenance(now) {
        status["maintenance"]["active"] = forced;
    }
    status
}

async fn active_maintenance(s: &AppState) -> Option<Value> {
    let now = now_utc();
    if let Some(forced) = forced_maintenance(now) {
        return Some(forced);
    }
    let rows = maintenance_rows(s, now).await;
    // Usual case: nothing scheduled, so skip building the status document.
    if rows.is_empty() {
        return None;
    }
    let mut status = pick_status(&rows, now);
    let active = status["maintenance"]["active"].take();
    (!active.is_null()).then_some(active)
}

/// 점검 중에도 열어 두는 요청: 세션 확인·로그인, 콘텐츠·상태 API, 상태 확인
pub fn allowed_during_maintenance(method: &Method, path: &str) -> bool {
    let session = (*method == Method::GET && matches!(path, "/api/me" | "/api/staff/me"))
        || (*method == Method::POST
            && matches!(
                path,
                "/api/auth/login" | "/api/auth/logout" | "/api/auth/csrf"
            ));
    session
        || !path.starts_with("/api/")
        || path == "/api/status"
        || path.starts_with("/api/content/")
        || path == "/api/notices"
        || path.starts_with("/api/notices/")
        || path == "/api/events"
        || path.starts_with("/api/events/")
}

/// 점검 중 API 차단 (서비스 비밀 확인 뒤에 돈다, api::router). 관리자 사이트는 막지 않는다.
/// MCP calls that keep working during maintenance: session setup, the tool
/// and prompt lists, and the public status and notices (no account data).
fn mcp_allowed_during_maintenance(msg: &Value) -> bool {
    match msg.get("method").and_then(Value::as_str) {
        Some(
            "initialize"
            | "notifications/initialized"
            | "ping"
            | "server/discover"
            | "tools/list"
            | "prompts/list"
            | "prompts/get"
            | "resources/list"
            | "resources/read",
        ) => true,
        Some("tools/call") => matches!(
            msg["params"]["name"].as_str(),
            Some("get_service_status" | "list_announcements" | "get_announcement")
        ),
        _ => false,
    }
}

pub async fn gate(State(s): State<AppState>, req: Request, next: Next) -> Response {
    if crate::sites::site(req.headers()) == crate::sites::Site::Admin
        || allowed_during_maintenance(req.method(), req.uri().path())
    {
        return next.run(req).await;
    }
    match active_maintenance(&s).await {
        Some(m) if req.uri().path() == crate::mcp::MCP_PATH && req.method() == Method::POST => {
            // AI clients: discovery and the service status keep working (so the
            // assistant can explain the maintenance); everything else gets a
            // JSON-RPC error with the request's id, not a transport failure.
            let (parts, body) = req.into_parts();
            let bytes = axum::body::to_bytes(body, 64 * 1024)
                .await
                .unwrap_or_default();
            let msg: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            if mcp_allowed_during_maintenance(&msg) {
                return next
                    .run(Request::from_parts(parts, axum::body::Body::from(bytes)))
                    .await;
            }
            let title = m["title"].as_str().unwrap_or("Maintenance");
            let mut r = reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"jsonrpc":"2.0","id":msg.get("id").cloned().unwrap_or(Value::Null),"error":{"code":-32000,
                    "message":format!("AUDENIQ is under maintenance: {title}"),"data":{"maintenance":m}}}),
                "no-store",
            );
            r.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
            r
        }
        Some(_)
            if req.method() == Method::POST
                && matches!(
                    req.uri().path(),
                    "/api/oauth/token"
                        | "/api/oauth/register"
                        | "/api/oauth/revoke"
                        | "/api/oauth/device_authorization"
                ) =>
        {
            // Token, registration and revocation in OAuth's shape: clients retry.
            let mut r = reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":"temporarily_unavailable","error_description":"AUDENIQ is under maintenance. Retry later."}),
                "no-store",
            );
            r.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
            r
        }
        Some(m) => {
            let mut r = reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":{"code":"MAINTENANCE","message":m["title"]},"maintenance":m}),
                "no-store",
            );
            r.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
            r
        }
        None => next.run(req).await,
    }
}

// ---------------------------------------------------------------- 공개 읽기

async fn status(State(s): State<AppState>) -> Response {
    reply(
        StatusCode::OK,
        service_status(&s, now_utc()).await,
        "no-store",
    )
}

fn public_table(name: &str) -> Option<Table> {
    Table::parse(name).filter(|t| *t != Table::Maintenance)
}

/// 공개된 공지·이벤트 목록 (게시 시각이 지난 것, 최대 200개). MCP 공개 도구도 쓴다.
pub async fn published(s: &AppState, table: Table) -> Result<Vec<Value>, Error> {
    let now = now_utc();
    let sql = format!(
        "SELECT {} FROM content.{} WHERE deleted_at IS NULL AND published_at <= $1 ORDER BY {} LIMIT 200",
        table.json(),
        table.name(),
        table.order()
    );
    let rows = sqlx::query_scalar::<_, Value>(sqlx::AssertSqlSafe(sql))
        .bind(now)
        .fetch_all(&s.pool)
        .await?;
    let today = today_kst(now);
    Ok(rows
        .into_iter()
        .map(|r| present(table, r, &today))
        .collect())
}

/// 공개된 공지·이벤트 하나. 없거나 아직 게시 전이면 `None`.
pub async fn published_one(s: &AppState, table: Table, id: &str) -> Result<Option<Value>, Error> {
    if !valid_id(id) {
        return Ok(None);
    }
    let now = now_utc();
    let sql = format!(
        "SELECT {} FROM content.{} WHERE id = $1 AND deleted_at IS NULL AND published_at <= $2",
        table.json(),
        table.name()
    );
    let row = sqlx::query_scalar::<_, Value>(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(now)
        .fetch_optional(&s.pool)
        .await?;
    Ok(row.map(|r| present(table, r, &today_kst(now))))
}

async fn public_list(State(s): State<AppState>, Path(name): Path<String>) -> Response {
    let Some(table) = public_table(&name) else {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    };
    match published(&s, table).await {
        Ok(items) => reply(StatusCode::OK, json!({"items": items}), PUBLIC_CACHE),
        Err(e) => from_error(e),
    }
}

async fn public_get(
    State(s): State<AppState>,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    let Some(table) = public_table(&name) else {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    };
    match published_one(&s, table, &id).await {
        Ok(Some(row)) => reply(StatusCode::OK, row, PUBLIC_CACHE),
        Ok(None) => fail(StatusCode::NOT_FOUND, "NOT_FOUND"),
        Err(e) => from_error(e),
    }
}

fn present(table: Table, row: Value, today: &str) -> Value {
    if table == Table::Events {
        with_event_status(row, today)
    } else {
        row
    }
}

// ---------------------------------------------------------------- 관리

/// ADMIN 스태프 세션(쓰기는 CSRF까지). 비상 도구용 Bearer CONTENT_ADMIN_TOKEN도 받는다.
async fn authorize(s: &AppState, h: &HeaderMap, write: bool) -> Option<Response> {
    if let Some(auth) = h.get(header::AUTHORIZATION) {
        let token = std::env::var("CONTENT_ADMIN_TOKEN").unwrap_or_default();
        if token.len() < 32 {
            return Some(fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "CONTENT_ADMIN_DISABLED",
            ));
        }
        let given = auth
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or_default();
        return (!crate::auth::secret_matches_digest(given, &crate::auth::token_digest(&token)))
            .then(|| fail(StatusCode::UNAUTHORIZED, "UNAUTHENTICATED"));
    }
    match staff::staff(s, h, write).await {
        Ok(st) if st.role.is_admin() => None,
        Ok(_) => Some(fail(StatusCode::FORBIDDEN, "FORBIDDEN")),
        Err(e) => Some(from_error(e)),
    }
}

async fn admin_list(State(s): State<AppState>, h: HeaderMap, Path(name): Path<String>) -> Response {
    let Some(table) = Table::parse(&name) else {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    };
    if let Some(denied) = authorize(&s, &h, false).await {
        return denied;
    }
    let now = now_utc();
    let sql = format!(
        "SELECT {} || jsonb_build_object(\
           'created_at',to_char(created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),\
           'deleted_at',to_char(deleted_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'))\
         FROM content.{} ORDER BY {} LIMIT 500",
        table.json(),
        table.name(),
        table.order()
    );
    match sqlx::query_scalar::<_, Value>(sqlx::AssertSqlSafe(sql))
        .fetch_all(&s.pool)
        .await
    {
        Ok(rows) => {
            let today = today_kst(now);
            let items: Vec<Value> = rows
                .into_iter()
                .map(|r| present(table, r, &today))
                .collect();
            reply(
                StatusCode::OK,
                json!({"items": items, "now": iso(now)}),
                "no-store",
            )
        }
        Err(e) => from_error(e.into()),
    }
}

fn parse_body(body: &Bytes) -> Result<Value, &'static str> {
    let text = std::str::from_utf8(body).map_err(|_| "INVALID_INPUT")?;
    serde_json::from_str(text).map_err(|_| "INVALID_INPUT")
}

async fn admin_create(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(name): Path<String>,
    body: Bytes,
) -> Response {
    let Some(table) = Table::parse(&name) else {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    };
    if let Some(denied) = authorize(&s, &h, true).await {
        return denied;
    }
    let now = now_utc();
    let (id, row) = match parse_body(&body).and_then(|v| validate(table, &v, now)) {
        Ok(v) => v,
        Err(code) => return fail(StatusCode::BAD_REQUEST, code),
    };
    let id = id.unwrap_or_else(|| {
        format!(
            "{}-{}-{}",
            table.id_prefix(),
            now.format("%Y%m%d"),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        )
    });
    let result = write_row(&s.pool, table, &id, &row, now, true).await;
    finish(table, result, StatusCode::CREATED)
}

async fn admin_update(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((name, id)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let Some(table) = Table::parse(&name) else {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    };
    if !valid_id(&id) {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    }
    if let Some(denied) = authorize(&s, &h, true).await {
        return denied;
    }
    let now = now_utc();
    let row = match parse_body(&body).and_then(|v| validate(table, &v, now)) {
        Ok((Some(given), _)) if given != id => return fail(StatusCode::BAD_REQUEST, "ID_INVALID"),
        Ok((_, row)) => row,
        Err(code) => return fail(StatusCode::BAD_REQUEST, code),
    };
    let result = write_row(&s.pool, table, &id, &row, now, false).await;
    finish(table, result, StatusCode::OK)
}

async fn admin_delete(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((name, id)): Path<(String, String)>,
) -> Response {
    let Some(table) = Table::parse(&name) else {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    };
    if !valid_id(&id) {
        return fail(StatusCode::NOT_FOUND, "NOT_FOUND");
    }
    if let Some(denied) = authorize(&s, &h, true).await {
        return denied;
    }
    let sql = format!(
        "UPDATE content.{} SET deleted_at = $1, updated_at = $1 WHERE id = $2 AND deleted_at IS NULL",
        table.name()
    );
    let result = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(now_utc())
        .bind(&id)
        .execute(&s.pool)
        .await;
    if table == Table::Maintenance {
        reset_maintenance_cache();
    }
    match result {
        Ok(r) if r.rows_affected() == 1 => reply(
            StatusCode::OK,
            json!({"id": id, "deleted": true}),
            "no-store",
        ),
        Ok(_) => fail(StatusCode::NOT_FOUND, "NOT_FOUND"),
        Err(e) => from_error(e.into()),
    }
}

enum Written {
    Row(Value),
    Missing,
    Taken,
}

fn finish(table: Table, result: Result<Written, sqlx::Error>, ok: StatusCode) -> Response {
    if table == Table::Maintenance {
        reset_maintenance_cache();
    }
    match result {
        Ok(Written::Row(v)) => reply(ok, present(table, v, &today_kst(now_utc())), "no-store"),
        Ok(Written::Missing) => fail(StatusCode::NOT_FOUND, "NOT_FOUND"),
        Ok(Written::Taken) => fail(StatusCode::CONFLICT, "ID_TAKEN"),
        Err(e) => from_error(e.into()),
    }
}

/// 새 글(insert) 또는 수정. 수정은 삭제된 글도 다시 보이게 한다(복구).
async fn write_row(
    pool: &sqlx::PgPool,
    table: Table,
    id: &str,
    row: &Row,
    now: DateTime<Utc>,
    insert: bool,
) -> Result<Written, sqlx::Error> {
    let ret = table.json();
    let q = match (row, insert) {
        (Row::Notice { .. }, true) => format!(
            "INSERT INTO content.notices (id,title,body,pinned,published_at,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$6) ON CONFLICT (id) DO NOTHING RETURNING {ret}"
        ),
        (Row::Notice { .. }, false) => format!(
            "UPDATE content.notices SET title=$2,body=$3,pinned=$4,published_at=$5,updated_at=$6,deleted_at=NULL \
             WHERE id=$1 RETURNING {ret}"
        ),
        (Row::Event { .. }, true) => format!(
            "INSERT INTO content.events (id,title,summary,body,place,starts_on,ends_on,link_url,published_at,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$10) ON CONFLICT (id) DO NOTHING RETURNING {ret}"
        ),
        (Row::Event { .. }, false) => format!(
            "UPDATE content.events SET title=$2,summary=$3,body=$4,place=$5,starts_on=$6,ends_on=$7,link_url=$8,\
             published_at=$9,updated_at=$10,deleted_at=NULL WHERE id=$1 RETURNING {ret}"
        ),
        (Row::Maintenance { .. }, true) => format!(
            "INSERT INTO content.maintenance (id,title,body,starts_at,ends_at,kind,end_unknown,published_at,created_at,updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$9) ON CONFLICT (id) DO NOTHING RETURNING {ret}"
        ),
        (Row::Maintenance { .. }, false) => format!(
            "UPDATE content.maintenance SET title=$2,body=$3,starts_at=$4,ends_at=$5,kind=$6,end_unknown=$7,\
             published_at=$8,updated_at=$9,deleted_at=NULL WHERE id=$1 RETURNING {ret}"
        ),
    };
    let query = sqlx::query_scalar::<_, Value>(sqlx::AssertSqlSafe(q)).bind(id);
    let query = match row {
        Row::Notice {
            title,
            body,
            pinned,
            published_at,
        } => query.bind(title).bind(body).bind(pinned).bind(published_at),
        Row::Event {
            title,
            summary,
            body,
            place,
            starts_on,
            ends_on,
            link_url,
            published_at,
        } => query
            .bind(title)
            .bind(summary)
            .bind(body)
            .bind(place)
            .bind(starts_on)
            .bind(ends_on)
            .bind(link_url)
            .bind(published_at),
        Row::Maintenance {
            title,
            body,
            starts_at,
            ends_at,
            kind,
            end_unknown,
            published_at,
        } => query
            .bind(title)
            .bind(body)
            .bind(starts_at)
            .bind(ends_at)
            .bind(*kind)
            .bind(end_unknown)
            .bind(published_at),
    };
    let _ = table;
    Ok(match query.bind(now).fetch_optional(pool).await? {
        Some(v) => Written::Row(v),
        None if insert => Written::Taken,
        None => Written::Missing,
    })
}

// ---------------------------------------------------------------- 운영자 CLI (audeniq-admin maintenance|notice)
// 비상 업무만: API·로그인·관리자 화면에 들어갈 수 없을 때도 DB에 바로 점검을 시작·연장·종료·예약·취소하고
// 긴급 공지를 올리고 내린다. 관리 화면과 같은 행을 쓰고, 도는 API는 STATUS_TTL 안에 반영한다.

/// 관리 화면 긴급 점검의 기본 안내 (web/admin ContentAdmin의 EMERGENCY_TEXT)
pub const EMERGENCY_BODY: &str = "서비스를 안정적으로 되돌리기 위해 긴급 점검을 하고 있어요. 점검 중에는 로그인과 발매 접수를 할 수 없어요. 불편을 드려 죄송해요.";
/// 종료 시각 미정으로 시작할 때의 예상 종료 (관리 화면과 같음)
const UNKNOWN_END_MINUTES: i64 = 12 * 60;

async fn cli_audit(
    c: &mut sqlx::PgConnection,
    operator: &str,
    action: &str,
    id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO operations.audit_events(id,actor_service,action,reason_code,request_id) VALUES($1,$2,$3,$4,$5)")
        .bind(uuid::Uuid::new_v4())
        .bind(format!("audeniq-admin:{operator}"))
        .bind(action)
        .bind(id)
        .bind(uuid::Uuid::new_v4())
        .execute(c)
        .await?;
    Ok(())
}

async fn cli_rows(
    c: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
) -> Result<Vec<Value>, sqlx::Error> {
    let sql = format!(
        "SELECT {} FROM content.maintenance WHERE deleted_at IS NULL AND ends_at > $1 ORDER BY starts_at LIMIT 20",
        Table::Maintenance.json()
    );
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(now)
        .fetch_all(c)
        .await
}

/// 진행 중인 점검 행의 id (게시됐고 시작했고 아직 안 끝난 것)
async fn cli_active_ids(
    c: &mut sqlx::PgConnection,
    now: DateTime<Utc>,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT id FROM content.maintenance WHERE deleted_at IS NULL AND published_at <= $1 AND starts_at <= $1 AND ends_at > $1
          ORDER BY starts_at FOR UPDATE",
    )
    .bind(now)
    .fetch_all(c)
    .await
}

/// `/api/status`와 같은 모양 + 이 프로세스 환경의 비상 스위치(MAINTENANCE_MODE)
pub async fn cli_status(pool: &sqlx::PgPool) -> crate::error::Result<Value> {
    let now = now_utc();
    let mut c = pool.acquire().await?;
    let mut status = pick_status(&cli_rows(&mut c, now).await?, now);
    status["emergency_switch"] = forced_maintenance(now).unwrap_or(Value::Null);
    Ok(status)
}

/// 지금부터 긴급 점검. `minutes`가 없으면 종료 시각 미정.
pub async fn cli_start(
    pool: &sqlx::PgPool,
    operator: &str,
    minutes: Option<i64>,
    title: Option<&str>,
    body: Option<&str>,
) -> crate::error::Result<Value> {
    if minutes.is_some_and(|m| !(1..=7 * 24 * 60).contains(&m)) {
        return Err(Error::InvalidCode("MINUTES_INVALID"));
    }
    let title = text(
        Some(&json!(title.unwrap_or("긴급 서버 점검"))),
        200,
        false,
        true,
    )
    .map_err(Error::InvalidCode)?;
    let body = text(
        Some(&json!(body.unwrap_or(EMERGENCY_BODY))),
        2000,
        true,
        false,
    )
    .map_err(Error::InvalidCode)?;
    let now = now_utc();
    let mut tx = pool.begin().await?;
    // 동시에 두 운영자가 시작해도 점검 행은 하나
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('content.maintenance.cli'))")
        .execute(&mut *tx)
        .await?;
    if !cli_active_ids(&mut tx, now).await?.is_empty() {
        return Err(Error::PolicyGate("MAINTENANCE_ALREADY_ACTIVE"));
    }
    let starts_at = now - Duration::seconds(1);
    let ends_at = now + Duration::minutes(minutes.unwrap_or(UNKNOWN_END_MINUTES));
    let id = format!(
        "{}-{}-{}",
        Table::Maintenance.id_prefix(),
        now.format("%Y%m%d"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let row: Value = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "INSERT INTO content.maintenance (id,title,body,starts_at,ends_at,kind,end_unknown,published_at,created_at,updated_at) \
         VALUES ($1,$2,$3,$4,$5,'emergency',$6,$4,$7,$7) RETURNING {}",
        Table::Maintenance.json()
    )))
    .bind(&id)
    .bind(&title)
    .bind(&body)
    .bind(starts_at)
    .bind(ends_at)
    .bind(minutes.is_none())
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    cli_audit(&mut tx, operator, "maintenance.started", &id).await?;
    tx.commit().await?;
    Ok(row)
}

/// 진행 중인 점검을 지금 끝낸다 (예약 점검이 진행 중이어도 끝낸다).
pub async fn cli_stop(pool: &sqlx::PgPool, operator: &str) -> crate::error::Result<Value> {
    let now = now_utc();
    let mut tx = pool.begin().await?;
    let ids = cli_active_ids(&mut tx, now).await?;
    if ids.is_empty() {
        return Err(Error::PolicyGate("MAINTENANCE_NOT_ACTIVE"));
    }
    // ends_at > starts_at 제약: 막 시작한 점검은 시작 1초 뒤에 끝난다
    sqlx::query(
        "UPDATE content.maintenance SET ends_at=greatest($2, starts_at + interval '1 second'), end_unknown=false, updated_at=$2
          WHERE id = ANY($1)",
    )
    .bind(&ids)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    for id in &ids {
        cli_audit(&mut tx, operator, "maintenance.stopped", id).await?;
    }
    tx.commit().await?;
    Ok(json!({"stopped": ids, "ended_at": iso(now)}))
}

/// 진행 중인 점검의 예상 종료를 `minutes`분 늦춘다 (지난 예정이면 지금부터).
pub async fn cli_extend(
    pool: &sqlx::PgPool,
    operator: &str,
    minutes: i64,
) -> crate::error::Result<Value> {
    if !(1..=7 * 24 * 60).contains(&minutes) {
        return Err(Error::InvalidCode("MINUTES_INVALID"));
    }
    let now = now_utc();
    let mut tx = pool.begin().await?;
    let ids = cli_active_ids(&mut tx, now).await?;
    if ids.is_empty() {
        return Err(Error::PolicyGate("MAINTENANCE_NOT_ACTIVE"));
    }
    sqlx::query(
        "UPDATE content.maintenance SET ends_at=greatest(ends_at, $2) + make_interval(mins => $3), end_unknown=false, updated_at=$2
          WHERE id = ANY($1)",
    )
    .bind(&ids)
    .bind(now)
    .bind(minutes as i32)
    .execute(&mut *tx)
    .await?;
    for id in &ids {
        cli_audit(&mut tx, operator, "maintenance.extended", id).await?;
    }
    tx.commit().await?;
    let mut c = pool.acquire().await?;
    Ok(pick_status(&cli_rows(&mut c, now).await?, now))
}

/// CLI 시각: `YYYY-MM-DDTHH:MM:SSZ`(UTC) 또는 한국 시각 `YYYY-MM-DD HH:MM`
/// (`T`로 이어도 된다). 응답·저장은 UTC.
pub fn cli_time(v: &str) -> crate::error::Result<String> {
    let v = v.trim();
    if let Some(t) = parse_ts(v) {
        return Ok(iso(t));
    }
    let kst = FixedOffset::east_opt(9 * 3600).expect("valid offset");
    ["%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|f| chrono::NaiveDateTime::parse_from_str(v, f).ok())
        .and_then(|t| t.and_local_timezone(kst).single())
        .map(|t| iso(t.with_timezone(&Utc)))
        .ok_or(Error::InvalidCode("TIME_INVALID"))
}

fn cli_table(kind: &str) -> crate::error::Result<Table> {
    match Table::parse(kind) {
        Some(t @ (Table::Notices | Table::Maintenance)) => Ok(t),
        _ => Err(Error::InvalidCode("CONTENT_KIND_INVALID")),
    }
}

/// 공지(`notices`)나 점검(`maintenance`) 목록: 예약·숨김 글 포함, 삭제한 글 제외, 최근 50개
pub async fn cli_list(pool: &sqlx::PgPool, kind: &str) -> crate::error::Result<Value> {
    let table = cli_table(kind)?;
    let sql = format!(
        "SELECT {} FROM content.{} WHERE deleted_at IS NULL ORDER BY {} LIMIT 50",
        table.json(),
        table.name(),
        table.order()
    );
    let rows: Vec<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_all(pool)
        .await?;
    let now = now_utc();
    Ok(json!({"now": iso(now), "items": rows}))
}

/// 관리 화면의 새 글과 같은 입력·검증(`validate`)으로 공지·예약 점검을 올린다.
pub async fn cli_create(
    pool: &sqlx::PgPool,
    operator: &str,
    kind: &str,
    input: Value,
) -> crate::error::Result<Value> {
    let table = cli_table(kind)?;
    let now = now_utc();
    let (_, row) = validate(table, &input, now).map_err(Error::InvalidCode)?;
    let id = format!(
        "{}-{}-{}",
        table.id_prefix(),
        now.format("%Y%m%d"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let Written::Row(v) = write_row(pool, table, &id, &row, now, true).await? else {
        return Err(Error::Conflict);
    };
    let mut c = pool.acquire().await?;
    cli_audit(
        &mut c,
        operator,
        &format!("content.{}.created", table.name()),
        &id,
    )
    .await?;
    Ok(present(table, v, &today_kst(now)))
}

/// 공지 내리기, 예약 점검 취소 (삭제 기록, 관리 화면에서 복구 가능).
/// 진행 중인 점검은 기록이 남도록 `cli_stop`으로 끝낸다.
pub async fn cli_remove(
    pool: &sqlx::PgPool,
    operator: &str,
    kind: &str,
    id: &str,
) -> crate::error::Result<Value> {
    let table = cli_table(kind)?;
    if !valid_id(id) {
        return Err(Error::NotFound);
    }
    let now = now_utc();
    let mut tx = pool.begin().await?;
    if table == Table::Maintenance && cli_active_ids(&mut tx, now).await?.iter().any(|a| a == id) {
        return Err(Error::PolicyGate("MAINTENANCE_ACTIVE_USE_STOP"));
    }
    let sql = format!(
        "UPDATE content.{} SET deleted_at = $1, updated_at = $1 WHERE id = $2 AND deleted_at IS NULL",
        table.name()
    );
    let n = sqlx::query(sqlx::AssertSqlSafe(sql))
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(Error::NotFound);
    }
    cli_audit(
        &mut tx,
        operator,
        &format!("content.{}.removed", table.name()),
        id,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id": id, "deleted": true}))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/status", get(status))
        .route("/api/content/{table}", get(admin_list).post(admin_create))
        .route(
            "/api/content/{table}/{id}",
            axum::routing::put(admin_update).delete(admin_delete),
        )
        // 공개 목록·상세: notices, events (그 밖의 이름은 404)
        .route(
            "/api/notices",
            get(|s: State<AppState>| public_list(s, Path("notices".into()))),
        )
        .route(
            "/api/events",
            get(|s: State<AppState>| public_list(s, Path("events".into()))),
        )
        .route(
            "/api/notices/{id}",
            get(|s: State<AppState>, Path(id): Path<String>| {
                public_get(s, Path(("notices".into(), id)))
            }),
        )
        .route(
            "/api/events/{id}",
            get(|s: State<AppState>, Path(id): Path<String>| {
                public_get(s, Path(("events".into(), id)))
            }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<Utc> {
        parse_ts(s).unwrap()
    }

    #[test]
    fn notices_validate_like_the_worker() {
        let now = t("2026-10-10T00:00:00Z");
        let (id, row) = validate(
            Table::Notices,
            &json!({"title":" 안내 ","body":"a\r\nb","pinned":true}),
            now,
        )
        .unwrap();
        assert_eq!(id, None);
        assert_eq!(
            row,
            Row::Notice {
                title: "안내".into(),
                body: "a\nb".into(),
                pinned: true,
                published_at: now
            }
        );
        for (input, code) in [
            (json!({"title":""}), "TITLE_REQUIRED"),
            (json!({"title":"a","extra":1}), "INVALID_INPUT"),
            (json!({"title":"a","pinned":"yes"}), "INVALID_INPUT"),
            (json!({"title":"a\u{202e}b"}), "TEXT_INVALID_CHARACTERS"),
            (json!({"title":"a\nb"}), "TEXT_INVALID_CHARACTERS"),
            (json!({"title":"x".repeat(201)}), "TOO_LONG"),
            (json!({"title":"a","id":"Bad_ID"}), "ID_INVALID"),
            (
                json!({"title":"a","published_at":"2026-10-10"}),
                "PUBLISHED_AT_INVALID",
            ),
            (json!([]), "INVALID_INPUT"),
        ] {
            assert_eq!(
                validate(Table::Notices, &input, now).unwrap_err(),
                code,
                "{input}"
            );
        }
        assert!(validate(Table::Notices, &json!({"title":"a","body":"x\ty\nz"}), now).is_ok());
    }

    #[test]
    fn events_and_maintenance_check_dates_and_links() {
        let now = t("2026-10-10T00:00:00Z");
        let ok = json!({"title":"t","starts_on":"2026-10-01","ends_on":"","link_url":"https://a.example/x"});
        assert!(validate(Table::Events, &ok, now).is_ok());
        for (input, code) in [
            (
                json!({"title":"t","starts_on":"2026-02-31"}),
                "DATES_INVALID",
            ),
            (
                json!({"title":"t","starts_on":"2026-10-02","ends_on":"2026-10-01"}),
                "DATES_INVALID",
            ),
            (
                json!({"title":"t","starts_on":"2026-10-02","link_url":"http://a"}),
                "LINK_URL_INVALID",
            ),
            (
                json!({"title":"t","starts_on":"2026-10-02","link_url":"https://a b"}),
                "LINK_URL_INVALID",
            ),
        ] {
            assert_eq!(validate(Table::Events, &input, now).unwrap_err(), code);
        }
        let m = json!({"title":"점검","starts_at":"2026-10-10T01:00:00Z","ends_at":"2026-10-10T02:00:00Z","kind":"emergency","end_unknown":true});
        let (_, row) = validate(Table::Maintenance, &m, now).unwrap();
        assert!(matches!(
            row,
            Row::Maintenance {
                kind: "emergency",
                end_unknown: true,
                ..
            }
        ));
        let backwards = json!({"title":"점검","starts_at":"2026-10-10T02:00:00Z","ends_at":"2026-10-10T02:00:00Z"});
        assert_eq!(
            validate(Table::Maintenance, &backwards, now).unwrap_err(),
            "DATES_INVALID"
        );
        let kind = json!({"title":"점검","starts_at":"2026-10-10T01:00:00Z","ends_at":"2026-10-10T02:00:00Z","kind":"other"});
        assert_eq!(
            validate(Table::Maintenance, &kind, now).unwrap_err(),
            "INVALID_INPUT"
        );
    }

    #[test]
    fn status_picks_active_and_72h_upcoming() {
        let row = |id: &str, s: &str, e: &str, p: &str| json!({"id":id,"starts_at":s,"ends_at":e,"published_at":p,"title":id});
        let rows = vec![
            row(
                "later",
                "2026-10-14T00:00:00Z",
                "2026-10-14T01:00:00Z",
                "2026-10-01T00:00:00Z",
            ),
            row(
                "soon",
                "2026-10-11T00:00:00Z",
                "2026-10-11T01:00:00Z",
                "2026-10-01T00:00:00Z",
            ),
            row(
                "hidden",
                "2026-10-10T11:00:00Z",
                "2026-10-10T13:00:00Z",
                "2026-10-11T00:00:00Z",
            ),
            row(
                "now",
                "2026-10-10T11:00:00Z",
                "2026-10-10T13:00:00Z",
                "2026-10-01T00:00:00Z",
            ),
        ];
        let st = pick_status(&rows, t("2026-10-10T12:00:00Z"));
        assert_eq!(st["maintenance"]["active"]["id"], "now");
        assert_eq!(st["maintenance"]["upcoming"]["id"], "soon");
        assert_eq!(st["now"], "2026-10-10T12:00:00Z");
        let none = pick_status(&rows, t("2026-10-20T00:00:00Z"));
        assert!(
            none["maintenance"]["active"].is_null() && none["maintenance"]["upcoming"].is_null()
        );
    }

    #[test]
    fn cli_time_reads_kst_and_utc() {
        assert_eq!(
            cli_time("2026-10-23 02:00").unwrap(),
            "2026-10-22T17:00:00Z"
        );
        assert_eq!(
            cli_time("2026-10-23T02:30").unwrap(),
            "2026-10-22T17:30:00Z"
        );
        assert_eq!(
            cli_time(" 2026-10-22T17:00:00Z ").unwrap(),
            "2026-10-22T17:00:00Z"
        );
        for bad in ["", "내일 2시", "2026-13-01 02:00", "2026-10-23"] {
            assert!(cli_time(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn emergency_switch_and_open_paths() {
        let now = t("2026-10-10T00:00:00Z");
        assert!(env_maintenance("off", "", "", "", now).is_none());
        assert!(env_maintenance("", "", "", "", now).is_none());
        let m = env_maintenance("on", "", "DB 작업", "2026-10-10T03:00:00Z", now).unwrap();
        assert_eq!(m["title"], "긴급 서버 점검");
        assert_eq!(m["ends_at"], "2026-10-10T03:00:00Z");
        assert_eq!(m["end_unknown"], false);
        let open = env_maintenance("1", "점검", "", "bad", now).unwrap();
        assert_eq!(open["end_unknown"], true);
        assert_eq!(open["ends_at"], "9999-12-31T00:00:00Z");

        for (m, p) in [
            (Method::GET, "/api/me"),
            (Method::POST, "/api/auth/login"),
            (Method::GET, "/api/status"),
            (Method::GET, "/api/notices/n-1"),
            (Method::PUT, "/api/content/maintenance/m-1"),
            (Method::GET, "/ready"),
        ] {
            assert!(allowed_during_maintenance(&m, p), "{m} {p}");
        }
        for (m, p) in [
            (Method::POST, "/api/orgs"),
            (Method::GET, "/api/staff/releases"),
            (Method::POST, "/api/me"),
            (Method::GET, "/api/noticesx"),
        ] {
            assert!(!allowed_during_maintenance(&m, p), "{m} {p}");
        }
        assert_eq!(event_status("2026-10-11", None, "2026-10-10"), "upcoming");
        assert_eq!(
            event_status("2026-10-01", Some("2026-10-10"), "2026-10-10"),
            "ongoing"
        );
        assert_eq!(event_status("2026-10-01", None, "2026-10-10"), "ended");
        assert_eq!(today_kst(t("2026-10-10T15:00:00Z")), "2026-10-11");
    }
}
