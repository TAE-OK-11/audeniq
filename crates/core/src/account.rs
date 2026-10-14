//! 계정 정보: 본인인증(실명)과 해외 납세 의무. 스튜디오 '계정 정보' 화면과
//! 수령 계좌 등록 흐름이 쓰던 값을 사용자 단위로 서버에 둔다.
//!
//! - `GET /api/me/account`: 실명·인증 방법·인증 시각·성인 여부, 납세 정보
//!   (납세자번호는 뒷자리만), 인증 기관 준비 상태.
//! - `POST /api/me/identity {transaction_id}`: 인증 기관 거래를 서버가 직접
//!   확인한다(`signing::Provider`, 권리 서류 서명과 같은 연동). 브라우저가 보낸
//!   이름은 믿지 않고, 인증 기관이 확인한 이름을 실명으로 저장한다. 이미 인증한
//!   계정은 같은 사람(같은 연계정보)으로만 다시 인증할 수 있다.
//! - `PUT /api/me/tax`: 해외 납세 의무. 납세자번호(TIN)는 수령 계좌와 같은
//!   AES-256-GCM 키로 봉인한다.
//!
//! 인증 기관 계약 전(`IDENTITY_PROVIDER` 미설정)에는 본인인증을 거절한다
//! (`IDENTITY_PROVIDER_NOT_CONFIGURED`). 연계정보(CI)는 해시만, 생년월일은
//! 성인 여부 판단용으로만 보관하고 응답에 싣지 않는다.
use crate::{
    api::AppState,
    auth::{self, Actor},
    domain::sha256_hex,
    error::{Error, Result},
    operations,
    payout_keys::Field,
    signing::Provider,
};
use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::{get, post, put},
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;

/// 해외 납세 국가 (스튜디오 선택지와 같다). `OTHER`는 기타.
pub const TAX_COUNTRIES: [&str; 10] = [
    "US", "JP", "CN", "GB", "CA", "AU", "DE", "FR", "SG", "OTHER",
];
/// 민법상 성년.
const ADULT_AGE: i32 = 19;

/// 만 나이 (생일이 지나야 한 살 더한다).
pub fn age_on(birth: NaiveDate, today: NaiveDate) -> i32 {
    let mut age = today.year() - birth.year();
    if (today.month(), today.day()) < (birth.month(), birth.day()) {
        age -= 1;
    }
    age
}

fn parse_birth(sealed: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(sealed, "%Y-%m-%d").map_err(|_| Error::Internal)
}

fn ci_hash(s: &AppState, ci: &str) -> String {
    // signing.rs의 pepper_hash("ci")와 같은 값: 서명 기록과 대조할 수 있다.
    sha256_hex(format!("ci\0{}\0{ci}", s.config.service_secret))
}

// ---------------------------------------------------------------------------
// 조회
// ---------------------------------------------------------------------------
pub async fn get_account(s: &AppState, a: &Actor) -> Result<Value> {
    let provider = Provider::from_env();
    let identity = sqlx::query(
        "SELECT real_name,real_name_cipher,birth_date,birth_date_cipher,provider,method,verified_at,row_version FROM portal.account_identities WHERE user_id=$1",
    )
    .bind(a.user)
    .fetch_optional(&s.pool)
    .await?;
    let tax = sqlx::query(
        "SELECT foreign_tax,country,tin_last4,us_person,row_version,updated_at FROM portal.tax_residencies WHERE user_id=$1",
    )
    .bind(a.user)
    .fetch_optional(&s.pool)
    .await?;
    let today = crate::domain::today_kst();
    // Sealed (or, before `privacy encrypt-pii`, plaintext) name and birth date.
    let (real_name, birth) = match &identity {
        Some(r) => (
            crate::pii::open(
                &s.payout_keys,
                Field::IdentityRealName,
                a.user,
                r.get("real_name"),
                r.get::<Option<Vec<u8>>, _>("real_name_cipher").as_deref(),
            )?,
            match r.get::<Option<Vec<u8>>, _>("birth_date_cipher") {
                Some(c) => Some(parse_birth(&s.payout_keys.open_field(
                    Field::IdentityBirthDate,
                    a.user,
                    &c,
                )?)?),
                None => r.get::<Option<NaiveDate>, _>("birth_date"),
            },
        ),
        None => (None, None),
    };
    let verified = identity.as_ref().map(|r| {
        json!({
            "provider": r.get::<String, _>("provider"),
            "method": r.get::<String, _>("method"),
            "verified_at": r.get::<DateTime<Utc>, _>("verified_at"),
            "adult": birth.is_some_and(|b| age_on(b, today) >= ADULT_AGE),
        })
    });
    let tax = tax.map(|r| {
        json!({
            "foreign": r.get::<bool, _>("foreign_tax"),
            "country": r.get::<Option<String>, _>("country").unwrap_or_default(),
            "tin_last4": r.get::<Option<String>, _>("tin_last4"),
            "us_person": r.get::<bool, _>("us_person"),
            "row_version": r.get::<i64, _>("row_version"),
            "updated_at": r.get::<DateTime<Utc>, _>("updated_at"),
        })
    });
    let phone = crate::password_reset::registered_phone(s, a.user).await?;
    Ok(json!({
        "phone": phone,
        "sms_ready": crate::sms::SmsSender::from_env().ready(),
        "real_name": real_name.unwrap_or_default(),
        "verified": verified,
        "tax": tax,
        "identity_provider": {"ready": provider.name().is_some(), "name": provider.name()},
    }))
}

