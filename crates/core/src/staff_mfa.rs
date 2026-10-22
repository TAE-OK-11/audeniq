//! Staff sign-in protection (개인정보취급자 접속 통제).
//!
//! - Idle timeout: a staff session unused for 30 minutes is revoked
//!   (개인정보의 안전성 확보조치 기준, NIST SP 800-63B AAL2 re-authentication).
//! - Second factor: TOTP (RFC 6238, HMAC-SHA1, 30 s, 6 digits), the format
//!   every authenticator app reads. Required in production
//!   (`STAFF_MFA_REQUIRED`); the session records when it was verified.
//!
//! The TOTP secret must be recomputed by the server, so it is sealed with the
//! personal-data keyring rather than hashed. Each 30-second step is accepted
//! once (`last_step`), ±1 step for clock drift. Verification costs one HMAC.
use crate::{
    api::AppState,
    auth::{self, Actor},
    error::{Error, Result},
    operations,
    payout_keys::Field,
};
use axum::http::HeaderMap;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use subtle::ConstantTimeEq;
use uuid::Uuid;

pub const IDLE_MINUTES: i64 = 30;
const STEP_SECONDS: i64 = 30;
const DIGITS: u32 = 6;
const ISSUER: &str = "AUDENIQ Admin";

/// RFC 4648 base32 without padding (authenticator apps' secret format).
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// RFC 4226 HOTP value for one counter.
pub fn hotp(secret: &[u8], counter: u64) -> u32 {
    use hmac::{Hmac, KeyInit, Mac};
    let mut mac = <Hmac<sha1::Sha1> as KeyInit>::new_from_slice(secret).expect("HMAC key");
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let offset = (h[19] & 0x0f) as usize;
    let bin = u32::from_be_bytes([
        h[offset] & 0x7f,
        h[offset + 1],
        h[offset + 2],
        h[offset + 3],
    ]);
    bin % 10u32.pow(DIGITS)
}

/// The step a 6-digit code matches (now ±1), if any and newer than `last`.
pub fn matching_step(secret: &[u8], code: &str, now: i64, last: Option<i64>) -> Option<i64> {
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let current = now.div_euclid(STEP_SECONDS);
    let mut found = None;
    // Check every candidate (constant work) and keep the newest match.
    for step in [current - 1, current, current + 1] {
        let expected = format!(
            "{:0width$}",
            hotp(secret, step as u64),
            width = DIGITS as usize
        );
        if bool::from(expected.as_bytes().ct_eq(code.as_bytes())) && last.is_none_or(|l| step > l) {
            found = Some(step);
        }
    }
    found
}

/// Session state the staff gate needs, read with the role in one query.
pub(crate) struct Gate {
    pub idle: bool,
    pub mfa: bool,
    pub enrolled: bool,
    pub seen_seconds: f64,
}

/// Revokes an idle staff session; refreshes `last_seen_at` at most once a
/// minute (one small UPDATE per session-minute, not per request).
pub(crate) async fn check_session(s: &AppState, actor: &Actor, gate: &Gate) -> Result<()> {
    if gate.idle {
        sqlx::query("UPDATE identity.sessions SET revoked_at=now() WHERE token_hash=$1 AND revoked_at IS NULL")
            .bind(&actor.session_hash)
            .execute(&s.pool)
            .await?;
        return Err(Error::Unauthorized);
    }
    if gate.seen_seconds >= 60.0 {
        sqlx::query("UPDATE identity.sessions SET last_seen_at=now() WHERE token_hash=$1")
            .bind(&actor.session_hash)
            .execute(&s.pool)
            .await?;
    }
    if crate::config::staff_mfa_required()? && !gate.mfa {
        return Err(Error::PolicyGate(if gate.enrolled {
            "STAFF_MFA_REQUIRED"
        } else {
            "STAFF_MFA_ENROLL_REQUIRED"
        }));
    }
    Ok(())
}

/// Staff member signed in with a live (not idle) session, before the second
/// factor: the MFA endpoints themselves.
async fn pre_mfa(s: &AppState, h: &HeaderMap, write: bool) -> Result<Actor> {
    let actor = auth::actor(&s.pool, h, &s.config, write).await?;
    let idle: Option<bool> = sqlx::query_scalar(
        "SELECT se.last_seen_at < now() - make_interval(mins => $3)
           FROM identity.sessions se JOIN identity.staff_members sm ON sm.user_id=se.user_id AND sm.status='ACTIVE'
          WHERE se.token_hash=$1 AND se.user_id=$2",
    )
    .bind(&actor.session_hash)
    .bind(actor.user)
    .bind(IDLE_MINUTES as i32)
    .fetch_optional(&s.pool)
    .await?;
    match idle {
        None => Err(Error::Forbidden),
        Some(true) => {
            check_session(
                s,
                &actor,
                &Gate {
                    idle: true,
                    mfa: false,
                    enrolled: false,
                    seen_seconds: 0.0,
                },
            )
            .await?;
            Err(Error::Unauthorized)
        }
        Some(false) => Ok(actor),
    }
}

