//! 휴대폰 번호 등록과 문자 인증 비밀번호 재설정.
//!
//! 휴대폰 번호 등록 (로그인 상태, 계정 정보):
//! - `POST /api/me/phone/start {phone}` → 인증번호 문자 발송, `{challenge_id, expires_at, phone_last4}`
//! - `POST /api/me/phone/verify {challenge_id, code}` → 계정에 휴대폰 번호(해시·뒷자리) 저장
//!
//! 비밀번호 재설정 (로그인 없이):
//! 1. `POST /api/auth/password-reset/start {email, phone}` → 이메일 계정에 등록된 휴대폰 번호와
//!    같을 때만 인증번호를 보낸다. 계정이 없거나 번호가 달라도 같은 모양으로 답한다(가입 여부 비노출).
//! 2. `POST /api/auth/password-reset/verify {reset_id, code}` → 15분짜리 일회용 재설정 토큰.
//!    틀린 번호·만료·5회 초과·없는 요청은 모두 `RESET_CODE_INVALID` 하나로 답한다.
//! 3. `POST /api/auth/password-reset/complete {reset_token, new_password}` → 비밀번호를 바꾸고
//!    모든 세션을 끊는다. 다시 로그인해야 한다.
//!
//! 인증번호는 3분, 5회까지. 인증번호·토큰은 해시만 보관한다. 문자 발송 대행사 계약 전에는
//! `SMS_PROVIDER_NOT_CONFIGURED`로 거절한다 (`crate::sms`).
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    sms::{self, SmsSender},
};
use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

/// 인증번호 유효 시간 (스튜디오 본인인증 창의 3분 타이머와 같다).
pub const CODE_SECONDS: i64 = 180;
/// 인증번호 입력 기회.
pub const CODE_ATTEMPTS: i32 = 5;
/// 재설정 토큰 유효 시간.
pub const RESET_MINUTES: i64 = 15;

fn code_hash(s: &AppState, challenge: Uuid, code: &str) -> Vec<u8> {
    auth::hash_token(&format!(
        "sms-code\0{}\0{challenge}\0{code}",
        s.config.service_secret
    ))
}

fn message(code: &str) -> String {
    format!("[AUDENIQ] 인증번호 {code}를 3분 안에 입력해 주세요. 요청하지 않았다면 무시해 주세요.")
}

fn valid_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

/// 같은 계정·목적의 이전 인증번호는 새로 요청하면 쓸 수 없게 한다.
async fn new_challenge(
    tx: &mut sqlx::PgConnection,
    s: &AppState,
    purpose: &str,
    user: Uuid,
    phone: &str,
    code: &str,
) -> Result<(Uuid, DateTime<Utc>)> {
    sqlx::query(
        "UPDATE identity.sms_challenges SET consumed_at=now() WHERE user_id=$1 AND purpose=$2 AND consumed_at IS NULL",
    )
    .bind(user)
    .bind(purpose)
    .execute(&mut *tx)
    .await?;
    let id = Uuid::new_v4();
    let expires: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO identity.sms_challenges(id,purpose,user_id,phone_hash,phone_last4,code_hash,expires_at)
         VALUES($1,$2,$3,$4,$5,$6,now()+make_interval(secs=>$7)) RETURNING expires_at",
    )
    .bind(id)
    .bind(purpose)
    .bind(user)
    .bind(sms::phone_hash(&s.config.service_secret, phone))
    .bind(&phone[phone.len() - 4..])
    .bind(code_hash(s, id, code))
    .bind(CODE_SECONDS as f64)
    .fetch_one(&mut *tx)
    .await?;
    Ok((id, expires))
}

