//! 설문 접수 (예전 web/survey/src/worker.js + D1, 검증 규칙 동일)
//!
//!   GET  /api/survey/config     공개 Turnstile 사이트 키와 접수 가능 여부
//!   POST /api/survey/responses  응답 저장 (Turnstile 확인 후, 같은 submission_id는 한 번만)
//!
//! 설문 화면(survey.audeniq.com)의 `/api/config`, `/api/responses`를 CloudFront 함수가 이 경로로 바꾼다.
//! 설정: TURNSTILE_SITE_KEY, TURNSTILE_SECRET_KEY, SURVEY_ORIGIN (crate::sites)
//! 질문 정의는 web/survey/questions.json의 사본(survey_questions.json)이다 — 테스트가 같은지 확인한다.
use crate::api::AppState;
use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::{collections::BTreeMap, sync::OnceLock, time::Duration};
use uuid::Uuid;

const MAX_BYTES: usize = 32 * 1024;
const SURVEY_VERSION: i32 = 4;

#[derive(Deserialize)]
struct Question {
    #[serde(rename = "type")]
    kind: String,
    required: bool,
    max: Option<usize>,
    options: Vec<String>,
}

fn questions() -> &'static BTreeMap<String, Question> {
    static Q: OnceLock<BTreeMap<String, Question>> = OnceLock::new();
    Q.get_or_init(|| {
        serde_json::from_str(include_str!("survey_questions.json")).expect("survey questions")
    })
}

fn reply(status: StatusCode, body: Value) -> Response {
    let mut r = (status, Json(body)).into_response();
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        "x-robots-tag",
        HeaderValue::from_static("noindex, nofollow"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    r
}
fn err(message: &str, status: StatusCode) -> Response {
    reply(status, json!({"ok": false, "error": message}))
}
const INVALID: &str = "입력 내용을 다시 확인해 주세요.";

fn site_key() -> Option<String> {
    std::env::var("TURNSTILE_SITE_KEY")
        .ok()
        .filter(|k| k.len() > 5 && !k.contains("REPLACE_WITH"))
}
fn secret_key() -> Option<String> {
    std::env::var("TURNSTILE_SECRET_KEY")
        .ok()
        .filter(|k| !k.is_empty())
}
fn ready() -> bool {
    site_key().is_some() && secret_key().is_some() && crate::sites::survey_origin().is_some()
}

/// JS의 String.length (UTF-16 단위)
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}
fn bounded(v: Option<&Value>, n: usize) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| js_len(s) <= n)
}
fn index_value(s: &str, options: usize) -> bool {
    (1..=2).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<usize>().is_ok_and(|n| n < options)
}
fn has(v: Option<&Value>, x: &str) -> bool {
    v.and_then(Value::as_array)
        .is_some_and(|a| a.iter().any(|e| e.as_str() == Some(x)))
}
fn is_uuid_v4(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            14 => *c == b'4',
            19 => matches!(c.to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b'),
            _ => c.is_ascii_hexdigit(),
        })
}
fn valid_contact(c: &str) -> bool {
    if let Some(handle) = c.strip_prefix('@') {
        return (1..=30).contains(&handle.len())
            && handle
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_');
    }
    if c.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = c.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let chars: Vec<char> = domain.chars().collect();
    !local.is_empty()
        && chars
            .iter()
            .enumerate()
            .any(|(i, ch)| *ch == '.' && i > 0 && i + 1 < chars.len())
}

#[derive(Debug, PartialEq)]
pub struct Submission {
    pub id: Uuid,
    pub answers: Value,
    pub beta: String,
    pub contact: Option<String>,
    pub beta_consent: bool,
    pub turnstile_token: String,
}

fn optional_other(
    answers: &Map<String, Value>,
    key: &str,
    chosen: Option<&Value>,
    q: &Question,
) -> bool {
    let Some(value) = bounded(answers.get(&format!("{key}_other")), 160) else {
        return false;
    };
    let Some(other) = q.options.iter().position(|o| o == "기타") else {
        return true;
    };
    let other = other.to_string();
    let has_other = if q.kind == "multi" {
        has(chosen, &other)
    } else {
        chosen.and_then(Value::as_str) == Some(other.as_str())
    };
    has_other == !value.trim().is_empty()
}

