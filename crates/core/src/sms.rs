//! 문자(SMS) 인증번호 발송과 휴대폰 번호 처리.
//!
//! 문자 발송은 발송 대행사(알리고·NHN Cloud SMS·솔라피 …) 계약이 필요하다. 계약 전에는
//! `Unconfigured`라 인증번호를 보내지 않고 `SMS_PROVIDER_NOT_CONFIGURED`로 거절한다.
//! `SMS_PROVIDER=test`(운영 `APP_ENV=production`에서는 무시)는 통합 테스트·로컬 시연용으로,
//! 실제로 보내지 않고 프로세스 안의 보관함에 남긴다.
//!
//! 업체를 붙일 때: `SmsSender`에 업체 변형을 더하고 `from_env`에서 `SMS_PROVIDER` 값과 자격 증명으로
//! 고른다 (자격 증명이 비면 `Unconfigured`). `send`가 실패하면 `SMS_SEND_FAILED`로 답하고, 로그에는
//! 번호·본문을 남기지 않는다. 호출하는 쪽(휴대폰 등록·비밀번호 찾기·이메일 찾기)은 바꿀 필요가 없다.
//!
//! 휴대폰 번호 원문은 저장하지 않는다: 서비스 비밀값을 섞은 해시와 뒷자리 4개만 둔다.
use crate::{
    domain::sha256_hex,
    error::{Error, Result},
};
use std::sync::Mutex;

pub enum SmsSender {
    Unconfigured,
    Test,
}

/// `SMS_PROVIDER=test`가 보낸 문자 (휴대폰 번호, 본문). 테스트에서만 읽는다.
static TEST_OUTBOX: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

impl SmsSender {
    pub fn from_env() -> Self {
        let production = std::env::var("APP_ENV").as_deref() == Ok("production");
        match std::env::var("SMS_PROVIDER").as_deref() {
            Ok("test") if !production => Self::Test,
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
