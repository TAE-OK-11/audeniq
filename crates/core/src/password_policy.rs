//! Password rules (NIST SP 800-63B §3.1.1.2, KISA 비밀번호 작성 규칙).
//!
//! Length 12–128 bytes is checked by the callers. On top of that a new
//! password must not be a commonly used or easily guessed value: a common
//! word or keyboard/sequence pattern (optionally followed by digits or
//! symbols), one short unit repeated, too few distinct characters, or the
//! account's own email name. No composition rules (NIST: they do not help).
//! Pure CPU work on a short string: no I/O, negligible cost.
use crate::error::{Error, Result};

/// Common words and keyboard walks that make up most guessed passwords,
/// including Korean phrases typed on a QWERTY layout (e.g. "dkssud" = 안녕).
const COMMON: &[&str] = &[
    "password",
    "passw0rd",
    "pass",
    "qwerty",
    "qwertyuiop",
    "asdf",
    "asdfgh",
    "asdfghjkl",
    "zxcv",
    "zxcvbnm",
    "qwer",
    "qwerasdf",
    "qazwsx",
    "iloveyou",
    "admin",
    "administrator",
    "welcome",
    "letmein",
    "login",
    "master",
    "monkey",
    "dragon",
    "sunshine",
    "princess",
    "football",
    "baseball",
    "soccer",
    "superman",
    "batman",
    "shadow",
    "trustno",
    "starwars",
    "whatever",
    "freedom",
    "hello",
    "hellokitty",
    "charlie",
    "michael",
    "jennifer",
    "abc",
    "abcd",
    "abcde",
    "abcdef",
    "test",
    "testtest",
    "guest",
    "user",
    "secret",
    "changeme",
    "default",
    "love",
    "lover",
    "sarang",
    "saranghae",
    "dkssud",
    "dkssudgktpdy",
    "tkfkdgo",
    "gkgkgk",
    "korea",
    "seoul",
    "audeniq",
    "music",
    "artist",
    "studio",
    "album",
    "kpop",
];

/// Sequences a password may be cut out of (forwards or backwards).
const SEQUENCES: &[&str] = &[
    "0123456789012345678901234567890",
    "abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz",
    "qwertyuiopasdfghjklzxcvbnmqwertyuiopasdfghjklzxcvbnm",
    "1q2w3e4r5t6y7u8i9o0p1q2w3e4r5t6y7u8i9o0p",
    "q1w2e3r4t5y6u7i8o9p0q1w2e3r4t5y6u7i8o9p0",
    "1qaz2wsx3edc4rfv5tgb6yhn7ujm8ik9ol0p",
    "qazwsxedcrfvtgbyhnujmikolpqazwsxedcrfvtgbyhnujmikolp",
    "!@#$%^&*()!@#$%^&*()",
];

fn is_sequence(p: &str) -> bool {
    SEQUENCES.iter().any(|s| {
        let rev: String = s.chars().rev().collect();
        s.contains(p) || rev.contains(p)
    })
}

fn repeats_unit(p: &[char]) -> bool {
    (1..=p.len() / 3).any(|n| p.len().is_multiple_of(n) && p.chunks(n).all(|c| c == &p[..n]))
}

/// Refuses a weak new password with `PASSWORD_TOO_WEAK`.
pub fn check(password: &str, email: &str) -> Result<()> {
    let lower = password.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut distinct = chars.clone();
    distinct.sort_unstable();
    distinct.dedup();
    // The word part with trailing digits/symbols removed ("password2024!").
    let word = lower.trim_end_matches(|c: char| !c.is_alphabetic());
    // ...and with leading digits/symbols removed too ("2024password").
    let core = word.trim_start_matches(|c: char| !c.is_alphabetic());
    let name = email.split('@').next().unwrap_or("").to_lowercase();
    let weak = distinct.len() < 5
        || repeats_unit(&chars)
        || is_sequence(&lower)
        || (!word.is_empty() && word.len() < lower.len() && is_sequence(word))
        || COMMON
            .iter()
            .any(|c| *c == core || (core.len() <= c.len() + 2 && core.starts_with(c)))
        || (core.chars().all(|c| c.is_ascii_alphabetic())
            && COMMON
                .iter()
                .any(|c| core == c.repeat(2) || core == c.repeat(3)))
        || (name.chars().count() >= 4 && lower.contains(&name));
    if weak {
        Err(Error::InvalidCode("PASSWORD_TOO_WEAK"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn common_patterns_and_the_email_name_are_refused() {
        for p in [
            "password1234",
            "Password2024!",
            "2024password",
            "qwertyuiop12",
            "1q2w3e4r5t6y",
            "123456789012",
            "abcdefghijkl",
            "aaaaaaaaaaaa",
            "abcabcabcabc",
            "121212121212",
            "dkssudgktpdy",
            "iloveyou1234",
            "audeniq12345",
            "jiminkim2024",
            "zxcvbnm12345",
            "0987654321098",
            "!@#$%^&*()!@",
        ] {
            assert!(check(p, "jiminkim@example.com").is_err(), "{p}");
        }
    }

    #[test]
    fn ordinary_long_passwords_pass() {
        for p in [
            "correct horse battery",
            "blue-river-73-lamp",
            "Tq8#vLm2pZ!r",
            "새벽세시의음악작업실",
            "my dog eats kimchi",
            "passage of tides 9",
        ] {
            assert!(check(p, "jiminkim@example.com").is_ok(), "{p}");
        }
    }
}
