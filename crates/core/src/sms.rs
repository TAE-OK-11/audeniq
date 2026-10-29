//! 문자(SMS) 인증번호 발송과 휴대폰 번호 처리.
//!
//! 발송 서비스는 `SMS_PROVIDER`로 고른다. 설정하지 않았거나 자격 증명이 비어 있으면
//! `Unconfigured`라 인증번호를 보내지 않고 `SMS_PROVIDER_NOT_CONFIGURED`로 거절한다.
//! - `solapi`: 솔라피 REST API (`POST /messages/v4/send`, HMAC-SHA256 인증).
//!   `SOLAPI_API_KEY`·`SOLAPI_API_SECRET`과 솔라피에 등록한 발신번호 `SMS_FROM`이 필요하다.
//!   보내지 못하면 `SMS_SEND_FAILED` (로그에는 HTTP 상태·솔라피 오류 코드만, 번호·본문은 남기지 않는다).
//! - `test`(운영 `APP_ENV=production`에서는 무시): 통합 테스트·로컬 시연용으로,
//!   실제로 보내지 않고 프로세스 안의 보관함에 남긴다.
//!
//! 휴대폰 번호 원문은 저장하지 않는다: 서비스 비밀값을 섞은 해시와 뒷자리 4개만 둔다.
use crate::config::env_value as env;
use crate::{
    domain::sha256_hex,
    error::{Error, Result},
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::{
    sync::{Mutex, OnceLock},
    time::Duration,
};

pub enum SmsSender {
    Unconfigured,
    Test,
    Solapi(Solapi),
}

/// 솔라피 계정 자격 증명과 발신번호.
pub struct Solapi {
    key: String,
    secret: String,
    from: String,
    base: String,
}

const SOLAPI_BASE: &str = "https://api.solapi.com";

impl Solapi {
    fn from_env() -> Option<Self> {
        let (Some(key), Some(secret), Some(from)) = (
            env("SOLAPI_API_KEY"),
            env("SOLAPI_API_SECRET"),
            env("SMS_FROM"),
        ) else {
            tracing::error!(
                "SMS_PROVIDER=solapi needs SOLAPI_API_KEY, SOLAPI_API_SECRET and SMS_FROM"
            );
            return None;
        };
        let from: String = from.chars().filter(char::is_ascii_digit).collect();
        if !(8..=12).contains(&from.len()) {
            tracing::error!("SMS_FROM must be the sender number registered with Solapi");
            return None;
        }
        Some(Self {
            key,
            secret,
            from,
            base: fixture().unwrap_or_else(|| SOLAPI_BASE.into()),
        })
    }

    /// `Authorization: HMAC-SHA256 apiKey=…, date=…, salt=…, signature=hex(HMAC(secret, date+salt))`
    fn authorization(&self, date: &str, salt: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.secret.as_bytes()).expect("HMAC key");
        mac.update(date.as_bytes());
        mac.update(salt.as_bytes());
        let signature = crate::hex::encode(mac.finalize().into_bytes());
        format!(
            "HMAC-SHA256 apiKey={}, date={date}, salt={salt}, signature={signature}",
            self.key
        )
    }

    async fn send(&self, phone: &str, text: &str) -> Result<()> {
        let date = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut salt = [0u8; 16];
        getrandom_fill(&mut salt);
        let salt = crate::hex::encode(salt);
        let body = serde_json::json!({"message": {"to": phone, "from": self.from, "text": text}});
        let failed = || Error::PolicyGate("SMS_SEND_FAILED");
        let r = http()
            .post(format!("{}/messages/v4/send", self.base))
            .header("authorization", self.authorization(&date, &salt))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "SMS provider request failed");
                failed()
            })?;
        let status = r.status();
        let v: serde_json::Value = r.json().await.unwrap_or_default();
        // 접수 성공은 2xx와 statusCode "2000". 그 밖은 솔라피 오류 코드만 남긴다.
        let code = v
            .get("statusCode")
            .or_else(|| v.get("errorCode"))
            .and_then(|c| c.as_str())
            .unwrap_or("");
        if status.is_success() && (code.is_empty() || code == "2000") {
            return Ok(());
        }
        tracing::warn!(
            status = status.as_u16(),
            provider_code = code,
            "SMS not accepted"
        );
        Err(failed())
    }
}

/// 로컬·테스트에서만 솔라피 대신 받을 주소 (`SMS_FIXTURE_BASE`, 운영에서는 무시).
fn fixture() -> Option<String> {
    let base = env("SMS_FIXTURE_BASE")?;
    let local = crate::external_http::local_http_enabled(true);
    crate::external_http::validate_url(&base, local)
        .ok()
        .map(|_| base.trim_end_matches('/').to_owned())
}

fn http() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        crate::external_http::client_builder(fixture().is_some())
            .timeout(Duration::from_secs(10))
            .build()
            .expect("HTTP client")
    })
}

/// `SMS_PROVIDER=test`가 보낸 문자 (휴대폰 번호, 본문). 테스트에서만 읽는다.
static TEST_OUTBOX: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