// ---------------------------------------------------------------------------
// 본인인증
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityInput {
    pub transaction_id: String,
}

pub async fn verify_identity(s: &AppState, a: &Actor, i: IdentityInput) -> Result<Value> {
    let provider = Provider::from_env();
    if provider.name().is_none() {
        return Err(Error::PolicyGate("IDENTITY_PROVIDER_NOT_CONFIGURED"));
    }
    if i.transaction_id.is_empty() || i.transaction_id.len() > 200 {
        return Err(Error::Invalid);
    }
    auth::rate(&s.pool, &format!("account-identity:{}", a.user), 10).await?;
    let verified = provider.verify(&i.transaction_id).await?;
    let birth = NaiveDate::parse_from_str(&verified.birth_date, "%Y-%m-%d")
        .map_err(|_| Error::InvalidCode("IDENTITY_NOT_VERIFIED"))?;
    let birth_sealed = s.payout_keys.seal_field(
        Field::IdentityBirthDate,
        a.user,
        &birth.format("%Y-%m-%d").to_string(),
    )?;
    let name: String = verified.name.trim().chars().take(120).collect();
    if name.is_empty() {
        return Err(Error::InvalidCode("IDENTITY_NOT_VERIFIED"));
    }
    let name_sealed = s
        .payout_keys
        .seal_field(Field::IdentityRealName, a.user, &name)?;
    let ci = ci_hash(s, &verified.ci);
    let mut tx = s.pool.begin().await?;
    // 같은 인증 거래로 다른 계정(또는 서명 요청)을 인증할 수 없다.
    let reused: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM portal.account_identities WHERE provider=$1 AND transaction_id=$2 AND user_id<>$3)
             OR EXISTS(SELECT 1 FROM portal.signing_requests WHERE identity->>'provider'=$1 AND identity->>'transaction_id'=$2)",
    )
    .bind(verified.provider)
    .bind(&verified.transaction_id)
    .bind(a.user)
    .fetch_one(&mut *tx)
    .await?;
    if reused {
        return Err(Error::InvalidCode("IDENTITY_NOT_VERIFIED"));
    }
    // 계정의 주인은 한 사람: 다시 인증할 때는 같은 사람이어야 한다.
    let previous: Option<String> = sqlx::query_scalar(
        "SELECT ci_hash FROM portal.account_identities WHERE user_id=$1 FOR UPDATE",
    )
    .bind(a.user)
    .fetch_optional(&mut *tx)
    .await?;
    if previous.as_deref().is_some_and(|p| p != ci) {
        return Err(Error::InvalidCode("IDENTITY_ACCOUNT_MISMATCH"));
    }
    let row = sqlx::query(
        "INSERT INTO portal.account_identities(user_id,real_name_cipher,birth_date_cipher,provider,method,transaction_id,ci_hash)
         VALUES($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT(user_id) DO UPDATE SET real_name=NULL,real_name_cipher=EXCLUDED.real_name_cipher,
           birth_date=NULL,birth_date_cipher=EXCLUDED.birth_date_cipher,provider=EXCLUDED.provider,
           method=EXCLUDED.method,transaction_id=EXCLUDED.transaction_id,verified_at=now(),
           row_version=portal.account_identities.row_version+1
         RETURNING verified_at,row_version",
    )
    .bind(a.user)
    .bind(&name_sealed)
    .bind(&birth_sealed)
    .bind(verified.provider)
    .bind(&verified.method)
    .bind(&verified.transaction_id)
    .bind(&ci)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        // 동시에 같은 거래로 두 계정을 인증하려 한 경우
        sqlx::Error::Database(d) if d.is_unique_violation() => {
            Error::InvalidCode("IDENTITY_NOT_VERIFIED")
        }
        e => e.into(),
    })?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "portal.account.identity_verified",
        "USER_VERIFY",
        a.request,
    )
    .await?;
    tx.commit().await?;
    let today = crate::domain::today_kst();
    Ok(json!({
        "real_name": name,
        "verified": {
            "provider": verified.provider,
            "method": verified.method,
            "verified_at": row.get::<DateTime<Utc>, _>("verified_at"),
            "adult": age_on(birth, today) >= ADULT_AGE,
        },
        "row_version": row.get::<i64, _>("row_version"),
    }))
}