/// 제출 검증 (web/survey/src/worker.js validateSubmission과 같은 규칙). 맞지 않으면 None.
pub fn validate(p: &Value) -> Option<Submission> {
    const TOP: [&str; 7] = [
        "submission_id",
        "answers",
        "beta",
        "contact",
        "beta_contact_consent",
        "survey_consent",
        "turnstile_token",
    ];
    let qs = questions();
    let p = p.as_object()?;
    if p.keys().any(|k| !TOP.contains(&k.as_str())) {
        return None;
    }
    let id = p.get("submission_id")?.as_str().filter(|s| is_uuid_v4(s))?;
    let answers = p.get("answers")?.as_object()?;
    let allowed = |k: &str| {
        qs.contains_key(k)
            || k == "q4_provider"
            || k == "q14"
            || k.strip_suffix("_other")
                .and_then(|base| qs.get(base))
                .is_some_and(|q| q.options.iter().any(|o| o == "기타"))
    };
    if answers.keys().any(|k| !allowed(k)) {
        return None;
    }
    let q2 = answers.get("q2").and_then(Value::as_str);
    let no_experience = matches!(q2, Some("2" | "3" | "4")) || has(answers.get("q4"), "9");
    for (key, q) in qs {
        let selection = answers.get(key);
        let skipped = no_experience && matches!(key.as_str(), "q5" | "q6" | "q12");
        if q.kind == "multi" {
            let list = selection?.as_array()?;
            if skipped && !list.is_empty() {
                return None;
            }
            if list.len() > q.max.unwrap_or(q.options.len())
                || (q.required && !skipped && list.is_empty())
            {
                return None;
            }
            let mut seen = std::collections::HashSet::new();
            for v in list {
                let v = v.as_str()?;
                if !index_value(v, q.options.len()) || !seen.insert(v) {
                    return None;
                }
            }
            let exclusive = match key.as_str() {
                "q4" => Some("9"),
                "q6" => Some("18"),
                "q15" => Some("13"),
                _ => None,
            };
            if let Some(x) = exclusive
                && has(selection, x)
                && list.len() != 1
            {
                return None;
            }
        } else {
            let s = bounded(selection, 4)?;
            if skipped && !s.is_empty() {
                return None;
            }
            if s.is_empty() && q.required && !skipped {
                return None;
            }
            if !s.is_empty() && !index_value(s, q.options.len()) {
                return None;
            }
        }
        if q.options.iter().any(|o| o == "기타") && !optional_other(answers, key, selection, q) {
            return None;
        }
    }
    let provider = bounded(answers.get("q4_provider"), 160)?;
    bounded(answers.get("q14"), 2000)?;
    if has(answers.get("q4"), "9") && !provider.trim().is_empty() {
        return None;
    }
    let q3 = answers.get("q3").and_then(Value::as_str);
    if matches!(q2, Some("2" | "3" | "4")) && q3 != Some("6") {
        return None;
    }
    if matches!(q2, Some("0" | "1")) && q3 == Some("6") {
        return None;
    }
    let beta = p
        .get("beta")?
        .as_str()
        .filter(|b| matches!(*b, "" | "0" | "1" | "2" | "3"))?;
    let contact = bounded(p.get("contact"), 160)?.trim();
    if !contact.is_empty() && (beta == "3" || beta.is_empty()) {
        return None;
    }
    if !contact.is_empty() && !valid_contact(contact) {
        return None;
    }
    let consent = p.get("beta_contact_consent")?.as_bool()?;
    if consent != !contact.is_empty() {
        return None;
    }
    if p.get("survey_consent") != Some(&Value::Bool(true)) {
        return None;
    }
    let token = bounded(p.get("turnstile_token"), 2048).filter(|t| !t.is_empty())?;
    Some(Submission {
        id: Uuid::parse_str(&id.to_lowercase()).ok()?,
        answers: Value::Object(answers.clone()),
        beta: beta.to_owned(),
        contact: (!contact.is_empty()).then(|| contact.to_owned()),
        beta_consent: consent,
        turnstile_token: token.to_owned(),
    })
}