pub async fn status(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let a = pre_mfa(s, h, false).await?;
    let row = sqlx::query(
        "SELECT m.enrolled_at, se.mfa_at FROM identity.sessions se
           LEFT JOIN identity.staff_mfa m ON m.user_id=se.user_id WHERE se.token_hash=$1",
    )
    .bind(&a.session_hash)
    .fetch_one(&s.pool)
    .await?;
    Ok(json!({
        "required": crate::config::staff_mfa_required()?,
        "enrolled": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("enrolled_at").is_some(),
        "verified": row.get::<Option<chrono::DateTime<chrono::Utc>>, _>("mfa_at").is_some(),
    }))
}

/// Starts (or restarts) enrollment while no code has been confirmed yet.
/// Returns the secret once, for the authenticator app.
pub async fn enroll(s: &AppState, h: &HeaderMap) -> Result<Value> {
    let a = pre_mfa(s, h, true).await?;
    auth::rate(&s.pool, &format!("staff-mfa-enroll:{}", a.user), 10).await?;
    let mut secret = zeroize::Zeroizing::new([0u8; 20]);
    {
        use rand::{TryRng, rngs::SysRng};
        SysRng
            .try_fill_bytes(&mut secret[..])
            .map_err(|_| Error::Internal)?;
    }
    let encoded = zeroize::Zeroizing::new(base32(&secret[..]));
    let sealed = s
        .payout_keys
        .seal_field(Field::StaffTotpSecret, a.user, &encoded)?;
    let replaced = sqlx::query(
        "INSERT INTO identity.staff_mfa(user_id,totp_cipher) VALUES($1,$2)
         ON CONFLICT(user_id) DO UPDATE SET totp_cipher=EXCLUDED.totp_cipher,last_step=NULL,updated_at=now()
           WHERE identity.staff_mfa.enrolled_at IS NULL",
    )
    .bind(a.user)
    .bind(&sealed)
    .execute(&s.pool)
    .await?
    .rows_affected();
    if replaced != 1 {
        // An enrolled factor is reset only by an owner (audeniq-admin).
        return Err(Error::PolicyGate("STAFF_MFA_ALREADY_ENROLLED"));
    }
    let email = crate::pii::user_email(&s.payout_keys, &s.pool, a.user).await?;
    let label = format!("{ISSUER}:{email}");
    let uri = format!(
        "otpauth://totp/{}?secret={}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
        urlencoding(&label),
        *encoded,
        urlencoding(ISSUER)
    );
    Ok(json!({"secret": &*encoded, "otpauth_uri": uri, "digits": DIGITS, "period": STEP_SECONDS}))
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyInput {
    pub code: String,
}

/// Confirms a code: finishes enrollment and marks this session verified.
pub async fn verify(s: &AppState, h: &HeaderMap, i: VerifyInput) -> Result<Value> {
    let a = pre_mfa(s, h, true).await?;
    let mut tx = s.pool.begin().await?;
    let enrolling = consume_code(s, &mut tx, &a, &i.code)
        .await?
        .ok_or(Error::PolicyGate("STAFF_MFA_ENROLL_REQUIRED"))?;
    sqlx::query(
        "UPDATE identity.sessions SET mfa_at=now(), last_seen_at=now() WHERE token_hash=$1",
    )
    .bind(&a.session_hash)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        if enrolling {
            "staff.mfa.enrolled"
        } else {
            "staff.mfa.verified"
        },
        "TOTP",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"verified": true}))
}

