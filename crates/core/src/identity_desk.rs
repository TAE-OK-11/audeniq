//! 직원 실명인증: 현장이나 고객센터에서 신분증을 직접 보고 계정의 실명·생년월일을
//! 확인한다. 인증 기관(PASS 등)을 쓸 수 없는 사용자를 위한 것이다.
//!
//! - `POST /api/staff/identity/lookup {email}`: 가입 이메일로 계정을 찾는다. 이메일은 주소창·
//!   접근 기록에 남지 않게 본문으로 받는다. 응답의 이메일·실명은 가린다.
//! - `POST /api/staff/identity/{user}/verify`: 신분증의 실명·생년월일, 확인한 곳(현장·고객센터),
//!   신분증 종류를 저장한다. 신분증 번호는 받지 않는다.
//!
//! 규칙
//! - ADMIN·SUPPORT만 (`Duty::Identity`). 직원 세션 보호(2단계 인증·유휴 만료)와 접근 기록은
//!   [`staff::staff`]가 한다. 자기 계정은 인증할 수 없다.
//! - 실명·생년월일은 본인인증과 같은 키로 봉인해 `portal.account_identities`에 둔다
//!   (provider `OFFLINE`, method = 확인한 곳, 연계정보 없음).
//! - 이미 인증된 계정: 신분증 정보가 저장된 것과 같아야 한다. 인증 기관으로 인증한 계정은
//!   그 기록(연계정보)을 그대로 두고 확인 기록만 남긴다. 직원 인증을 잘못 입력했으면 ADMIN만
//!   `correction`으로 바로잡을 수 있고, 그때는 사유(note)가 필요하다.
//! - 처리할 때마다 `portal.offline_identity_checks`에 한 줄 쌓고(고치거나 지우지 않는다),
//!   감사 기록과 사용자 알림을 남긴다.
use crate::{
    account::{self, StoredIdentity},
    api::AppState,
    error::{Error, Result},
    operations,
    payout_keys::Field,
    print_stamp::{mask_email, mask_name},
    staff::{self, Duty, StaffRole},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::post,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

pub const PROVIDER: &str = "OFFLINE";
/// 확인한 곳
pub const CHANNELS: [&str; 2] = ["IN_PERSON", "CUSTOMER_CENTER"];
/// 법령상 신분증 (주민등록증·운전면허증·여권·외국인등록증·장애인등록증·국가보훈등록증)
pub const DOCUMENT_TYPES: [&str; 6] = [
    "RESIDENT_CARD",
    "DRIVER_LICENSE",
    "PASSPORT",
    "ALIEN_REGISTRATION",
    "DISABILITY_CARD",
    "VETERAN_CARD",
];
const NOTE_MAX: usize = 500;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupInput {
    pub email: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyInput {
    pub real_name: String,
    /// YYYY-MM-DD
    pub birth_date: String,
    pub channel: String,
    pub document_type: String,
    #[serde(default)]
    pub note: String,
    /// ADMIN: 이전 직원 실명인증을 다른 실명·생년월일로 바로잡는다.
    #[serde(default)]
    pub correction: bool,
}

/// 신분증에 적힌 실명: 앞뒤 공백을 빼고 1~120자, 제어 문자·연속 공백 없음.
pub fn clean_name(raw: &str) -> Result<String> {
    let name = raw.trim();
    let n = name.chars().count();
    if n == 0 || n > 120 || name.chars().any(char::is_control) || name.contains("  ") {
        return Err(Error::InvalidCode("REAL_NAME_INVALID"));
    }
    Ok(name.to_string())
}

/// 생년월일: 실제 있는 날짜, 오늘 이전, 130년 이내.
pub fn parse_birth(raw: &str, today: NaiveDate) -> Result<NaiveDate> {
    let d = NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|_| Error::InvalidCode("BIRTH_DATE_INVALID"))?;
    if d >= today || account::age_on(d, today) > 130 {
        return Err(Error::InvalidCode("BIRTH_DATE_INVALID"));
    }
    Ok(d)
}

async fn desk(s: &AppState, h: &HeaderMap) -> Result<staff::Staff> {
    let st = staff::staff(s, h, true).await?;
    staff::require(&st, Duty::Identity)?;
    Ok(st)
}

fn identity_json(i: &StoredIdentity) -> Value {
    json!({
        "provider": i.provider,
        "method": i.method,
        "offline": i.provider == PROVIDER,
        "verified_at": i.verified_at,
        "name_masked": mask_name(&i.name),
        "adult": account::is_adult(i.birth),
    })
}

async fn summary(s: &AppState, user: Uuid) -> Result<Value> {
    let mut tx = s.pool.begin().await?;
    let row = sqlx::query("SELECT status, created_at FROM identity.users WHERE id=$1")
        .bind(user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    let email = crate::pii::user_email(&s.payout_keys, &mut *tx, user).await?;
    let identity = account::stored_identity(s, &mut tx, user).await?;
    let checks: Vec<Value> = sqlx::query(
        "SELECT c.id, c.staff_user_id, c.channel, c.document_type, c.correction, c.note, c.created_at
           FROM portal.offline_identity_checks c WHERE c.user_id=$1 ORDER BY c.created_at DESC LIMIT 20",
    )
    .bind(user)
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": r.get::<Uuid, _>("id"),
            "staff_user_id": r.get::<Uuid, _>("staff_user_id"),
            "channel": r.get::<String, _>("channel"),
            "document_type": r.get::<String, _>("document_type"),
            "correction": r.get::<bool, _>("correction"),
            "note": r.get::<String, _>("note"),
            "created_at": r.get::<DateTime<Utc>, _>("created_at"),
        })
    })
    .collect();
    tx.commit().await?;
    Ok(json!({
        "user_id": user,
        "email_masked": mask_email(&email),
        "status": row.get::<String, _>("status"),
        "joined_at": row.get::<DateTime<Utc>, _>("created_at"),
        "identity": identity.as_ref().map(identity_json),
        "checks": checks,
    }))
}