async fn verify_turnstile(token: &str, secret: &str, hostname: &str) -> bool {
    let client = match crate::external_http::client_builder(false)
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let form = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("secret", secret)
        .append_pair("response", token)
        .finish();
    let res = client
        .post("https://challenges.cloudflare.com/turnstile/v0/siteverify")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await;
    let Ok(res) = res else { return false };
    if !res.status().is_success() {
        return false;
    }
    let Ok(v) = res.json::<Value>().await else {
        return false;
    };
    v["success"] == true && v["hostname"] == hostname && v["action"] == "audeniq_survey"
}

async fn config() -> Response {
    reply(
        StatusCode::OK,
        json!({"turnstileSitekey": site_key().unwrap_or_default(), "ready": ready()}),
    )
}

async fn submit(State(s): State<AppState>, h: HeaderMap, body: Bytes) -> Response {
    let json_type = h
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| {
            let t = t.to_ascii_lowercase();
            t == "application/json"
                || t.starts_with("application/json;")
                || t.starts_with("application/json ")
        });
    if !json_type {
        return err("JSON 형식으로 제출해 주세요.", StatusCode::BAD_REQUEST);
    }
    if body.len() > MAX_BYTES {
        return err(
            "제출 크기 제한을 초과했습니다.",
            StatusCode::PAYLOAD_TOO_LARGE,
        );
    }
    let (Some(secret), Some(origin)) = (secret_key(), crate::sites::survey_origin()) else {
        return err(
            "설문 접수를 준비하고 있어요. 설정을 확인해 주세요.",
            StatusCode::SERVICE_UNAVAILABLE,
        );
    };
    if site_key().is_none() {
        return err(
            "설문 접수를 준비하고 있어요. 설정을 확인해 주세요.",
            StatusCode::SERVICE_UNAVAILABLE,
        );
    }
    let Some(item) = std::str::from_utf8(&body)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
        .as_ref()
        .and_then(validate)
    else {
        return err(INVALID, StatusCode::BAD_REQUEST);
    };
    let receipt = item.id.to_string();
    let existing: Result<Option<Uuid>, _> =
        sqlx::query_scalar("SELECT id FROM content.survey_responses WHERE id=$1")
            .bind(item.id)
            .fetch_optional(&s.pool)
            .await;
    match existing {
        Ok(Some(_)) => {
            return reply(
                StatusCode::OK,
                json!({"ok":true,"receipt":receipt,"duplicate":true}),
            );
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(error = %e, "survey lookup failed");
            return err(
                "일시적인 오류가 발생했어요. 다시 시도해 주세요.",
                StatusCode::INTERNAL_SERVER_ERROR,
            );
        }
    }
    let hostname = url::Url::parse(origin)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    if !verify_turnstile(&item.turnstile_token, &secret, &hostname).await {
        return err(
            "보안 확인이 만료됐어요. 다시 확인해 주세요.",
            StatusCode::FORBIDDEN,
        );
    }
    let write = sqlx::query(
        "INSERT INTO content.survey_responses(id,survey_version,answers,beta_preference,contact,beta_contact_consent,survey_consent) \
         VALUES($1,$2,$3,$4,$5,$6,true) ON CONFLICT (id) DO NOTHING",
    )
    .bind(item.id)
    .bind(SURVEY_VERSION)
    .bind(&item.answers)
    .bind(&item.beta)
    .bind(&item.contact)
    .bind(item.beta_consent)
    .execute(&s.pool)
    .await;
    match write {
        Ok(r) => reply(
            StatusCode::OK,
            json!({"ok":true,"receipt":receipt,"duplicate":r.rows_affected()==0}),
        ),
        Err(e) => {
            tracing::warn!(error = %e, "survey insert failed");
            err(
                "일시적인 오류가 발생했어요. 다시 시도해 주세요.",
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/survey/config", get(config))
        .route("/api/survey/responses", post(submit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> Value {
        let qs = questions();
        let mut answers = Map::new();
        for (k, q) in qs {
            let v = if q.kind == "multi" {
                json!(["0"])
            } else {
                json!("0")
            };
            answers.insert(k.clone(), v);
            if q.options.iter().any(|o| o == "기타") {
                answers.insert(format!("{k}_other"), json!(""));
            }
        }
        answers.insert("q4_provider".into(), json!(""));
        answers.insert("q14".into(), json!(""));
        json!({"submission_id":"ccb1d9d3-65de-4a58-96d7-4fe041109895","answers":answers,"beta":"","contact":"",
               "beta_contact_consent":false,"survey_consent":true,"turnstile_token":"t"})
    }

    #[test]
    fn questions_copy_matches_the_survey_app() {
        let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/survey/questions.json");
        if let Ok(text) = std::fs::read_to_string(app) {
            assert_eq!(
                serde_json::from_str::<Value>(&text).unwrap(),
                serde_json::from_str::<Value>(include_str!("survey_questions.json")).unwrap(),
                "crates/core/src/survey_questions.json 을 web/survey/questions.json 과 맞추세요"
            );
        }
    }

    #[test]
    fn a_complete_answer_is_accepted() {
        let s = validate(&valid()).expect("valid submission");
        assert_eq!(s.id.to_string(), "ccb1d9d3-65de-4a58-96d7-4fe041109895");
        assert_eq!(s.contact, None);
        assert!(!s.beta_consent);
    }

    #[test]
    fn invalid_submissions_are_rejected() {
        type Change = Box<dyn Fn(&mut Value)>;
        let cases: Vec<Change> = vec![
            Box::new(|v| v["extra"] = json!(1)),
            Box::new(|v| v["submission_id"] = json!("not-a-uuid")),
            Box::new(|v| v["answers"]["unknown"] = json!("1")),
            Box::new(|v| v["answers"]["q1"] = json!([])),
            Box::new(|v| v["answers"]["q1"] = json!(["0", "0"])),
            Box::new(|v| v["answers"]["q1"] = json!(["99"])),
            Box::new(|v| v["answers"]["q2"] = json!("")),
            Box::new(|v| v["answers"]["q4"] = json!(["9", "0"])),
            Box::new(|v| v["survey_consent"] = json!(false)),
            Box::new(|v| v["turnstile_token"] = json!("")),
            Box::new(|v| {
                v["contact"] = json!("a@b.co");
                v["beta"] = json!("3");
                v["beta_contact_consent"] = json!(true);
            }),
            Box::new(|v| {
                v["contact"] = json!("not an email");
                v["beta"] = json!("0");
                v["beta_contact_consent"] = json!(true);
            }),
            Box::new(|v| {
                v["contact"] = json!("a@b.co");
                v["beta"] = json!("0");
            }),
        ];
        for (i, change) in cases.iter().enumerate() {
            let mut v = valid();
            change(&mut v);
            assert!(validate(&v).is_none(), "case {i}");
        }
        let mut ok = valid();
        ok["contact"] = json!("@artist_1");
        ok["beta"] = json!("0");
        ok["beta_contact_consent"] = json!(true);
        assert_eq!(validate(&ok).unwrap().contact.as_deref(), Some("@artist_1"));
    }

    #[test]
    fn contacts_follow_the_worker_pattern() {
        for c in ["a@b.co", "x.y@mail.example.com", "@a", "@a.b_c"] {
            assert!(valid_contact(c), "{c}");
        }
        for c in ["a@b", "a@.b", "a@b.", "a@@b.c", "a b@c.d", "@", "@a-b"] {
            assert!(!valid_contact(c), "{c}");
        }
    }
}