/// 인증번호 확인. 맞으면 `verified_at`을 기록하고 true. 틀리면 시도 횟수만 늘린다.
async fn check_code(
    tx: &mut sqlx::PgConnection,
    s: &AppState,
    purpose: &str,
    id: Uuid,
    code: &str,
    user: Option<Uuid>,
) -> Result<Option<(Uuid, String, String)>> {
    let row = sqlx::query(
        "SELECT user_id,phone_hash,phone_last4,code_hash,attempts,expires_at>now() AS live
         FROM identity.sms_challenges
         WHERE id=$1 AND purpose=$2 AND consumed_at IS NULL AND verified_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .bind(purpose)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let owner: Uuid = row.get("user_id");
    if user.is_some_and(|u| u != owner) {
        return Ok(None);
    }
    let attempts: i32 = row.get("attempts");
    if !row.get::<bool, _>("live") || attempts >= CODE_ATTEMPTS {
        return Ok(None);
    }
    let expected: Vec<u8> = row.get("code_hash");
    let ok = valid_code(code)
        && bool::from(subtle::ConstantTimeEq::ct_eq(
            code_hash(s, id, code).as_slice(),
            expected.as_slice(),
        ));
    if !ok {
        sqlx::query("UPDATE identity.sms_challenges SET attempts=attempts+1 WHERE id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        return Ok(None);
    }
    sqlx::query("UPDATE identity.sms_challenges SET verified_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    Ok(Some((owner, row.get("phone_hash"), row.get("phone_last4"))))
}

// ---------------------------------------------------------------------------
// 휴대폰 번호 등록
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhoneStart {
    pub phone: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhoneVerify {
    pub challenge_id: Uuid,
    pub code: String,
}

pub async fn start_phone(s: &AppState, a: &Actor, i: PhoneStart) -> Result<Value> {
    let sender = SmsSender::from_env();
    if !sender.ready() {
        return Err(Error::PolicyGate("SMS_PROVIDER_NOT_CONFIGURED"));
    }
    let phone = sms::normalize_phone(&i.phone)?;
    auth::rate(&s.pool, &format!("phone-start:user:{}", a.user), 5).await?;
    auth::rate(&s.pool, &format!("phone-start:phone:{phone}"), 5).await?;
    let code = sms::new_code();
    let mut tx = s.pool.begin().await?;
    let (id, expires) = new_challenge(&mut tx, s, "PHONE_REGISTER", a.user, &phone, &code).await?;
    tx.commit().await?;
    sender.send(&phone, &message(&code)).await?;
    Ok(json!({"challenge_id": id, "expires_at": expires, "phone_last4": &phone[phone.len() - 4..]}))
}

pub async fn verify_phone(s: &AppState, a: &Actor, i: PhoneVerify) -> Result<Value> {
    auth::rate(&s.pool, &format!("phone-verify:user:{}", a.user), 20).await?;
    let mut tx = s.pool.begin().await?;
    let checked = check_code(
        &mut tx,
        s,
        "PHONE_REGISTER",
        i.challenge_id,
        &i.code,
        Some(a.user),
    )
    .await?;
    let Some((_, hash, last4)) = checked else {
        // 틀린 시도 횟수는 남긴다
        tx.commit().await?;
        return Err(Error::InvalidCode("SMS_CODE_INVALID"));
    };
    sqlx::query("UPDATE identity.sms_challenges SET consumed_at=now() WHERE id=$1")
        .bind(i.challenge_id)
        .execute(&mut *tx)
        .await?;
    let verified_at: DateTime<Utc> = sqlx::query_scalar(
        "INSERT INTO portal.account_phones(user_id,phone_hash,phone_last4) VALUES($1,$2,$3)
         ON CONFLICT(user_id) DO UPDATE SET phone_hash=EXCLUDED.phone_hash,phone_last4=EXCLUDED.phone_last4,
           verified_at=now(),row_version=portal.account_phones.row_version+1
         RETURNING verified_at",
    )
    .bind(a.user)
    .bind(&hash)
    .bind(&last4)
    .fetch_one(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "portal.account.phone_verified",
        "USER_VERIFY",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"phone": {"last4": last4, "verified_at": verified_at}}))
}

// ---------------------------------------------------------------------------
// 비밀번호 재설정
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetStart {
    pub email: String,
    pub phone: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetVerify {
    pub reset_id: Uuid,
    pub code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetComplete {
    pub reset_token: String,
    pub new_password: String,
}

impl Drop for ResetComplete {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.new_password);
    }
}

/// 재설정 시도 제한의 출처 키 (IP를 모르면 모두 같은 출처, 한도는 넉넉하게).
fn source(h: &HeaderMap) -> (String, i32) {
    match auth::client_ip_key(h) {
        Some(ip) => (ip, 1),
        None => ("unknown".into(), auth::UNKNOWN_SOURCE_FACTOR),
    }
}

pub async fn start_reset(s: &AppState, h: &HeaderMap, i: ResetStart) -> Result<Value> {
    auth::origin(h, &s.config)?;
    let sender = SmsSender::from_env();
    if !sender.ready() {
        return Err(Error::PolicyGate("SMS_PROVIDER_NOT_CONFIGURED"));
    }
    let email = i.email.trim().to_lowercase();
    if email.len() > 254 || !email.contains('@') || email.chars().any(char::is_whitespace) {
        return Err(Error::Invalid);
    }
    let phone = sms::normalize_phone(&i.phone)?;
    let (src, factor) = source(h);
    auth::rate(&s.pool, &format!("reset-start:src:{src}"), 10 * factor).await?;
    auth::rate(&s.pool, &format!("reset-start:acct:{email}"), 5).await?;
    auth::rate(&s.pool, &format!("reset-start:phone:{phone}"), 5).await?;
    let hash = sms::phone_hash(&s.config.service_secret, &phone);
    let user: Option<Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM identity.users u JOIN portal.account_phones p ON p.user_id=u.id
         WHERE (u.email=$1 OR u.email_bidx=ANY($3)) AND u.status='ACTIVE' AND p.phone_hash=$2",
    )
    .bind(&email)
    .bind(&hash)
    .bind(crate::pii::email_indexes(&s.payout_keys, &email))
    .fetch_optional(&s.pool)
    .await?;
    let Some(user) = user else {
        // 가입 여부·등록 번호를 알리지 않는다: 같은 모양의 응답, 아무 문자도 보내지 않는다.
        return Ok(json!({
            "reset_id": Uuid::new_v4(),
            "expires_at": Utc::now() + Duration::seconds(CODE_SECONDS),
            "phone_last4": &phone[phone.len() - 4..],
        }));
    };
    let code = sms::new_code();
    let mut tx = s.pool.begin().await?;
    let (id, expires) = new_challenge(&mut tx, s, "PASSWORD_RESET", user, &phone, &code).await?;
    operations::audit(
        &mut tx,
        Some(user),
        None,
        Some(user),
        "auth.password_reset_requested",
        "SMS_SENT",
        auth::request_id(h),
    )
    .await?;
    tx.commit().await?;
    sender.send(&phone, &message(&code)).await?;
    Ok(json!({"reset_id": id, "expires_at": expires, "phone_last4": &phone[phone.len() - 4..]}))
}

pub async fn verify_reset(s: &AppState, h: &HeaderMap, i: ResetVerify) -> Result<Value> {
    auth::origin(h, &s.config)?;
    let (src, factor) = source(h);
    auth::rate(&s.pool, &format!("reset-verify:src:{src}"), 30 * factor).await?;
    let mut tx = s.pool.begin().await?;
    let checked = check_code(&mut tx, s, "PASSWORD_RESET", i.reset_id, &i.code, None).await?;
    let Some((user, _, _)) = checked else {
        tx.commit().await?;
        return Err(Error::InvalidCode("RESET_CODE_INVALID"));
    };
    let token = auth::random_token();
    let expires: DateTime<Utc> = sqlx::query_scalar(
        "UPDATE identity.sms_challenges SET reset_token_hash=$2,reset_expires_at=now()+make_interval(mins=>$3)
         WHERE id=$1 RETURNING reset_expires_at",
    )
    .bind(i.reset_id)
    .bind(auth::hash_token(&token))
    .bind(RESET_MINUTES as i32)
    .fetch_one(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(user),
        None,
        Some(user),
        "auth.password_reset_verified",
        "SMS_CODE_OK",
        auth::request_id(h),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"reset_token": token, "expires_at": expires}))
}