// ---------------------------------------------------------------------------
// 해외 납세 의무
// ---------------------------------------------------------------------------
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxInput {
    pub foreign: bool,
    #[serde(default)]
    pub country: String,
    /// 납세자번호. 비우면 지운다. `keep_tin`이면 저장된 번호를 그대로 둔다.
    #[serde(default)]
    pub tin: String,
    #[serde(default)]
    pub keep_tin: bool,
    #[serde(default)]
    pub us_person: bool,
    /// 처음 저장은 0(저장된 행은 1부터), 이후에는 조회한 값 (동시 수정 방지).
    pub row_version: i64,
}

impl Drop for TaxInput {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.tin);
    }
}

/// 검증된 납세 정보: (국가, 납세자번호(공백·하이픈 제거), 미국인 여부).
pub fn validate_tax(i: &TaxInput) -> Result<(Option<String>, Option<String>, bool)> {
    if !i.foreign {
        if !i.country.trim().is_empty() || !i.tin.trim().is_empty() || i.keep_tin || i.us_person {
            return Err(Error::Invalid);
        }
        return Ok((None, None, false));
    }
    let country = i.country.trim().to_ascii_uppercase();
    if !TAX_COUNTRIES.contains(&country.as_str()) {
        return Err(Error::InvalidCode("TAX_COUNTRY_INVALID"));
    }
    if i.us_person && country != "US" {
        return Err(Error::Invalid);
    }
    if i.tin
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == ' '))
    {
        return Err(Error::InvalidCode("TIN_INVALID"));
    }
    let tin: String = i
        .tin
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if !tin.is_empty() && !(4..=30).contains(&tin.len()) {
        return Err(Error::InvalidCode("TIN_INVALID"));
    }
    if !tin.is_empty() && i.keep_tin {
        return Err(Error::Invalid);
    }
    Ok((
        Some(country),
        Some(tin).filter(|t| !t.is_empty()),
        i.us_person,
    ))
}