impl SmsSender {
    pub fn from_env() -> Self {
        let production = std::env::var("APP_ENV").as_deref() == Ok("production");
        match std::env::var("SMS_PROVIDER").as_deref() {
            Ok("test") if !production => Self::Test,
            Ok("solapi") => Solapi::from_env().map_or(Self::Unconfigured, Self::Solapi),
            _ => Self::Unconfigured,
        }
    }
    pub fn ready(&self) -> bool {
        !matches!(self, Self::Unconfigured)
    }
    pub async fn send(&self, phone: &str, text: &str) -> Result<()> {
        match self {
            Self::Unconfigured => Err(Error::PolicyGate("SMS_PROVIDER_NOT_CONFIGURED")),
            Self::Test => {
                TEST_OUTBOX
                    .lock()
                    .map_err(|_| Error::Internal)?
                    .push((phone.to_string(), text.to_string()));
                Ok(())
            }
            Self::Solapi(p) => p.send(phone, text).await,
        }
    }
}

/// 테스트 발송기로 이 번호에 마지막으로 보낸 문자의 6자리 인증번호.
pub fn test_last_code(phone: &str) -> Option<String> {
    let phone = normalize_phone(phone).ok()?;
    let outbox = TEST_OUTBOX.lock().ok()?;
    let (_, text) = outbox.iter().rev().find(|(p, _)| *p == phone)?;
    code_in(text)
}

fn code_in(text: &str) -> Option<String> {
    text.split(|c: char| !c.is_ascii_digit())
        .find(|w| w.len() == 6)
        .map(str::to_string)
}

/// 국내 휴대폰 번호(010·011·016·017·018·019)만, 숫자만 남긴다. 하이픈·공백 허용.
pub fn normalize_phone(raw: &str) -> Result<String> {
    if raw.len() > 20
        || raw
            .chars()
            .any(|c| !(c.is_ascii_digit() || c == '-' || c == ' '))
    {
        return Err(Error::InvalidCode("PHONE_INVALID"));
    }
    let d: String = raw.chars().filter(char::is_ascii_digit).collect();
    let ok = (d.len() == 10 || d.len() == 11)
        && d.starts_with("01")
        && matches!(d.as_bytes()[2], b'0' | b'1' | b'6' | b'7' | b'8' | b'9')
        && (d.as_bytes()[2] != b'0' || d.len() == 11);
    if ok {
        Ok(d)
    } else {
        Err(Error::InvalidCode("PHONE_INVALID"))
    }
}

/// 저장·대조용 휴대폰 번호 해시 (서비스 비밀값을 섞는다).
pub fn phone_hash(secret: &str, phone: &str) -> String {
    sha256_hex(format!("phone\0{secret}\0{phone}"))
}

/// 6자리 인증번호 (OS 난수, 균등 분포).
pub fn new_code() -> String {
    loop {
        let mut b = [0u8; 4];
        getrandom_fill(&mut b);
        let n = u32::from_be_bytes(b);
        // 4_294_000_000은 1_000_000의 배수: 그 아래만 쓰면 치우침이 없다.
        if n < 4_294_000_000 {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

fn getrandom_fill(buf: &mut [u8]) {
    use rand::{TryRng, rngs::SysRng};
    SysRng
        .try_fill_bytes(buf)
        .expect("operating system RNG unavailable");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phones() {
        assert_eq!(normalize_phone("010-1234-5678").unwrap(), "01012345678");
        assert_eq!(normalize_phone("011 234 5678").unwrap(), "0112345678");
        for bad in [
            "0101234567",
            "02-123-4567",
            "01512345678",
            "010-1234-567a",
            "+821012345678",
            "",
        ] {
            assert!(normalize_phone(bad).is_err(), "{bad}");
        }
        assert_ne!(
            phone_hash("a", "01012345678"),
            phone_hash("b", "01012345678")
        );
    }

    #[test]
    fn solapi_signature() {
        let p = Solapi {
            key: "NCSAYU7YDBXYORXC".into(),
            secret: "secret".into(),
            from: "0212345678".into(),
            base: SOLAPI_BASE.into(),
        };
        let a = p.authorization("2026-10-29T00:00:00Z", "abc");
        // HMAC-SHA256("secret", "2026-10-29T00:00:00Z" + "abc")
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(b"2026-10-29T00:00:00Zabc");
        let sig = crate::hex::encode(mac.finalize().into_bytes());
        assert_eq!(
            a,
            format!(
                "HMAC-SHA256 apiKey=NCSAYU7YDBXYORXC, date=2026-10-29T00:00:00Z, salt=abc, signature={sig}"
            )
        );
        assert_eq!(sig.len(), 64);
    }

    #[test]
    fn codes() {
        for _ in 0..200 {
            let c = new_code();
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
        assert_eq!(
            code_in("[AUDENIQ] 인증번호 012345 (3분 안에 입력)").as_deref(),
            Some("012345")
        );
    }
}
