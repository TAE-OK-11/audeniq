//! Protected-artist names: hard block against artist impersonation.
//!
//! Sandbox round 2 delivered a release by "Taylor Swift" from a random
//! account. Names on the managed list (`catalog.protected_artists` plus
//! per-entry aliases and signature phrases such as "Taylor's Version") are
//! refused wherever they can appear: artist, label and organization names,
//! release and track titles/versions, the release profile (display artist,
//! featured artists) and credited party names. Organizations holding an
//! explicit exception (`catalog.protected_artist_exceptions`) are exempt.
//!
//! Matching is built to defeat evasion while leaving ordinary names alone:
//! each character is NFKC-normalized, stripped of invisible/format
//! characters and diacritics, lower-cased and folded through the Unicode
//! confusables skeleton (UTS #39: Cyrillic/Greek look-alikes, fullwidth,
//! `1`→`l`...). Everything that is not a letter or digit is dropped, so
//! inserted spaces/dots/dashes do not help ("T a y l o r", "T.aylor"). A hit
//! must start at a word start and end at a word end of the original text, so
//! "Taylor", "Swift" or "Taylor Swiftly" alone are not matches.
//!
//! See docs/PROTECTED_ARTISTS.md for policy and list management.
use crate::error::{Error, Result};
use sqlx::PgConnection;
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
use uuid::Uuid;

/// Error / check code for a protected-name hit.
pub const ARTIST_NAME_PROTECTED: &str = "ARTIST_NAME_PROTECTED";

/// Invisible or format characters dropped before matching.
fn invisible(c: char) -> bool {
    matches!(c,
        '\u{ad}' | '\u{34f}' | '\u{61c}' | '\u{115f}' | '\u{1160}' | '\u{17b4}' | '\u{17b5}'
        | '\u{180b}'..='\u{180f}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{206f}' | '\u{3164}' | '\u{fe00}'..='\u{fe0f}' | '\u{feff}'
        | '\u{ffa0}' | '\u{e0000}'..='\u{e007f}')
        || c.is_control()
}

/// Fold one character to its comparable form ("" for separators).
fn fold_char(c: char) -> String {
    let mut out = String::new();
    for n in c.to_string().nfkc() {
        if invisible(n) {
            continue;
        }
        // Case-fold first so ASCII letters compare case-insensitively, then
        // map look-alikes through the confusables skeleton. A non-ASCII letter
        // whose lower-case form has no prototype (Cyrillic т) is retried via
        // its capital (Т -> T).
        let lower: String = n.to_lowercase().collect();
        let mut skel: String = unicode_security::skeleton(&lower)
            .collect::<String>()
            .to_lowercase();
        if !skel.is_ascii() {
            let upper: String = n.to_uppercase().collect();
            let alt: String = unicode_security::skeleton(&upper)
                .collect::<String>()
                .to_lowercase();
            if alt.is_ascii() {
                skel = unicode_security::skeleton(&alt)
                    .collect::<String>()
                    .to_lowercase();
            }
        }
        for s in skel.to_lowercase().nfd() {
            if !is_combining_mark(s) && s.is_alphanumeric() {
                out.push(s);
            }
        }
    }
    out
}

/// Folded form of a whole needle (list entry): letters/digits only.
pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

/// True when `text` contains `needle` (already folded) on word boundaries.
fn contains_folded(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    // (folded char, index of the source "unit") plus per-unit word flags.
    let mut folded: Vec<(char, usize)> = Vec::new();
    let mut starts: Vec<bool> = Vec::new();
    let mut ends: Vec<bool> = Vec::new();
    let mut prev_alnum = false;
    for c in text.chars() {
        if c.to_string().nfkc().all(invisible) {
            continue; // invisible chars neither split nor join words
        }
        let f = fold_char(c);
        let alnum = !f.is_empty();
        if alnum {
            let unit = starts.len();
            starts.push(!prev_alnum);
            ends.push(true);
            if unit > 0 && prev_alnum {
                ends[unit - 1] = false;
            }
            folded.extend(f.chars().map(|x| (x, unit)));
        }
        prev_alnum = alnum;
    }
    let hay: Vec<char> = folded.iter().map(|(c, _)| *c).collect();
    let pat: Vec<char> = needle.chars().collect();
    if pat.len() > hay.len() {
        return false;
    }
    (0..=hay.len() - pat.len()).any(|i| {
        hay[i..i + pat.len()] == pat[..] && {
            let (first, last) = (folded[i].1, folded[i + pat.len() - 1].1);
            // must not start/end in the middle of one source char's expansion
            (i == 0 || folded[i - 1].1 != first)
                && (i + pat.len() == folded.len() || folded[i + pat.len()].1 != last)
                && starts[first]
                && ends[last]
        }
    })
}

/// A protected entry with its folded names and phrases.
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    folded: Vec<String>,
}