pub async fn put_tax(s: &AppState, a: &Actor, i: TaxInput) -> Result<Value> {
    let (country, tin, us_person) = validate_tax(&i)?;
    let tin = tin.map(zeroize::Zeroizing::new);
    let sealed = match &tin {
        Some(t) => Some((
            s.payout_keys.seal_field(Field::TaxTin, a.user, t)?,
            t[t.len() - 4..].to_string(),
            s.payout_keys.active_version(),
        )),
        None => None,
    };
    let keep = i.keep_tin;
    let mut tx = s.pool.begin().await?;
    let rv: Option<i64> = if i.row_version == 0 {
        if keep {
            return Err(Error::Invalid);
        }
        sqlx::query_scalar(
            "INSERT INTO portal.tax_residencies(user_id,foreign_tax,country,tin_cipher,tin_last4,key_version,us_person,row_version,tin_field_aad)
             VALUES($1,$2,$3,$4,$5,$6,$7,1,true) ON CONFLICT(user_id) DO NOTHING RETURNING row_version",
        )
        .bind(a.user)
        .bind(i.foreign)
        .bind(&country)
        .bind(sealed.as_ref().map(|t| &t.0))
        .bind(sealed.as_ref().map(|t| &t.1))
        .bind(sealed.as_ref().map(|t| t.2))
        .bind(us_person)
        .fetch_optional(&mut *tx)
        .await?
    } else {
        sqlx::query_scalar(
            "UPDATE portal.tax_residencies SET foreign_tax=$2,country=$3,
               tin_cipher=CASE WHEN $8 THEN tin_cipher ELSE $4 END,
               tin_last4=CASE WHEN $8 THEN tin_last4 ELSE $5 END,
               key_version=CASE WHEN $8 THEN key_version ELSE $6 END,
               tin_field_aad=CASE WHEN $8 THEN tin_field_aad ELSE true END,
               us_person=$7,row_version=row_version+1,updated_at=now()
             WHERE user_id=$1 AND row_version=$9 RETURNING row_version",
        )
        .bind(a.user)
        .bind(i.foreign)
        .bind(&country)
        .bind(sealed.as_ref().map(|t| &t.0))
        .bind(sealed.as_ref().map(|t| &t.1))
        .bind(sealed.as_ref().map(|t| t.2))
        .bind(us_person)
        .bind(keep)
        .bind(i.row_version)
        .fetch_optional(&mut *tx)
        .await?
    };
    let rv = rv.ok_or(Error::Conflict)?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "portal.account.tax_updated",
        "USER_EDIT",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"row_version": rv}))
}

// ---------------------------------------------------------------------------
// HTTP routes (merged into api::router)
// ---------------------------------------------------------------------------
async fn h_get(State(s): State<AppState>, h: HeaderMap) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, false).await?;
    Ok(Json(get_account(&s, &a).await?))
}
async fn h_identity(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<IdentityInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(verify_identity(&s, &a, i).await?))
}
async fn h_tax(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<TaxInput>,
) -> Result<Json<Value>> {
    let a = auth::actor(&s.pool, &h, &s.config, true).await?;
    Ok(Json(put_tax(&s, &a, i).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/me/account", get(h_get))
        .route("/api/me/identity", post(h_identity))
        .route("/api/me/tax", put(h_tax))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tax(foreign: bool, country: &str, tin: &str, us: bool) -> TaxInput {
        TaxInput {
            foreign,
            country: country.into(),
            tin: tin.into(),
            keep_tin: false,
            us_person: us,
            row_version: 0,
        }
    }

    #[test]
    fn ages() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(age_on(d("2007-10-10"), d("2026-10-10")), 19);
        assert_eq!(age_on(d("2007-10-11"), d("2026-10-10")), 18);
        assert_eq!(age_on(d("2008-02-29"), d("2026-02-28")), 17);
    }

    #[test]
    fn tax_rules() {
        assert_eq!(
            validate_tax(&tax(false, "", "", false)).unwrap(),
            (None, None, false)
        );
        assert!(validate_tax(&tax(false, "US", "", false)).is_err());
        assert_eq!(
            validate_tax(&tax(true, "us", "123-45-6789", true)).unwrap(),
            (Some("US".into()), Some("123456789".into()), true)
        );
        assert_eq!(
            validate_tax(&tax(true, "OTHER", "", false)).unwrap(),
            (Some("OTHER".into()), None, false)
        );
        assert!(matches!(
            validate_tax(&tax(true, "KR", "", false)),
            Err(Error::InvalidCode("TAX_COUNTRY_INVALID"))
        ));
        assert!(
            validate_tax(&tax(true, "JP", "", true)).is_err(),
            "US person only for US"
        );
        assert!(matches!(
            validate_tax(&tax(true, "JP", "12", false)),
            Err(Error::InvalidCode("TIN_INVALID"))
        ));
        assert!(matches!(
            validate_tax(&tax(true, "JP", "12<34>56", false)),
            Err(Error::InvalidCode("TIN_INVALID"))
        ));
    }
}