pub async fn lookup(s: &AppState, h: &HeaderMap, i: LookupInput) -> Result<Value> {
    let st = desk(s, h).await?;
    let email = i.email.trim();
    if email.is_empty() || email.len() > 254 || !email.contains('@') {
        return Err(Error::Invalid);
    }
    // 이메일로 계정이 있는지 알아내는 것을 막는다 (직원 계정이 털린 경우)
    crate::auth::rate(&s.pool, &format!("identity-desk:{}", st.actor.user), 60).await?;
    let user = crate::pii::user_by_email(&s.payout_keys, &s.pool, email)
        .await?
        .ok_or(Error::NotFound)?;
    summary(s, user).await
}

pub async fn verify(s: &AppState, h: &HeaderMap, user: Uuid, i: VerifyInput) -> Result<Value> {
    let st = desk(s, h).await?;
    if st.actor.user == user {
        return Err(Error::InvalidCode("IDENTITY_SELF_VERIFY"));
    }
    let name = clean_name(&i.real_name)?;
    let birth = parse_birth(&i.birth_date, crate::domain::today_kst())?;
    if !CHANNELS.contains(&i.channel.as_str())
        || !DOCUMENT_TYPES.contains(&i.document_type.as_str())
    {
        return Err(Error::Invalid);
    }
    let note = i.note.trim();
    staff::note_ok(note, NOTE_MAX)?;
    if i.correction && st.role != StaffRole::Admin {
        return Err(Error::Forbidden);
    }
    if i.correction && note.is_empty() {
        return Err(Error::InvalidCode("NOTE_REQUIRED"));
    }
    crate::auth::rate(&s.pool, &format!("identity-desk:{}", st.actor.user), 60).await?;

    let mut tx = s.pool.begin().await?;
    let status: String = sqlx::query_scalar("SELECT status FROM identity.users WHERE id=$1")
        .bind(user)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
    if status != "ACTIVE" {
        return Err(Error::InvalidCode("ACCOUNT_NOT_ACTIVE"));
    }
    let previous = account::stored_identity(s, &mut tx, user).await?;
    // 이미 인증된 사람과 다른 사람이면 거절: 바로잡기는 직원 인증에만, ADMIN만.
    let write = match &previous {
        None => true,
        Some(p) if p.same_person(&name, birth) => p.provider == PROVIDER,
        Some(p) if p.provider == PROVIDER && i.correction => true,
        Some(_) => return Err(Error::InvalidCode("IDENTITY_ACCOUNT_MISMATCH")),
    };
    if i.correction
        && previous
            .as_ref()
            .is_none_or(|p| p.provider != PROVIDER || p.same_person(&name, birth))
    {
        // 바로잡을 것이 없다 (처음 인증이거나, 인증 기관 기록이거나, 같은 내용)
        return Err(Error::InvalidCode("NOTHING_TO_CORRECT"));
    }
    let check = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO portal.offline_identity_checks(id,user_id,staff_user_id,channel,document_type,correction,note)
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(check)
    .bind(user)
    .bind(st.actor.user)
    .bind(&i.channel)
    .bind(&i.document_type)
    .bind(i.correction)
    .bind(note)
    .execute(&mut *tx)
    .await?;
    if write {
        let name_sealed = s
            .payout_keys
            .seal_field(Field::IdentityRealName, user, &name)?;
        let birth_sealed = s.payout_keys.seal_field(
            Field::IdentityBirthDate,
            user,
            &birth.format("%Y-%m-%d").to_string(),
        )?;
        sqlx::query(
            "INSERT INTO portal.account_identities(user_id,real_name_cipher,birth_date_cipher,provider,method,transaction_id,ci_hash)
             VALUES($1,$2,$3,$4,$5,$6,NULL)
             ON CONFLICT(user_id) DO UPDATE SET real_name=NULL,real_name_cipher=EXCLUDED.real_name_cipher,
               birth_date=NULL,birth_date_cipher=EXCLUDED.birth_date_cipher,provider=EXCLUDED.provider,
               method=EXCLUDED.method,transaction_id=EXCLUDED.transaction_id,ci_hash=NULL,verified_at=now(),
               row_version=portal.account_identities.row_version+1
             WHERE portal.account_identities.provider=$4",
        )
        .bind(user)
        .bind(&name_sealed)
        .bind(&birth_sealed)
        .bind(PROVIDER)
        .bind(&i.channel)
        .bind(check.to_string())
        .execute(&mut *tx)
        .await?;
    }
    let action = if i.correction {
        "portal.account.identity_corrected"
    } else {
        "portal.account.identity_verified"
    };
    operations::audit(
        &mut tx,
        Some(st.actor.user),
        None,
        Some(user),
        action,
        &format!("STAFF_{}:{}", i.channel, i.document_type),
        st.actor.request,
    )
    .await?;
    // 본인 모르게 실명이 등록되지 않도록 알린다 (개인 작업 공간으로).
    sqlx::query("SELECT portal.identity_desk_notice($1,$2,$3)")
    .bind(user)
    .bind(if i.correction { "실명 정보를 바로잡았어요" } else { "실명인증을 마쳤어요" })
    .bind(match i.channel.as_str() {
        "IN_PERSON" => "어데닉 직원이 현장에서 신분증으로 실명을 확인했어요. 내가 한 게 아니라면 고객센터로 알려 주세요.",
        _ => "어데닉 고객센터에서 신분증으로 실명을 확인했어요. 내가 한 게 아니라면 고객센터로 알려 주세요.",
    })
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    summary(s, user).await
}

async fn h_lookup(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<LookupInput>,
) -> Result<Json<Value>> {
    Ok(Json(lookup(&s, &h, i).await?))
}
async fn h_verify(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(user): Path<Uuid>,
    Json(i): Json<VerifyInput>,
) -> Result<Json<Value>> {
    Ok(Json(verify(&s, &h, user, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/staff/identity/lookup", post(h_lookup))
        .route("/api/staff/identity/{user}/verify", post(h_verify))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(clean_name("  홍길동 ").unwrap(), "홍길동");
        assert_eq!(clean_name("John Smith").unwrap(), "John Smith");
        for bad in ["", "   ", "a\u{0}b", "a  b", &"가".repeat(121)] {
            assert!(clean_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn birth_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        assert_eq!(
            parse_birth("1990-02-28", today).unwrap(),
            NaiveDate::from_ymd_opt(1990, 2, 28).unwrap()
        );
        for bad in [
            "1990-02-30",
            "2026-10-19",
            "2030-01-01",
            "1890-01-01",
            "19900101",
            "",
        ] {
            assert!(parse_birth(bad, today).is_err(), "{bad}");
        }
    }
}