impl Entry {
    /// Build an entry from its display name plus aliases/phrases.
    pub fn new(name: &str, aliases: &[String]) -> Self {
        let mut folded: Vec<String> = std::iter::once(name.to_string())
            .chain(aliases.iter().cloned())
            .map(|s| fold(&s))
            .filter(|s| s.chars().count() >= 4)
            .collect();
        folded.sort();
        folded.dedup();
        Self {
            name: name.to_string(),
            folded,
        }
    }
    /// True when `text` names this protected artist.
    pub fn matches(&self, text: &str) -> bool {
        self.folded.iter().any(|n| contains_folded(text, n))
    }
}

/// First protected entry named by any of `texts`.
pub fn find<'a>(entries: &'a [Entry], texts: &[&str]) -> Option<&'a Entry> {
    entries.iter().find(|e| texts.iter().any(|t| e.matches(t)))
}

/// Active protected entries that apply to `org` (entries for which the org
/// holds an unrevoked exception are skipped).
pub async fn load(c: &mut PgConnection, org: Uuid) -> Result<Vec<Entry>> {
    let rows: Vec<(String, Vec<String>)> = sqlx::query_as(
        "SELECT p.name, COALESCE(array_agg(a.alias) FILTER (WHERE a.alias IS NOT NULL), '{}')
         FROM catalog.protected_artists p
         LEFT JOIN catalog.protected_artist_aliases a ON a.protected_artist_id=p.id
         WHERE p.active AND NOT EXISTS (
           SELECT 1 FROM catalog.protected_artist_exceptions x
           WHERE x.protected_artist_id=p.id AND x.org_id=$1 AND x.revoked_at IS NULL)
         GROUP BY p.id, p.name",
    )
    .bind(org)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows.iter().map(|(n, a)| Entry::new(n, a)).collect())
}

/// Refuse input that names a protected artist the org is not cleared for.
pub async fn enforce(c: &mut PgConnection, org: Uuid, texts: &[&str]) -> Result<()> {
    if texts.iter().all(|t| t.trim().is_empty()) {
        return Ok(());
    }
    let entries = load(c, org).await?;
    match find(&entries, texts) {
        Some(e) => {
            tracing::info!(%org, protected = %e.name, "protected artist name refused");
            Err(Error::PolicyGate(ARTIST_NAME_PROTECTED))
        }
        None => Ok(()),
    }
}

/// Every string inside a JSON value (profile documents).
pub fn json_strings(v: &serde_json::Value) -> Vec<&str> {
    let mut out = Vec::new();
    fn walk<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a str>) {
        match v {
            serde_json::Value::String(s) => out.push(s),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(v, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> Vec<Entry> {
        vec![Entry::new(
            "Taylor Swift",
            &["Taylor Alison Swift".into(), "Taylor's Version".into()],
        )]
    }

    #[test]
    fn blocks_name_and_evasion_variants() {
        let e = ts();
        for bad in [
            "Taylor Swift",
            "taylor swift",
            "TAYLOR SWIFT",
            "TaylorSwift",
            "T a y l o r  S w i f t",
            "T.aylor Swift",
            "T.a.y.l.o.r S.w.i.f.t",
            "Taylor-Swift",
            "Taylor_Swift",
            "Tay\u{200b}lor Sw\u{200d}ift",
            "Taylor \u{202e}Swift",
            "\u{feff}Taylor Swift",
            "Ｔａｙｌｏｒ Ｓｗｉｆｔ", // fullwidth
            "Тaylor Swift",            // Cyrillic Т
            "Taylοr Swift",            // Greek omicron
            "Tay1or Swift",            // digit one for l
            "Taylör Swíft",            // diacritics
            "𝐓𝐚𝐲𝐥𝐨𝐫 𝐒𝐰𝐢𝐟𝐭",            // math bold
            "Song (feat. Taylor Swift)",
            "Artist feat. Taylor Swift",
            "Artist ft. Taylor Swift",
            "Artist with Taylor Swift",
            "Artist x Taylor Swift",
            "Artist & Taylor Swift",
            "Love Story (Taylor's Version)",
            "Love Story (Taylors Version)",
            "Taylor Alison Swift",
        ] {
            assert!(find(&e, &[bad]).is_some(), "should block {bad:?}");
        }
    }

    #[test]
    fn leaves_unrelated_names_alone() {
        let e = ts();
        for ok in [
            "Taylor",
            "Swift",
            "Taylor Smith",
            "James Taylor",
            "Swift River",
            "Taylor Swiftly",
            "Taylormade Swifts",
            "My Version",
            "Taylor Made Version",
            "BTS",
            "",
        ] {
            assert!(find(&e, &[ok]).is_none(), "false positive on {ok:?}");
        }
    }

    #[test]
    fn short_aliases_are_ignored() {
        // Very short folded aliases would match everywhere; they are dropped.
        let e = vec![Entry::new("Ab", &[])];
        assert!(find(&e, &["Ab"]).is_none());
    }
}