pub async fn complete_reset(s: &AppState, h: &HeaderMap, i: ResetComplete) -> Result<Value> {
    auth::origin(h, &s.config)?;
    if i.reset_token.len() != 64 || !(12..=128).contains(&i.new_password.len()) {
        return Err(Error::Invalid);
    }
    let (src, factor) = source(h);
    auth::rate(&s.pool, &format!("reset-complete:src:{src}"), 30 * factor).await?;
    let token_hash = auth::hash_token(&i.reset_token);
    // 토큰을 먼저 확인해 없는 토큰에 비밀번호 해시(무거운 작업)를 쓰지 않는다.
    let live: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM identity.sms_challenges WHERE reset_token_hash=$1 AND consumed_at IS NULL AND reset_expires_at>now()",
    )
    .bind(&token_hash)
    .fetch_optional(&s.pool)
    .await?;
    let Some(user) = live else {
        return Err(Error::InvalidCode("RESET_TOKEN_INVALID"));
    };
    let email = crate::pii::user_email(&s.payout_keys, &s.pool, user).await?;
    crate::password_policy::check(&i.new_password, &email)?;
    let permit = s
        .password_slots
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Error::Internal)?;
    // The slot stays with the Argon2 work even if the client disconnects.
    let password = i.new_password.clone();
    let (permit, hash) = crate::password_work::run(permit, move || auth::hash_password(password))
        .await
        .map_err(|_| Error::Internal)?;
    drop(permit);
    let hash = hash?;
    let mut tx = s.pool.begin().await?;
    // 같은 토큰으로 두 번 바꿀 수 없다 (동시 요청 포함).
    let user: Option<Uuid> = sqlx::query_scalar(
        "UPDATE identity.sms_challenges c SET consumed_at=now()
         FROM identity.users u
         WHERE c.reset_token_hash=$1 AND c.consumed_at IS NULL AND c.reset_expires_at>now()
           AND u.id=c.user_id AND u.status='ACTIVE'
         RETURNING c.user_id",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *tx)
    .await?;
    let user = user.ok_or(Error::InvalidCode("RESET_TOKEN_INVALID"))?;
    sqlx::query("UPDATE identity.users SET password_hash=$2 WHERE id=$1")
        .bind(user)
        .bind(hash)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE identity.sessions SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?;
    crate::mcp::oauth::revoke_user_grants(&mut tx, user, "PASSWORD_RESET").await?;
    // 남은 다른 재설정 요청도 닫는다.
    sqlx::query(
        "UPDATE identity.sms_challenges SET consumed_at=now() WHERE user_id=$1 AND purpose='PASSWORD_RESET' AND consumed_at IS NULL",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(user),
        None,
        Some(user),
        "auth.password_reset",
        "ALL_SESSIONS_REVOKED",
        auth::request_id(h),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"changed": true, "reauthentication_required": true}))
}