/// Checks and uses up one code. `Ok(None)`: no authenticator on file;
/// `Ok(Some(enrolling))`: accepted (true when it completed enrollment). A
/// wrong code is audited (committed here) and answers STAFF_MFA_CODE_INVALID.
async fn consume_code(
    s: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    a: &Actor,
    code: &str,
) -> Result<Option<bool>> {
    // 5 tries per 15 minutes per staff account (10^6 codes: ~0.0015% a window).
    auth::rate(&s.pool, &format!("staff-mfa-verify:{}", a.user), 5).await?;
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(row) = sqlx::query(
        "SELECT totp_cipher, enrolled_at, last_step FROM identity.staff_mfa WHERE user_id=$1 FOR UPDATE",
    )
    .bind(a.user)
    .fetch_optional(&mut **tx)
    .await?
    else {
        return Ok(None);
    };
    let secret = s.payout_keys.open_field(
        Field::StaffTotpSecret,
        a.user,
        row.get::<&[u8], _>("totp_cipher"),
    )?;
    let raw = zeroize::Zeroizing::new(base32_decode(&secret).ok_or(Error::Internal)?);
    let enrolling = row
        .get::<Option<chrono::DateTime<chrono::Utc>>, _>("enrolled_at")
        .is_none();
    let Some(step) = matching_step(
        &raw,
        &code,
        chrono::Utc::now().timestamp(),
        row.get("last_step"),
    ) else {
        // The failure is recorded even though the caller's work is refused.
        let mut log = s.pool.begin().await?;
        operations::audit(
            &mut log,
            Some(a.user),
            None,
            Some(a.user),
            "staff.mfa.failed",
            "TOTP_MISMATCH",
            a.request,
        )
        .await?;
        log.commit().await?;
        return Err(Error::InvalidCode("STAFF_MFA_CODE_INVALID"));
    };
    sqlx::query(
        "UPDATE identity.staff_mfa SET last_step=$2, enrolled_at=COALESCE(enrolled_at, now()), updated_at=now() WHERE user_id=$1",
    )
    .bind(a.user)
    .bind(step)
    .execute(&mut **tx)
    .await?;
    Ok(Some(enrolling))
}

/// Step-up for the admin console's riskiest operations (staff changes,
/// money, identifier ranges, partner go-live): a fresh authenticator code on
/// the request itself, even though the session already passed the second
/// factor. Without an enrolled authenticator there is nothing to check,
/// which only happens where MFA is not required (development).
pub async fn step_up(s: &AppState, a: &Actor, code: Option<&str>) -> Result<()> {
    let enrolled: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM identity.staff_mfa WHERE user_id=$1 AND enrolled_at IS NOT NULL)",
    )
    .bind(a.user)
    .fetch_one(&s.pool)
    .await?;
    if !enrolled {
        if crate::config::staff_mfa_required()? {
            return Err(Error::PolicyGate("STAFF_MFA_ENROLL_REQUIRED"));
        }
        return Ok(());
    }
    let code = code
        .filter(|c| !c.trim().is_empty())
        .ok_or(Error::PolicyGate("STAFF_STEP_UP_REQUIRED"))?;
    let mut tx = s.pool.begin().await?;
    consume_code(s, &mut tx, a, code).await?;
    operations::audit(
        &mut tx,
        Some(a.user),
        None,
        Some(a.user),
        "staff.mfa.step_up",
        "TOTP",
        a.request,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

pub fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 5 / 8);
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        buffer = (buffer << 5) | u32::from(v);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    Some(out)
}

/// Owner CLI: removes a staff member's factor (lost device). The next sign-in
/// enrolls again; their sessions are revoked.
pub async fn reset(pool: &sqlx::PgPool, operator: &str, user: Uuid) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM identity.staff_mfa WHERE user_id=$1")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE identity.sessions SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL",
    )
    .bind(user)
    .execute(&mut *tx)
    .await?;
    operations::audit(
        &mut tx,
        None,
        None,
        Some(user),
        "staff.mfa.reset",
        &format!("OPERATOR:{}", operator.trim()),
        Uuid::new_v4(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 Appendix B (SHA-1 seed "12345678901234567890"), 6-digit truncation.
    #[test]
    fn rfc6238_vectors() {
        let seed = b"12345678901234567890";
        for (t, code) in [
            (59i64, "287082"),
            (1111111109, "081804"),
            (1111111111, "050471"),
            (1234567890, "005924"),
            (2000000000, "279037"),
            (20000000000, "353130"),
        ] {
            assert_eq!(format!("{:06}", hotp(seed, (t / 30) as u64)), code, "t={t}");
            assert_eq!(matching_step(seed, code, t, None), Some(t / 30));
        }
    }

    #[test]
    fn codes_are_single_use_and_bounded_to_one_step_of_drift() {
        let seed = b"12345678901234567890";
        let t = 1234567890;
        let code = format!("{:06}", hotp(seed, (t / 30) as u64));
        let step = matching_step(seed, &code, t, None).unwrap();
        assert_eq!(
            matching_step(seed, &code, t, Some(step)),
            None,
            "replay refused"
        );
        assert!(
            matching_step(seed, &code, t + 30, None).is_some(),
            "one step late is accepted"
        );
        assert_eq!(
            matching_step(seed, &code, t + 90, None),
            None,
            "older codes expire"
        );
        for bad in ["", "12345", "1234567", "abcdef", "00000 "] {
            assert_eq!(matching_step(seed, bad, t, None), None);
        }
    }

    #[test]
    fn base32_round_trips_rfc4648() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(
            base32(b"12345678901234567890"),
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        );
        assert_eq!(base32_decode("MZXW6YTBOI").unwrap(), b"foobar");
        assert!(base32_decode("mzxw").is_none());
    }
}
