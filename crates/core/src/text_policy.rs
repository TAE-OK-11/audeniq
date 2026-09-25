//! Input text hygiene for catalog metadata (titles, names, P/C lines...).
//!
//! Sandbox round 2: control characters (BEL/ESC/VT), NUL and bidi overrides
//! passed Stage 1/2 and then broke delivery-message generation (NUL even
//! returned a 500 because Postgres `text` cannot store it). Text is now
//! validated where it enters the system, with a clear 400, and re-checked in
//! Stage 1 so older data cannot slip through to packaging.
use crate::error::{Error, Result};
use serde_json::Value;

/// Error code returned for rejected text.
pub const TEXT_INVALID_CHARACTERS: &str = "TEXT_INVALID_CHARACTERS";

/// Characters that are never acceptable in delivered metadata:
/// - C0 controls (incl. NUL, BEL, ESC, VT), DEL and C1 controls;
/// - bidi embedding/override/isolate controls (U+202A..U+202E,
///   U+2066..U+2069), which can visually disguise text (e.g. `gnp.exe`);
/// - invisible separators used to evade matching: zero-width space (U+200B),
///   word joiner and invisible operators (U+2060..U+2064), BOM (U+FEFF),
///   soft hyphen (U+00AD), Mongolian vowel separator (U+180E).
///
/// ZWJ/ZWNJ (U+200C/U+200D, needed for emoji sequences and several scripts)
/// and LRM/RLM (U+200E/U+200F, used in correct Arabic/Hebrew text) are allowed.
/// `multiline` additionally allows `\n`, `\r` and `\t` (lyrics).
pub fn forbidden_char(c: char, multiline: bool) -> bool {
    match c {
        '\n' | '\r' | '\t' => !multiline,
        '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => true,
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => true,
        '\u{200b}' | '\u{2060}'..='\u{2064}' | '\u{feff}' | '\u{ad}' | '\u{180e}' => true,
        _ => false,
    }
}

/// True when `s` contains no forbidden character.
pub fn is_clean(s: &str, multiline: bool) -> bool {
    !s.chars().any(|c| forbidden_char(c, multiline))
}

/// Validate one single-line field.
pub fn check(s: &str) -> Result<()> {
    if is_clean(s, false) {
        Ok(())
    } else {
        Err(Error::InvalidCode(TEXT_INVALID_CHARACTERS))
    }
}

/// Validate a multi-line field (lyrics).
pub fn check_multiline(s: &str) -> Result<()> {
    if is_clean(s, true) {
        Ok(())
    } else {
        Err(Error::InvalidCode(TEXT_INVALID_CHARACTERS))
    }
}

/// Validate every string (keys and values) inside a JSON document.
pub fn check_json(v: &Value) -> Result<()> {
    if json_is_clean(v) {
        Ok(())
    } else {
        Err(Error::InvalidCode(TEXT_INVALID_CHARACTERS))
    }
}

/// Recursive JSON variant of [`is_clean`] (single-line rules, except that
/// strings under a `lyrics` key may be multi-line).
pub fn json_is_clean(v: &Value) -> bool {
    fn walk(v: &Value, multiline: bool) -> bool {
        match v {
            Value::String(s) => is_clean(s, multiline),
            Value::Array(a) => a.iter().all(|x| walk(x, multiline)),
            Value::Object(o) => o
                .iter()
                .all(|(k, x)| is_clean(k, false) && walk(x, k == "lyrics")),
            _ => true,
        }
    }
    walk(v, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_controls_nul_and_bidi_overrides() {
        for bad in [
            "Bad\u{7}\u{1b}[31mTitle\u{b}",
            "nul\u{0}byte",
            "Song \u{202e}gnp.exe",
            "B\u{200b}T\u{200b}S",
            "line\nbreak",
            "c1\u{85}",
        ] {
            assert!(check(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn accepts_real_world_text() {
        for ok in [
            "방탄소년단 (BTS)",
            "Beyoncé — Déjà Vu",
            "🔥🔥🔥",
            "👩‍👩‍👧 family",
            "مرحبا\u{200f} world",
            "Sigur Rós – Hoppípolla",
        ] {
            assert!(check(ok).is_ok(), "{ok:?}");
        }
        assert!(check_multiline("verse one\nverse two\r\n\tchorus").is_ok());
    }

    #[test]
    fn json_walks_nested_strings() {
        assert!(json_is_clean(
            &json!({"p_line":"℗ 2027","lyrics":"a\nb","x":[1,"ok"]})
        ));
        assert!(!json_is_clean(&json!({"artist":"A\u{7}"})));
        assert!(!json_is_clean(
            &json!({"nested":{"list":["ok","bad\u{0}"]}})
        ));
    }
}