/// 계정 정보 응답에 싣는 등록 휴대폰 (뒷자리·인증 시각).
pub async fn registered_phone(s: &AppState, user: Uuid) -> Result<Value> {
    let row =
        sqlx::query("SELECT phone_last4,verified_at FROM portal.account_phones WHERE user_id=$1")
            .bind(user)
            .fetch_optional(&s.pool)
            .await?;
    Ok(row.map_or(Value::Null, |r| {
        json!({"last4": r.get::<String, _>("phone_last4"), "verified_at": r.get::<DateTime<Utc>, _>("verified_at")})
    }))
}

// ---------------------------------------------------------------------------
// HTTP routes (merged into api::router)
// ---------------------------------------------------------------------------
async fn h_phone_start(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<PhoneStart>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(start_phone(&s, &a, i).await?))
}
async fn h_phone_verify(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<PhoneVerify>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(verify_phone(&s, &a, i).await?))
}
async fn h_reset_start(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ResetStart>,
) -> Result<Json<Value>> {
    Ok(Json(start_reset(&s, &h, i).await?))
}
async fn h_reset_verify(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ResetVerify>,
) -> Result<Json<Value>> {
    Ok(Json(verify_reset(&s, &h, i).await?))
}
async fn h_reset_complete(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<ResetComplete>,
) -> Result<Json<Value>> {
    Ok(Json(complete_reset(&s, &h, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/me/phone/start", post(h_phone_start))
        .route("/api/me/phone/verify", post(h_phone_verify))
        .route("/api/auth/password-reset/start", post(h_reset_start))
        .route("/api/auth/password-reset/verify", post(h_reset_verify))
        .route("/api/auth/password-reset/complete", post(h_reset_complete))
}
